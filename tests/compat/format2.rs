use elysiandb::durable;

use super::corpus::{Engine, shutdown_engine};
use super::fixture::FixtureRequest;
use super::golden::{differs, expected_body, golden_full, intentional_change, testdata, truncate};
use super::legacy::{copy_folder, legacy_read_requests, start_server_on_folder};

const FORMAT2_FIXTURES_DIR: &str = "pr-format2";

#[test]
fn folders_of_the_format_two_engine_are_read() {
    let _engine = Engine;
    let want = golden_full();

    let mut errors = Vec::new();
    for variant in [
        "format2",
        "format2-crash",
        "deb5d46",
        "deb5d46-crash",
        "e68e64f",
        "e68e64f-crash",
        "c104f4a",
        "c104f4a-crash",
    ] {
        let folder = copy_folder(&testdata(FORMAT2_FIXTURES_DIR).join(variant));
        let folder = folder.path();

        for round in 0..2 {
            let mut s = start_server_on_folder(folder, true);
            for (i, r) in legacy_read_requests().iter().enumerate() {
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

            if variant.ends_with("-crash") {
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
                std::fs::read_to_string(folder.join(durable::MANIFEST_FILE)).unwrap_or_default();
            if !manifest.contains(r#""format": 2"#) {
                panic!("{variant} round {round}: the manifest keeps format 2, got {manifest}");
            }

            if let Err(e) = std::fs::metadata(folder.join(durable::PREVIOUS_MANIFEST)) {
                panic!(
                    "{variant} round {round}: the previous checkpoint is kept to fall back on: {e}"
                );
            }
        }
    }

    if !errors.is_empty() {
        panic!("{}", errors.join("\n"));
    }
}
