use elysiandb::datastore;

use super::corpus::{Engine, shutdown_engine};
use super::fixture::FixtureRequest;
use super::golden::{
    differs, expected_body, golden_full, intentional_change, matching, testdata, truncate,
};
use super::legacy::{copy_folder, legacy_read_requests, start_server_on_folder};

const FORMAT1_FIXTURES_DIR: &str = "pr-format1";

#[test]
fn folders_of_the_previous_format_are_upgraded() {
    let _engine = Engine;
    let want = golden_full();

    let mut errors = Vec::new();
    for variant in ["format1", "format1-crash"] {
        let folder = copy_folder(&testdata(FORMAT1_FIXTURES_DIR).join(variant));
        let folder = folder.path();

        for round in 0..2 {
            let mut s = start_server_on_folder(folder, true);
            let reqs = legacy_read_requests();
            for (i, r) in reqs.iter().enumerate() {
                let got = s.run(r);
                if intentional_change(&want[i].request) {
                    continue;
                }
                if differs(&got, &want[i], false) {
                    errors.push(format!(
                        "{variant} round {round} #{i} {} {}\nwant {} {}\ngot  {} {}",
                        r.method,
                        r.path,
                        want[i].status,
                        truncate(expected_body(&want[i], false)),
                        got.status,
                        truncate(&got.body)
                    ));
                }
            }

            if variant == "format1-crash" {
                let got = s.run(&FixtureRequest::new("GET", "/kv/extra:key", ""));
                if got.status != 200 || !got.body.contains(r#""value":"after the checkpoint""#) {
                    panic!(
                        "{variant} round {round}: the write after the last checkpoint is replayed, got {} {:?}",
                        got.status, got.body
                    );
                }
            }

            shutdown_engine();

            let manifest =
                std::fs::read_to_string(folder.join(datastore::MANIFEST_FILE)).unwrap_or_default();
            if !manifest.contains(r#""format": 2"#) {
                panic!(
                    "{variant} round {round}: the manifest is upgraded to format 2, got {manifest}"
                );
            }

            let kept = matching(
                &folder.join(datastore::LEGACY_CHECKPOINTS),
                "checkpoint-",
                ".ckpt",
            );
            let left = matching(folder, "checkpoint-", ".ckpt");
            if kept.len() != 1 || !left.is_empty() {
                panic!(
                    "{variant} round {round}: the previous checkpoint is kept aside, kept {kept:?} left {left:?}"
                );
            }
        }
    }

    if !errors.is_empty() {
        panic!("{}", errors.join("\n"));
    }
}
