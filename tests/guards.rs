mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use elysiandb::crypto::sha256;
use elysiandb::gocompat::json::Value;

use common::{hex, read_golden, read_json, str_field};

const GUARD_FILES: [&str; 3] = ["capabilities-go.tsv", "capabilities.tsv", "go-tests.tsv"];

const REMOVED_BY_D17: [&str; 3] = [
    "key\tmonitoring.pprof",
    "key\tmonitoring.pprof.enabled",
    "route\tGET /debug/pprof/{profile:*}",
];

const GO_TESTS: usize = 868;
const GO_FUZZ_SEED_TESTS: usize = 1;

const TARGETS: [(&str, &[&str]); 5] = [
    ("test/bench", &["tests/bench.rs", "tests/bench/main.rs"]),
    ("test/compat", &["tests/compat.rs", "tests/compat/main.rs"]),
    (
        "test/e2e/http",
        &["tests/e2e_http.rs", "tests/e2e_http/main.rs"],
    ),
    (
        "test/e2e/process",
        &["tests/e2e_process.rs", "tests/e2e_process/main.rs"],
    ),
    (
        "test/e2e/tcp",
        &["tests/e2e_tcp.rs", "tests/e2e_tcp/main.rs"],
    ),
];

const NOT_TRANSLATED_YET: [(&str, usize); 1] = [("test/bench", 2)];

const PLURAL_ACRONYMS: [(&str, &str); 2] = [("ACLs", "Acls"), ("IDs", "Ids")];

const ADDED_TO_TRANSLATED_TARGETS: [(&str, &[&str]); 7] = [
    (
        "test/internal/cache_test",
        &[
            "test_default_limits",
            "test_dependencies_make_entries_stale",
            "test_eviction_follows_the_sorted_order",
            "test_eviction_removes_ids_after_data",
            "test_size_accounting",
            "test_ttl_defaults_to_one_second",
        ],
    ),
    (
        "test/internal/hook_test",
        &[
            "deep_entities_go_through_the_output_memo_without_recursion",
            "deep_query_rows_are_copied_without_recursion",
            "deep_rows_built_on_the_calling_thread_do_not_recurse",
            "dotted_filters_reach_plain_nested_objects",
            "hook_bench_local",
            "hook_bench_parts",
            "hook_corpus_europe_paris",
            "hook_corpus_utc",
            "hook_plan_bytecode_matches_the_baseline",
            "query_views_stored_by_one_hook_are_read_again_by_the_next",
        ],
    ),
    (
        "test/internal/schema_test",
        &[
            "test_analyze_entity_schema_keeps_nested_ids_and_skips_core_fields",
            "test_schema_handles_9990_levels_on_a_small_stack",
            "test_validate_entity_displays_the_included_documents_type",
            "test_validate_entity_reports_errors_in_d20_order",
            "test_validate_entity_skips_ids_at_every_depth",
            "test_validate_entity_update_keeps_errors_of_updated_roots",
            "test_validation_error_to_error",
        ],
    ),
    (
        "test/internal/storage_test",
        &["test_deep_copy_document_copies_9990_levels_on_a_small_stack"],
    ),
    (
        "test/internal/transport_test/http_test/adminui_test",
        &["admin_ui_handler_serves_the_embedded_assets"],
    ),
    (
        "test/internal/transport_test/http_test/api_test/hook_test",
        &[
            "apply_post_read_hooks_keeps_the_order_of_the_documents",
            "create_hook_decodes_the_body_like_go",
        ],
    ),
    (
        "test/internal/transport_test/tcp_test/tcp_routing_test",
        &["test_unregistered_security_refuses_credentials"],
    ),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn guard_text(file: &str) -> String {
    String::from_utf8(read_golden(&format!("guards/{file}")))
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn rows(file: &str) -> Vec<String> {
    guard_text(file).lines().map(str::to_string).collect()
}

fn rust_sources(dir: &str) -> String {
    let folder = root().join(dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();

    let mut src = String::new();
    for f in files {
        src.push_str(
            &std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display())),
        );
        src.push('\n');
    }

    src
}

struct Field {
    type_name: String,
    tag: String,
}

fn parse_rust_structs(src: &str) -> BTreeMap<String, Vec<Field>> {
    let mut structs = BTreeMap::new();
    let mut current: Option<(String, Vec<Field>)> = None;
    for line in src.lines() {
        let line = line.trim();
        if let Some(name) = line
            .strip_prefix("pub struct ")
            .and_then(|r| r.strip_suffix('{'))
        {
            current = Some((name.trim().to_string(), Vec::new()));
            continue;
        }
        let Some((_, fields)) = current.as_mut() else {
            continue;
        };
        if line == "}" {
            if let Some((name, fields)) = current.take() {
                structs.insert(name, fields);
            }
            continue;
        }
        let Some((_, rest)) = line.split_once(": ") else {
            continue;
        };
        let Some((ty, tail)) = rest.split_once(" = \"") else {
            continue;
        };
        let Some((tag, _)) = tail.split_once('"') else {
            continue;
        };
        let ty = ty.trim();
        let ty = ["Option<", "Vec<", "Box<"]
            .iter()
            .find_map(|w| ty.strip_prefix(w))
            .map_or(ty, |t| t.strip_suffix('>').unwrap_or(t));
        if !tag.is_empty() && tag != "-" {
            fields.push(Field {
                type_name: ty.trim().to_string(),
                tag: tag.to_string(),
            });
        }
    }

    structs
}

fn collect_keys(
    structs: &BTreeMap<String, Vec<Field>>,
    name: &str,
    prefix: &str,
    keys: &mut BTreeSet<String>,
) {
    let Some(fields) = structs.get(name) else {
        return;
    };
    for f in fields {
        let key = if prefix.is_empty() {
            f.tag.clone()
        } else {
            format!("{prefix}.{}", f.tag)
        };
        keys.insert(key.clone());
        collect_keys(structs, &f.type_name, &key, keys);
    }
}

fn parse_rust_routes(src: &str) -> BTreeSet<String> {
    let mut routes = BTreeSet::new();
    for method in ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"] {
        let needle = format!(".handle(\"{method}\", \"");
        let mut rest = src;
        while let Some(pos) = rest.find(&needle) {
            rest = &rest[pos + needle.len()..];
            if let Some(end) = rest.find('"') {
                routes.insert(format!("{method} {}", &rest[..end]));
            }
        }
    }

    routes
}

