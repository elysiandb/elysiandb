mod common;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use talos::func::NativeFn;
use talos::object::FunctionCall;
use talos::runtime::{InterruptHandle, JsResult, RunError, Throw};
use talos::string::STRING_EMPTY;
use talos::{CompilerSyntaxError, Program, Runtime, Value, compile, must_compile};

const PINNED_COMMIT: &str = "cb4a6c8074671c00df8cbc17a620c0f9462b312a";

const SUITES: &[&str] = &[
    "test/language",
    "test/built-ins",
    "test/annexB/built-ins/String/prototype/substr",
    "test/annexB/built-ins/String/prototype/trimLeft",
    "test/annexB/built-ins/String/prototype/trimRight",
    "test/annexB/built-ins/escape",
    "test/annexB/built-ins/unescape",
    "test/annexB/built-ins/RegExp",
];

const SKIP_LIST: &[&str] = &[
    "test/language/expressions/prefix-increment/S11.4.4_A6_T3.js",
    "test/language/expressions/prefix-increment/S11.4.4_A6_T2.js",
    "test/language/expressions/prefix-increment/S11.4.4_A6_T1.js",
    "test/language/expressions/prefix-decrement/S11.4.5_A6_T3.js",
    "test/language/expressions/prefix-decrement/S11.4.5_A6_T2.js",
    "test/language/expressions/prefix-decrement/S11.4.5_A6_T1.js",
    "test/language/expressions/postfix-increment/S11.3.1_A6_T3.js",
    "test/language/expressions/postfix-increment/S11.3.1_A6_T2.js",
    "test/language/expressions/postfix-increment/S11.3.1_A6_T1.js",
    "test/language/expressions/postfix-decrement/S11.3.2_A6_T3.js",
    "test/language/expressions/postfix-decrement/S11.3.2_A6_T2.js",
    "test/language/expressions/postfix-decrement/S11.3.2_A6_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.1_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.1_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.1_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.11_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.11_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.11_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.10_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.10_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.10_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.9_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.9_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.9_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.8_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.8_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.8_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.7_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.7_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.7_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.6_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.6_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.6_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.5_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.5_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.5_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.4_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.4_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.4_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.3_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.3_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.3_T1.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.2_T4.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.2_T2.js",
    "test/language/expressions/compound-assignment/S11.13.2_A7.2_T1.js",
    "test/language/expressions/assignment/S11.13.1_A7_T3.js",
    "test/built-ins/Date/prototype/toISOString/15.9.5.43-0-8.js",
    "test/built-ins/Date/prototype/toISOString/15.9.5.43-0-9.js",
    "test/built-ins/Date/prototype/toISOString/15.9.5.43-0-10.js",
    "test/built-ins/Date/UTC/fp-evaluation-order.js",
    "test/built-ins/RegExp/quantifier-integer-limit.js",
    "test/built-ins/Function/internals/Construct/base-ctor-revoked-proxy.js",
    "test/language/expressions/class/elements/private-getter-is-not-a-own-property.js",
    "test/language/expressions/class/elements/private-setter-is-not-a-own-property.js",
    "test/language/statements/class/elements/private-setter-is-not-a-own-property.js",
    "test/language/statements/class/elements/private-getter-is-not-a-own-property.js",
    "test/built-ins/RegExp/unicode_restricted_quantifiable_assertion.js",
    "test/built-ins/RegExp/unicode_restricted_octal_escape.js",
    "test/built-ins/RegExp/unicode_restricted_incomple_quantifier.js",
    "test/built-ins/RegExp/unicode_restricted_incomplete_quantifier.js",
    "test/built-ins/RegExp/unicode_restricted_identity_escape_x.js",
    "test/built-ins/RegExp/unicode_restricted_identity_escape_u.js",
    "test/built-ins/RegExp/unicode_restricted_identity_escape_c.js",
    "test/built-ins/RegExp/unicode_restricted_identity_escape_alpha.js",
    "test/built-ins/RegExp/unicode_restricted_identity_escape.js",
    "test/built-ins/RegExp/unicode_restricted_brackets.js",
    "test/built-ins/RegExp/unicode_restricted_character_class_escape.js",
    "test/annexB/built-ins/RegExp/prototype/compile/pattern-string-invalid-u.js",
    "test/annexB/built-ins/RegExp/RegExp-leading-escape-BMP.js",
    "test/annexB/built-ins/RegExp/RegExp-trailing-escape-BMP.js",
    "test/language/literals/regexp/S7.8.5_A1.4_T2.js",
    "test/language/literals/regexp/S7.8.5_A1.1_T2.js",
    "test/language/literals/regexp/S7.8.5_A2.1_T2.js",
    "test/language/literals/regexp/S7.8.5_A2.4_T2.js",
    "test/language/expressions/optional-chaining/member-expression.js",
    "test/language/expressions/class/elements/same-line-async-method-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/expressions/class/elements/same-line-async-method-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/destructuring/binding/syntax/destructuring-object-parameters-function-arguments-length.js",
    "test/language/destructuring/binding/syntax/destructuring-array-parameters-function-arguments-length.js",
    "test/language/comments/hashbang/function-constructor.js",
    "test/language/statements/class/elements/after-same-line-static-async-method-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/statements/class/elements/after-same-line-static-async-method-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/statements/class/elements/same-line-async-method-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/statements/class/elements/same-line-async-method-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/expressions/class/elements/after-same-line-static-async-method-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/expressions/class/elements/after-same-line-static-async-method-rs-static-async-generator-method-privatename-identifier.js",
    "test/built-ins/Object/seal/seal-asyncgeneratorfunction.js",
    "test/language/statements/switch/scope-lex-async-generator.js",
    "test/language/statements/class/elements/private-async-generator-method-name.js",
    "test/language/expressions/class/elements/private-async-generator-method-name.js",
    "test/language/expressions/async-generator/name.js",
    "test/language/statements/class/elements/same-line-gen-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/statements/class/elements/same-line-gen-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/statements/class/elements/new-sc-line-gen-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/statements/class/elements/new-sc-line-gen-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/statements/class/elements/after-same-line-static-gen-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/statements/class/elements/after-same-line-static-gen-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/statements/class/elements/after-same-line-gen-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/statements/class/elements/after-same-line-gen-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/expressions/class/elements/same-line-gen-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/expressions/class/elements/same-line-gen-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/expressions/class/elements/new-sc-line-gen-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/expressions/class/elements/new-sc-line-gen-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/expressions/class/elements/after-same-line-static-gen-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/expressions/class/elements/after-same-line-static-gen-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/language/expressions/class/elements/after-same-line-gen-rs-static-async-generator-method-privatename-identifier.js",
    "test/language/expressions/class/elements/after-same-line-gen-rs-static-async-generator-method-privatename-identifier-alt.js",
    "test/built-ins/GeneratorFunction/is-a-constructor.js",
    "test/language/expressions/optional-chaining/iteration-statement-for-await-of.js",
    "test/language/literals/numeric/non-octal-decimal-integer.js",
    "test/language/literals/string/S7.8.4_A4.3_T2.js",
    "test/language/literals/string/S7.8.4_A4.3_T1.js",
    "test/language/literals/regexp/invalid-range-negative-lookbehind.js",
    "test/language/literals/regexp/invalid-range-lookbehind.js",
    "test/language/literals/regexp/invalid-optional-negative-lookbehind.js",
    "test/language/literals/regexp/invalid-optional-lookbehind.js",
    "test/built-ins/RegExp/unicode_full_case_folding.js",
    "test/language/expressions/assignment/fn-name-lhs-cover.js",
    "test/annexB/built-ins/RegExp/RegExp-invalid-control-escape-character-class.js",
    "test/annexB/built-ins/RegExp/RegExp-control-escape-russian-letter.js",
    "test/built-ins/String/prototype/replaceAll/searchValue-replacer-RegExp-call.js",
    "test/built-ins/RegExp/nullable-quantifier.js",
    "test/built-ins/RegExp/lookahead-quantifier-match-groups.js",
];

const SKIP_PREFIXES: &[&str] = &[
    "test/built-ins/Async",
    "test/language/statements/class/elements/wrapped-in-sc-rs-static-async-generator-",
    "test/language/statements/class/elements/same-line-method-rs-static-async-generator-",
    "test/language/statements/class/elements/regular-definitions-rs-static-async-generator-",
    "test/language/statements/class/elements/private-static-async-generator-",
    "test/language/statements/class/elements/new-sc-line-method-rs-static-async-generator-",
    "test/language/statements/class/elements/multiple-stacked-definitions-rs-static-async-generator-",
    "test/language/statements/class/elements/new-no-sc-line-method-rs-static-async-generator-",
    "test/language/statements/class/elements/multiple-definitions-rs-static-async-generator-",
    "test/language/statements/class/elements/after-same-line-static-method-rs-static-async-generator-",
    "test/language/statements/class/elements/after-same-line-method-rs-static-async-generator-",
    "test/language/expressions/class/elements/wrapped-in-sc-rs-static-async-generator-",
    "test/language/expressions/class/elements/same-line-method-rs-static-async-generator-",
    "test/language/expressions/class/elements/regular-definitions-rs-static-async-generator-",
    "test/language/expressions/class/elements/private-static-async-generator-",
    "test/language/expressions/class/elements/new-sc-line-method-rs-static-async-generator-",
    "test/language/expressions/class/elements/multiple-stacked-definitions-rs-static-async-generator-",
    "test/language/expressions/class/elements/new-no-sc-line-method-rs-static-async-generator-",
    "test/language/expressions/class/elements/multiple-definitions-rs-static-async-generator-",
    "test/language/expressions/class/elements/after-same-line-static-method-rs-static-async-generator-",
    "test/language/expressions/class/elements/after-same-line-method-rs-static-async-generator-",
    "test/language/eval-code/direct/async-gen-",
    "test/language/literals/regexp/u-",
    "test/language/literals/string/legacy-octal-",
    "test/language/literals/string/legacy-non-octal-",
    "test/language/export/",
    "test/language/import/",
    "test/language/module-code/",
];