fn parse_rust_commands(src: &str) -> BTreeSet<String> {
    src.lines()
        .filter(|l| l.trim_start().starts_with("pub const "))
        .filter_map(|l| l.split_once("_COMMAND: &str = \""))
        .filter_map(|(_, rest)| rest.split_once('"'))
        .map(|(value, _)| value.to_string())
        .collect()
}

fn of_kind(rows: &[String], kind: &str) -> usize {
    let prefix = format!("{kind}\t");
    rows.iter().filter(|r| r.starts_with(&prefix)).count()
}

#[test]
fn the_manifest_describes_the_guard_files() {
    let manifest = read_json("guards/MANIFEST.json");
    assert_eq!(str_field(&manifest, "part"), "guards");
    assert_eq!(str_field(&manifest, "go_version"), "go1.24.7");
    assert_eq!(str_field(&manifest, "baseline"), "1a11e5e");
    let files = manifest
        .get("files")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the manifest lists no files"));
    let listed: Vec<&str> = files.iter().map(|f| str_field(f, "path")).collect();
    assert_eq!(listed, GUARD_FILES);

    for entry in files {
        let file = str_field(entry, "path");
        let data = read_golden(&format!("guards/{file}"));
        assert_eq!(
            entry.get("size").and_then(Value::as_u64),
            Some(data.len() as u64),
            "{file}"
        );
        assert_eq!(
            str_field(entry, "sha256"),
            hex(&sha256::sum256(&data)),
            "{file}"
        );
    }
}

#[test]
fn the_rust_inventory_is_the_go_inventory_without_the_names_removed_by_d17() {
    let go = rows("capabilities-go.tsv");
    let rust = rows("capabilities.tsv");
    assert_eq!(
        (
            of_kind(&go, "key"),
            of_kind(&go, "route"),
            of_kind(&go, "command")
        ),
        (81, 63, 9)
    );
    assert_eq!(
        (
            of_kind(&rust, "key"),
            of_kind(&rust, "route"),
            of_kind(&rust, "command")
        ),
        (79, 62, 9)
    );

    for removed in REMOVED_BY_D17 {
        assert!(go.iter().any(|r| r == removed), "{removed}");
    }
    let kept: Vec<String> = go
        .into_iter()
        .filter(|r| !REMOVED_BY_D17.contains(&r.as_str()))
        .collect();
    assert_eq!(kept, rust);
}