const FEATURES_BLACK_LIST: &[&str] = &[
    "async-iteration",
    "Symbol.asyncIterator",
    "resizable-arraybuffer",
    "regexp-named-groups",
    "regexp-duplicate-named-groups",
    "regexp-unicode-property-escapes",
    "regexp-match-indices",
    "regexp-modifiers",
    "RegExp.escape",
    "legacy-regexp",
    "tail-call-optimization",
    "Temporal",
    "import-assertions",
    "dynamic-import",
    "import.meta",
    "Atomics",
    "Atomics.waitAsync",
    "Atomics.pause",
    "FinalizationRegistry",
    "WeakRef",
    "__getter__",
    "__setter__",
    "ShadowRealm",
    "SharedArrayBuffer",
    "decorators",
    "regexp-v-flag",
    "iterator-helpers",
    "symbols-as-weakmap-keys",
    "uint8array-base64",
    "String.prototype.toWellFormed",
    "explicit-resource-management",
    "set-methods",
    "promise-try",
    "promise-with-resolvers",
    "array-grouping",
    "Math.sumPrecise",
    "Float16Array",
    "arraybuffer-transfer",
    "Array.fromAsync",
    "String.prototype.isWellFormed",
];

const SAB_STUB: &str = r#"
		Object.defineProperty(this, "SharedArrayBuffer", {
			get: function() {
				throw IgnorableTestError;
			}
		});"#;

#[derive(Debug, Default, PartialEq)]
struct Meta {
    negative_phase: String,
    negative_type: String,
    includes: Vec<String>,
    flags: Vec<String>,
    features: Vec<String>,
    es5id: String,
}

impl Meta {
    fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

struct Entry {
    key: String,
    first: String,
    body: Vec<String>,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn split_key(text: &str) -> Option<(&str, &str)> {
    let colon = text.find(':')?;
    let key = &text[..colon];
    let rest = &text[colon + 1..];
    if key.is_empty() || key.contains([' ', '\t', '"', '\'', '[', '{', '#']) {
        return None;
    }
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some((key, rest))
}

fn strip_comment(text: &str) -> &str {
    if text.starts_with('#') {
        return "";
    }
    let mut prev_space = false;
    for (i, c) in text.char_indices() {
        if c == '#' && prev_space {
            return &text[..i];
        }
        prev_space = c == ' ' || c == '\t';
    }
    text
}

fn unquote(text: &str) -> String {
    let t = text.trim();
    if t.len() >= 2
        && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')))
    {
        return t[1..t.len() - 1].to_string();
    }
    t.to_string()
}

fn front_matter_entries(yaml: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut base: Option<usize> = None;
    let yaml = yaml.replace("\r\n", "\n");
    for line in yaml.split(['\n', '\r']) {
        let text = line.trim_start_matches([' ', '\t']);
        if text.is_empty() || text.starts_with('#') {
            if let Some(e) = entries.last_mut() {
                e.body.push(String::new());
            }
            continue;
        }
        let indent = indent_of(line);
        let top = *base.get_or_insert(indent);
        if indent == top
            && !text.starts_with('-')
            && let Some((key, rest)) = split_key(text)
        {
            entries.push(Entry {
                key: key.to_string(),
                first: rest.trim().to_string(),
                body: Vec::new(),
            });
            continue;
        }
        if let Some(e) = entries.last_mut() {
            e.body.push(line.to_string());
        }
    }
    entries
}

fn string_list(e: &Entry) -> Result<Vec<String>, String> {
    let first = strip_comment(&e.first).trim().to_string();
    if first.starts_with('[') {
        let mut joined = first;
        for l in &e.body {
            joined.push(' ');
            joined.push_str(strip_comment(l.trim()));
        }
        let Some(end) = joined.find(']') else {
            return Err(format!("{}: unterminated flow sequence", e.key));
        };
        if !joined[end + 1..].trim().is_empty() {
            return Err(format!("{}: unexpected text after the sequence", e.key));
        }
        return Ok(joined[1..end]
            .split(',')
            .map(unquote)
            .filter(|s| !s.is_empty())
            .collect());
    }
    if !first.is_empty() {
        return Err(format!("{}: cannot use a scalar as a sequence", e.key));
    }
    let mut out = Vec::new();
    for l in &e.body {
        let t = strip_comment(l.trim()).trim();
        if t.is_empty() {
            continue;
        }
        let Some(item) = t.strip_prefix('-') else {
            return Err(format!("{}: expected a sequence item", e.key));
        };
        out.push(unquote(item));
    }
    Ok(out)
}

fn scalar(e: &Entry) -> String {
    let first = strip_comment(&e.first).trim().to_string();
    if first.starts_with(['|', '>']) {
        return e
            .body
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }
    let mut parts = Vec::new();
    if !first.is_empty() {
        parts.push(first);
    }
    for l in &e.body {
        let t = strip_comment(l.trim()).trim();
        if !t.is_empty() {
            parts.push(t.to_string());
        }
    }
    let joined = parts.join(" ");
    if matches!(joined.as_str(), "~" | "null" | "Null" | "NULL") {
        return String::new();
    }
    unquote(&joined)
}

fn parse_meta(yaml: &str) -> Result<Meta, String> {
    let mut meta = Meta::default();
    for e in front_matter_entries(yaml) {
        match e.key.as_str() {
            "includes" => meta.includes = string_list(&e)?,
            "flags" => meta.flags = string_list(&e)?,
            "features" => meta.features = string_list(&e)?,
            "es5id" => meta.es5id = scalar(&e),
            "negative" => {
                if !strip_comment(&e.first).trim().is_empty() {
                    return Err("negative: expected a block mapping".to_string());
                }
                for l in &e.body {
                    let t = l.trim();
                    if t.is_empty() {
                        continue;
                    }
                    let Some((key, rest)) = split_key(t) else {
                        return Err("negative: expected a mapping entry".to_string());
                    };
                    let value = unquote(strip_comment(rest.trim()));
                    match key {
                        "phase" => meta.negative_phase = value,
                        "type" => meta.negative_type = value,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(meta)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_tc39_file(src: &[u8]) -> Result<Meta, String> {
    let Some(start) = find(src, b"/*---") else {
        return Err("Invalid file format".to_string());
    };
    let start = start + 5;
    let end = match find(src, b"---*/") {
        Some(end) if end > start => end,
        _ => return Err("Invalid file format".to_string()),
    };
    let meta = parse_meta(&String::from_utf8_lossy(&src[start..end]))?;
    if !meta.negative_type.is_empty() && meta.negative_phase.is_empty() {
        return Err("negative type is set, but phase isn't".to_string());
    }
    Ok(meta)
}

#[derive(Clone, Debug, PartialEq)]
enum Outcome {
    Pass,
    Skip(String),
    Fail(String),
}

enum ScriptError {
    Io(String),
    Syntax(CompilerSyntaxError),
    Run(RunError),
}

impl ScriptError {
    fn text(&self) -> String {
        match self {
            ScriptError::Io(e) => e.clone(),
            ScriptError::Syntax(e) => e.to_string(),
            ScriptError::Run(RunError::Exception(e)) => e.string(),
            ScriptError::Run(e) => e.error(),
        }
    }
}

#[derive(Default)]
struct Watch {
    slot: Mutex<Option<(Instant, InterruptHandle)>>,
}

impl Watch {
    fn arm(&self, handle: InterruptHandle) {
        *self.slot.lock().expect("watch") = Some((Instant::now(), handle));
    }

    fn disarm(&self) {
        *self.slot.lock().expect("watch") = None;
    }

    fn check(&self, limit: Duration) {
        if let Some((started, handle)) = &*self.slot.lock().expect("watch")
            && started.elapsed() > limit
        {
            handle.interrupt("test262 time limit");
        }
    }
}

struct Ctx {
    base: PathBuf,
    prg_cache: Mutex<HashMap<String, Arc<Program>>>,
    sab_stub: Arc<Program>,
    gc_stress: bool,
}

fn throw_of(e: RunError) -> Throw {
    match e {
        RunError::Exception(ex) => Throw::Exception(ex),
        RunError::Interrupted(e) => Throw::Interrupted(e),
        RunError::StackOverflow(e) => Throw::StackOverflow(e),
    }
}

fn detach_array_buffer(rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    if let Value::Object(o) = call.arg(0)
        && let Some(data) = rt.array_buffer_data(*o)
    {
        data.borrow_mut().detach();
        return Ok(Value::Undefined);
    }
    Err(Throw::TypeError(
        "detachArrayBuffer() is called with incompatible argument".to_string(),
    ))
}

fn eval_script(rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let script = call.argument(0).go_string(rt)?;
    rt.run_string(&script).map_err(throw_of)
}

fn log(_rt: &mut Runtime, _call: FunctionCall<'_>) -> JsResult<Value> {
    Ok(Value::Undefined)
}

impl Ctx {
    fn new(base: PathBuf) -> Ctx {
        Ctx {
            base,
            prg_cache: Mutex::new(HashMap::new()),
            sab_stub: must_compile("sabStub.js", SAB_STUB.as_bytes(), false),
            gc_stress: std::env::var("TALOS_GC_STRESS").is_ok_and(|v| v == "1"),
        }
    }

    fn compile(&self, name: &str) -> Result<Arc<Program>, ScriptError> {
        let mut cache = self.prg_cache.lock().expect("program cache");
        if let Some(p) = cache.get(name) {
            return Ok(p.clone());
        }
        let src = std::fs::read(self.base.join(name))
            .map_err(|e| ScriptError::Io(format!("open {name}: {e}")))?;
        let prg = compile(name, &src, false).map_err(ScriptError::Syntax)?;
        cache.insert(name.to_string(), prg.clone());
        Ok(prg)
    }

    fn run_file(&self, name: &str, rt: &mut Runtime) -> Result<(), ScriptError> {
        let prg = self.compile(name)?;
        rt.run_program(&prg).map_err(ScriptError::Run)?;
        Ok(())
    }

    fn run_tc39_script(
        &self,
        name: &str,
        src: &[u8],
        includes: &[String],
        rt: &mut Runtime,
    ) -> Result<(), (ScriptError, bool)> {
        self.run_file("harness/assert.js", rt)
            .map_err(|e| (e, true))?;
        self.run_file("harness/sta.js", rt).map_err(|e| (e, true))?;
        for include in includes {
            self.run_file(&format!("harness/{include}"), rt)
                .map_err(|e| (e, true))?;
        }
        let p = compile(name, src, false).map_err(|e| (ScriptError::Syntax(e), true))?;
        rt.run_program(&p)
            .map_err(|e| (ScriptError::Run(e), false))?;
        Ok(())
    }

    fn error_type(rt: &mut Runtime, name: &str, err: &ScriptError) -> Result<String, String> {
        match err {
            ScriptError::Run(RunError::Exception(ex)) => {
                let Value::Object(o) = ex.value() else {
                    return Err(format!("{name}: error is not an object ({})", err.text()));
                };
                let c = o
                    .get_str(rt, &"constructor".into(), None)
                    .map_err(|_| format!("{name}: reading the error constructor threw"))?;
                let Some(c) = c else {
                    return Err(format!("{name}: error does not have a constructor"));
                };
                let Value::Object(c) = c else {
                    return Err(format!("{name}: error constructor is not an object"));
                };
                let n = c
                    .get_str(rt, &"name".into(), None)
                    .map_err(|_| format!("{name}: reading the constructor name threw"))?;
                let Some(n) = n else {
                    return Err(format!("{name}: error constructor does not have a name"));
                };
                n.go_string(rt)
                    .map_err(|_| format!("{name}: converting the constructor name threw"))
            }
            ScriptError::Syntax(_) => Ok("SyntaxError".to_string()),
            _ => Err(format!("{name}: error is not a JS error: {}", err.text())),
        }
    }

    fn run_tc39_test(&self, name: &str, src: &[u8], meta: &Meta, watch: &Watch) -> Outcome {
        let mut rt = Runtime::new();
        if self.gc_stress {
            rt.set_gc_stress(true);
        }
        watch.arm(rt.interrupt_handle());
        let o262 = rt.new_object();
        let ignorable = Value::Symbol(rt.new_symbol(Some(STRING_EMPTY.clone())));
        let create_realm = NativeFn::closure_with(vec![ignorable.clone()], |_rt, d, _call| {
            Err(Throw::Value(d[0].clone()))
        });
        let natives = [
            ("detachArrayBuffer", NativeFn::Fn(detach_array_buffer)),
            ("createRealm", create_realm),
            ("evalScript", NativeFn::Fn(eval_script)),
        ];
        for (prop, f) in natives {
            let f = rt.new_native_func(f, prop, 0);
            if o262
                .set_own_str(&mut rt, &prop.into(), Value::Object(f), true)
                .is_err()
            {
                return Outcome::Fail(format!("{name}: cannot set $262.{prop}"));
            }
        }
        if rt.set("$262", Value::Object(o262)).is_err()
            || rt.set("IgnorableTestError", ignorable.clone()).is_err()
        {
            return Outcome::Fail(format!("{name}: cannot set the harness globals"));
        }
        let _ = rt.run_program(&self.sab_stub);
        let out: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let is_async = meta.has_flag("async");
        let print = if is_async {
            if let Err(e) = self.run_file("harness/doneprintHandle.js", &mut rt) {
                return Outcome::Fail(e.text());
            }
            let out = out.clone();
            NativeFn::closure_with(Vec::new(), move |rt, _d, call| {
                let msg = match call.arguments.first() {
                    Some(v) => v.go_string(rt)?,
                    None => String::new(),
                };
                out.borrow_mut().push(msg);
                Ok(Value::Undefined)
            })
        } else {
            NativeFn::Fn(log)
        };
        let print = rt.new_native_func(print, "print", 0);
        if rt.set("print", Value::Object(print)).is_err() {
            return Outcome::Fail(format!("{name}: cannot set print"));
        }

        match self.run_tc39_script(name, src, &meta.includes, &mut rt) {
            Err((err, early)) => {
                if meta.negative_type.is_empty() {
                    if let ScriptError::Run(RunError::Exception(ex)) = &err
                        && ex.value().same_as(&ignorable)
                    {
                        return Outcome::Skip("Test threw IgnorableTestError".to_string());
                    }
                    return Outcome::Fail(format!("{name}: {}", err.text()));
                }
                let phase = meta.negative_phase.as_str();
                if (phase == "early" || phase == "parse") && !early || phase == "runtime" && early {
                    return Outcome::Fail(format!(
                        "{name}: error {} happened at the wrong phase (expected {phase})",
                        err.text()
                    ));
                }
                let err_type = match Ctx::error_type(&mut rt, name, &err) {
                    Ok(t) => t,
                    Err(msg) => return Outcome::Fail(msg),
                };
                if err_type != meta.negative_type {
                    return Outcome::Fail(format!(
                        "{name}: unexpected error type ({err_type}), expected ({})",
                        meta.negative_type
                    ));
                }
            }
            Ok(()) => {
                if !meta.negative_type.is_empty() {
                    return Outcome::Fail(format!("{name}: Expected error: <nil>"));
                }
            }
        }

        if rt.vm.sp != 0 {
            return Outcome::Fail(format!("sp: {}", rt.vm.sp));
        }
        if !rt.vm.iter_stack.is_empty() {
            return Outcome::Fail(format!(
                "iter stack is not empty: {}",
                rt.vm.iter_stack.len()
            ));
        }
        if is_async {
            let mut complete = false;
            for line in out.borrow().iter() {
                if line.starts_with("Test262:AsyncTestFailure:") {
                    return Outcome::Fail(line.clone());
                } else if line == "Test262:AsyncTestComplete" {
                    complete = true;
                }
            }
            if !complete {
                return Outcome::Fail("Test262:AsyncTestComplete was not printed".to_string());
            }
        }
        Outcome::Pass
    }

    fn run_tc39_file(&self, name: &str, watch: &Watch) -> Outcome {
        if SKIP_LIST.contains(&name) {
            return Outcome::Skip("Excluded".to_string());
        }
        if SKIP_PREFIXES.iter().any(|p| name.starts_with(p)) {
            return Outcome::Skip("Excluded".to_string());
        }
        let src = match std::fs::read(self.base.join(name)) {
            Ok(src) => src,
            Err(e) => return Outcome::Fail(format!("Could not parse {name}: {e}")),
        };
        let meta = match parse_tc39_file(&src) {
            Ok(meta) => meta,
            Err(e) => return Outcome::Fail(format!("Could not parse {name}: {e}")),
        };
        if meta.has_flag("module") {
            return Outcome::Skip("module".to_string());
        }
        if meta.es5id.is_empty()
            && meta
                .features
                .iter()
                .any(|f| FEATURES_BLACK_LIST.contains(&f.as_str()))
        {
            return Outcome::Skip("Blacklisted feature".to_string());
        }
        let has_raw = meta.has_flag("raw");
        if has_raw || !meta.has_flag("onlyStrict") {
            let o = self.run_tc39_test(name, &src, &meta, watch);
            if o != Outcome::Pass {
                return o;
            }
        }
        if !has_raw && !meta.has_flag("noStrict") {
            let mut strict = b"'use strict';\n".to_vec();
            strict.extend_from_slice(&src);
            let o = self.run_tc39_test(name, &strict, &meta, watch);
            if o != Outcome::Pass {
                return o;
            }
        }
        Outcome::Pass
    }

    fn collect(&self, dir: &str, out: &mut Vec<String>) {
        let mut entries: Vec<(String, bool)> = std::fs::read_dir(self.base.join(dir))
            .unwrap_or_else(|e| panic!("cannot read {dir}: {e}"))
            .map(|e| {
                let e = e.unwrap_or_else(|e| panic!("cannot read {dir}: {e}"));
                let is_dir = e.path().is_dir();
                (e.file_name().to_string_lossy().into_owned(), is_dir)
            })
            .collect();
        entries.sort();
        for (file, is_dir) in entries {
            if file.starts_with('.') {
                continue;
            }
            let name = format!("{dir}/{file}");
            if is_dir {
                self.collect(&name, out);
            } else if file.ends_with(".js") && !file.ends_with("_FIXTURE.js") {
                out.push(name);
            }
        }
    }
}

fn panic_text(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = p.downcast_ref::<String>() {
        return s.clone();
    }
    "unknown panic".to_string()
}

fn run_all(ctx: Arc<Ctx>, names: Arc<Vec<String>>) -> Vec<Outcome> {
    let limit = Duration::from_secs(
        std::env::var("TALOS_TEST262_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300),
    );
    let workers = std::env::var("TALOS_TEST262_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let results: Arc<Mutex<Vec<Option<Outcome>>>> = Arc::new(Mutex::new(vec![None; names.len()]));
    let watches: Vec<Arc<Watch>> = (0..workers).map(|_| Arc::new(Watch::default())).collect();
    let mut handles = Vec::new();
    for watch in &watches {
        let ctx = ctx.clone();
        let names = names.clone();
        let next = next.clone();
        let done = done.clone();
        let results = results.clone();
        let watch = watch.clone();
        let h = std::thread::Builder::new()
            .name("talos-test262".to_string())
            .stack_size(256 << 20)
            .spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= names.len() {
                        break;
                    }
                    let name = &names[i];
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        ctx.run_tc39_file(name, &watch)
                    }));
                    watch.disarm();
                    let outcome = match res {
                        Ok(o) => o,
                        Err(p) => {
                            Outcome::Fail(format!("panic while running {name}: {}", panic_text(p)))
                        }
                    };
                    results.lock().expect("results")[i] = Some(outcome);
                }
                done.fetch_add(1, Ordering::SeqCst);
            })
            .expect("spawn");
        handles.push(h);
    }
    while done.load(Ordering::SeqCst) < workers {
        std::thread::sleep(Duration::from_millis(200));
        for w in &watches {
            w.check(limit);
        }
    }
    for h in handles {
        h.join().expect("worker");
    }
    let results = std::mem::take(&mut *results.lock().expect("results"));
    results
        .into_iter()
        .map(|o| o.expect("every test has an outcome"))
        .collect()
}

fn checkout_commit(dir: &Path) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap_or_else(|e| panic!("cannot run git in {}: {e}", dir.display()));
    assert!(
        out.status.success(),
        "{} is not a git checkout of test262",
        dir.display()
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
#[ignore = "needs TALOS_TEST262_DIR, a test262 checkout at the pinned commit"]
fn tc39() {
    let dir = std::env::var("TALOS_TEST262_DIR")
        .unwrap_or_else(|_| panic!("TALOS_TEST262_DIR is not set"));
    let dir = PathBuf::from(dir);
    let head = checkout_commit(&dir);
    assert_eq!(
        head, PINNED_COMMIT,
        "test262 checkout is at {head}, want {PINNED_COMMIT}"
    );
    let summary = gocompat::json::unmarshal_value(&common::read_golden("hooks/test262.json"))
        .expect("test262.json");
    assert_eq!(common::s(&summary, "test262_commit"), PINNED_COMMIT);
    let filter = std::env::var("TALOS_TEST262_FILTER").ok();
    let selected = |name: &str| filter.as_deref().is_none_or(|f| name.contains(f));
    let golden = common::read_golden("hooks/test262-pass.tsv");
    let golden = String::from_utf8(golden).expect("test262-pass.tsv");
    let all_golden: BTreeSet<&str> = golden.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(all_golden.len() as i64, common::i(&summary, "passed"));
    let want: BTreeSet<&str> = all_golden.iter().copied().filter(|n| selected(n)).collect();

    let ctx = Arc::new(Ctx::new(dir));
    let mut names = Vec::new();
    for suite in SUITES {
        ctx.collect(suite, &mut names);
    }
    names.retain(|n| selected(n));
    let names = Arc::new(names);
    let outcomes = run_all(ctx, names.clone());

    let mut passed: BTreeSet<&str> = BTreeSet::new();
    let mut skipped = 0i64;
    let mut failed = 0i64;
    let mut report = String::new();
    for (name, outcome) in names.iter().zip(&outcomes) {
        let (status, detail) = match outcome {
            Outcome::Pass => {
                passed.insert(name);
                ("PASS", "")
            }
            Outcome::Skip(why) => {
                skipped += 1;
                ("SKIP", why.as_str())
            }
            Outcome::Fail(why) => {
                failed += 1;
                ("FAIL", why.as_str())
            }
        };
        report.push_str(&format!(
            "{name}\t{status}\t{}\n",
            detail.replace(['\n', '\t'], " ")
        ));
    }
    if let Ok(path) = std::env::var("TALOS_TEST262_REPORT") {
        std::fs::write(&path, &report).unwrap_or_else(|e| panic!("cannot write {path}: {e}"));
    }

    let extra: Vec<&str> = passed.difference(&want).copied().collect();
    let missing: Vec<&str> = want.difference(&passed).copied().collect();
    if !extra.is_empty() || !missing.is_empty() {
        let mut msg = format!(
            "test262 pass set differs from the baseline: {} passed, {} skipped, {} failed, {} expected, {} extra, {} missing\n",
            passed.len(),
            skipped,
            failed,
            want.len(),
            extra.len(),
            missing.len()
        );
        for name in &extra {
            msg.push_str(&format!("extra\t{name}\n"));
        }
        for name in &missing {
            msg.push_str(&format!("missing\t{name}\n"));
        }
        panic!("{msg}");
    }
    if filter.is_none() {
        assert_eq!(failed, common::i(&summary, "failed"));
        assert_eq!(skipped, common::i(&summary, "skipped"));
    }
}

fn meta_of(src: &str) -> Meta {
    parse_tc39_file(src.as_bytes()).expect("meta")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn front_matter_flow_sequences() {
    let m = meta_of(
        "// c\n/*---\nesid: sec-x\ndescription: a: b\nincludes: [propertyHelper.js, compareArray.js]\nflags: [onlyStrict]\nfeatures: [Symbol.iterator, \"class\"]\n---*/\nvar x;\n",
    );
    assert_eq!(
        m.includes,
        strings(&["propertyHelper.js", "compareArray.js"])
    );
    assert_eq!(m.flags, strings(&["onlyStrict"]));
    assert_eq!(m.features, strings(&["Symbol.iterator", "class"]));
    assert!(m.es5id.is_empty());
    assert!(m.has_flag("onlyStrict"));
    assert!(!m.has_flag("raw"));
}

#[test]
fn front_matter_block_sequences_and_negative() {
    let m = meta_of(
        "/*---\ninfo: |\n    flags: [raw]\n    es5id: nope\nfeatures:\n  - WeakMap\n  - Symbol\nnegative:\n  phase: parse\n  type: SyntaxError\nflags: [\n  noStrict,\n  async\n]\n---*/\n",
    );
    assert_eq!(m.features, strings(&["WeakMap", "Symbol"]));
    assert_eq!(m.negative_phase, "parse");
    assert_eq!(m.negative_type, "SyntaxError");
    assert_eq!(m.flags, strings(&["noStrict", "async"]));
    assert!(m.es5id.is_empty());
}

#[test]
fn front_matter_indented_document_and_es5id() {
    let m = meta_of(
        "/*---\r\n  description: >\r\n      text\r\n  es5id: 12.1.1\r\n  negative:\r\n    phase: runtime\r\n    type: ReferenceError\r\n  flags: [onlyStrict]\r\n---*/\r\n",
    );
    assert_eq!(m.es5id, "12.1.1");
    assert_eq!(m.negative_phase, "runtime");
    assert_eq!(m.negative_type, "ReferenceError");
    assert_eq!(m.flags, strings(&["onlyStrict"]));
}

#[test]
fn front_matter_errors() {
    assert_eq!(
        parse_tc39_file(b"var x;"),
        Err("Invalid file format".to_string())
    );
    assert_eq!(
        parse_tc39_file(b"---*/ /*---"),
        Err("Invalid file format".to_string())
    );
    assert_eq!(
        parse_tc39_file(b"/*---\nnegative:\n  type: SyntaxError\n---*/"),
        Err("negative type is set, but phase isn't".to_string())
    );
    assert!(parse_tc39_file(b"/*---\nflags: onlyStrict\n---*/").is_err());
}

#[test]
fn exclusions_match_the_baseline_lists() {
    assert_eq!(SKIP_LIST.len(), 123);
    assert_eq!(SKIP_PREFIXES.len(), 28);
    assert_eq!(FEATURES_BLACK_LIST.len(), 40);
}