#[test]
fn the_sources_declare_the_frozen_capabilities() {
    let mut keys = BTreeSet::new();
    collect_keys(
        &parse_rust_structs(&rust_sources("src/configuration")),
        "Config",
        "",
        &mut keys,
    );
    let routes = parse_rust_routes(&rust_sources("src/routing"));
    let commands = parse_rust_commands(&rust_sources("src/cmd"));
    assert_eq!((keys.len(), routes.len(), commands.len()), (79, 62, 9));

    let mut detected = String::new();
    for (kind, values) in [("key", &keys), ("route", &routes), ("command", &commands)] {
        for v in values {
            detected.push_str(&format!("{kind}\t{v}\n"));
        }
    }

    let frozen = guard_text("capabilities.tsv");
    let want: BTreeSet<&str> = frozen.lines().collect();
    let got: BTreeSet<&str> = detected.lines().collect();
    let missing: Vec<&&str> = want.difference(&got).collect();
    let added: Vec<&&str> = got.difference(&want).collect();
    assert!(
        missing.is_empty() && added.is_empty(),
        "missing {missing:?}\nadded {added:?}"
    );
    assert_eq!(detected, frozen);
}

fn target_roots(package: &str) -> Vec<PathBuf> {
    if let Some((_, roots)) = TARGETS.iter().find(|(p, _)| *p == package) {
        return roots.iter().map(|r| root().join(r)).collect();
    }

    let rel = package.strip_prefix("test/").unwrap_or(package);
    vec![root().join("tests").join(rel).join("main.rs")]
}

fn path_attribute(line: &str) -> Option<&str> {
    line.strip_prefix("#[path = \"")?.strip_suffix("\"]")
}

fn module_name(line: &str) -> Option<&str> {
    let line = line.strip_prefix("pub ").unwrap_or(line);
    let name = line.strip_prefix("mod ")?.strip_suffix(';')?;
    name.bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        .then_some(name)
}

fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }

    out
}

fn collect_files(file: &Path, is_root: bool, files: &mut Vec<PathBuf>) {
    let file = normalized(file);
    if files.contains(&file) {
        return;
    }
    let Ok(src) = std::fs::read_to_string(&file) else {
        return;
    };
    files.push(file.clone());

    let dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let modules = if is_root || stem == "main" || stem == "mod" {
        dir.clone()
    } else {
        dir.join(stem)
    };

    let mut attribute: Option<&str> = None;
    for line in src.lines() {
        let line = line.trim();
        if let Some(p) = path_attribute(line) {
            attribute = Some(p);
            continue;
        }
        if let Some(name) = module_name(line) {
            match attribute {
                Some(p) => collect_files(&dir.join(p), false, files),
                None => {
                    let flat = modules.join(format!("{name}.rs"));
                    if flat.exists() {
                        collect_files(&flat, false, files);
                    } else {
                        collect_files(&modules.join(name).join("mod.rs"), false, files);
                    }
                }
            }
        }
        attribute = None;
    }
}

fn function_name(line: &str) -> Option<&str> {
    let at = line.find("fn ")?;
    if at > 0 && !line[..at].ends_with(' ') {
        return None;
    }
    let rest = &line[at + 3..];
    let end = rest.find('(')?;
    let name = rest[..end].trim();
    (!name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
        .then_some(name)
}

fn test_functions(files: &[PathBuf]) -> Vec<String> {
    let mut names = Vec::new();
    for file in files {
        let src =
            std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim() != "#[test]" {
                continue;
            }
            if let Some(name) = lines[i + 1..].iter().find_map(|l| function_name(l)) {
                names.push(name.to_string());
            }
        }
    }

    names
}

fn snake_case(go_name: &str) -> String {
    let mut name = go_name.strip_prefix("Test").unwrap_or(go_name).to_string();
    for (acronym, word) in PLURAL_ACRONYMS {
        name = name.replace(acronym, word);
    }

    let mut out = String::new();
    for part in name.split('_').filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push('_');
        }
        let chars: Vec<char> = part.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            let before = i.checked_sub(1).map(|j| chars[j]);
            let after = chars.get(i + 1);
            let starts_a_word = c.is_ascii_uppercase()
                && before.is_some_and(|b| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || (b.is_ascii_uppercase() && after.is_some_and(char::is_ascii_lowercase))
                });
            if starts_a_word {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        }
    }

    out
}

fn rust_key(name: &str) -> String {
    name.strip_prefix("test_").unwrap_or(name).to_string()
}

#[test]
fn go_names_become_snake_case_names() {
    for (go, rust) in [
        ("TestGoldenCorpus", "golden_corpus"),
        ("TestCORSIsDisabledByDefault", "cors_is_disabled_by_default"),
        (
            "TestRequestIDIsKeptOrGenerated",
            "request_id_is_kept_or_generated",
        ),
        (
            "TestHTTPHandlerAddsRequestIDsAndRecoversPanics",
            "http_handler_adds_request_ids_and_recovers_panics",
        ),
        (
            "TestCanCreateEntity_ExistingEntityTypeWithoutACLGetsTheDefaultACLs",
            "can_create_entity_existing_entity_type_without_acl_gets_the_default_acls",
        ),
        (
            "TestExportLeavesAccountsAndACLsOut",
            "export_leaves_accounts_and_acls_out",
        ),
        (
            "TestResetDeletesKVKeysAndKeepsTheRESTAPIWorking",
            "reset_deletes_kv_keys_and_keeps_the_restapi_working",
        ),
        (
            "TestTCP_PING_SET_MGET_GET__WILDCARD__SAVE__RESET",
            "tcp_ping_set_mget_get_wildcard_save_reset",
        ),
        ("TestDELETESingle_HTTP", "delete_single_http"),
        (
            "TestAStartAfterACrashAnnouncesItsDuration",
            "a_start_after_a_crash_announces_its_duration",
        ),
        (
            "TestFullHeavyQueueAnswers503",
            "full_heavy_queue_answers503",
        ),
        (
            "TestInitDBMigratesA024FolderWithAPendingRecoveryLog",
            "init_db_migrates_a024_folder_with_a_pending_recovery_log",
        ),
        (
            "TestKill9LosesNoAcknowledgedWrite",
            "kill9_loses_no_acknowledged_write",
        ),
        (
            "TestCheckBasicAuthentication_InvalidBase64",
            "check_basic_authentication_invalid_base64",
        ),
        ("FuzzReplayNeverPanics", "fuzz_replay_never_panics"),
    ] {
        assert_eq!(snake_case(go), rust, "{go}");
    }
}

#[test]
fn every_go_test_is_translated_once_under_its_name() {
    let mut packages: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut tests = 0;
    let mut fuzz = 0;
    for row in rows("go-tests.tsv") {
        let (package, name) = row
            .split_once('\t')
            .unwrap_or_else(|| panic!("malformed row {row:?}"));
        if name.starts_with("Fuzz") {
            fuzz += 1;
        } else {
            tests += 1;
        }
        packages
            .entry(package.to_string())
            .or_default()
            .push(name.to_string());
    }
    assert_eq!((tests, fuzz), (GO_TESTS, GO_FUZZ_SEED_TESTS));

    let mut problems = Vec::new();
    for (package, _) in ADDED_TO_TRANSLATED_TARGETS {
        if !packages.contains_key(package) {
            problems.push(format!("{package}: listed with added tests, unknown to Go"));
        }
    }
    for (package, _) in NOT_TRANSLATED_YET {
        if !packages.contains_key(package) {
            problems.push(format!(
                "{package}: listed as not translated, unknown to Go"
            ));
        }
    }

    for (package, go_names) in &packages {
        let pending = NOT_TRANSLATED_YET.iter().find(|(p, _)| p == package);
        let Some(target) = target_roots(package).into_iter().find(|r| r.exists()) else {
            match pending {
                Some((_, n)) if *n == go_names.len() => {}
                Some((_, n)) => problems.push(format!(
                    "{package}: {n} tests listed as not translated, Go has {}",
                    go_names.len()
                )),
                None => problems.push(format!(
                    "{package}: no Rust target for its {} tests",
                    go_names.len()
                )),
            }
            continue;
        };
        if pending.is_some() {
            problems.push(format!(
                "{package}: translated in {}, still listed as not translated",
                target.display()
            ));
        }

        let mut files = Vec::new();
        collect_files(&target, true, &mut files);
        let rust_names = test_functions(&files);

        let mut translated: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for name in &rust_names {
            translated.entry(rust_key(name)).or_default().push(name);
        }

        let mut wanted = BTreeSet::new();
        for name in go_names {
            let key = snake_case(name);
            match translated.get(&key).map(Vec::len).unwrap_or(0) {
                1 => {}
                0 => problems.push(format!("{package}: {name} is not translated as {key}")),
                n => problems.push(format!("{package}: {name} is translated {n} times")),
            }
            if !wanted.insert(key) {
                problems.push(format!("{package}: {name} collides with another Go name"));
            }
        }

        let allowed: &[&str] = ADDED_TO_TRANSLATED_TARGETS
            .iter()
            .find(|(p, _)| p == package)
            .map(|(_, names)| *names)
            .unwrap_or(&[]);
        for name in &rust_names {
            if !wanted.contains(&rust_key(name)) && !allowed.contains(&name.as_str()) {
                problems.push(format!("{package}: {name} is not a Go test"));
            }
        }
        for name in allowed {
            if !rust_names.iter().any(|n| n == name) {
                problems.push(format!(
                    "{package}: {name} is listed as added, the target does not have it"
                ));
            }
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
