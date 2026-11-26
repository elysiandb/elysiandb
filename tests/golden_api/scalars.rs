use elysiandb::api;
use elysiandb::datastore::element_string;
use elysiandb::docpath;
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::strconv::{self, ErrorKind};
use elysiandb::gocompat::strings;
use elysiandb::gocompat::time::Duration;

use super::common::{hex, read_jsonl, str_field, unhex};
use super::{boolean, go_decode, integer};

fn flag(row: &Value, key: &str) -> bool {
    boolean(row.get(key).unwrap_or_else(|| panic!("missing {key}")))
}

fn bits_of(row: &Value) -> u64 {
    u64::from_str_radix(str_field(row, "bits"), 16).expect("bits")
}

#[test]
fn scalar_vectors() {
    let rows = read_jsonl("api/scalar_vectors.jsonl");
    let mut checked = 0;
    for row in &rows {
        match str_field(row, "fn") {
            "ParseDate" => {
                let input = str_field(row, "in").as_bytes();
                let parsed = docpath::parse_date(input);
                if parsed.is_some() != flag(row, "ok") {
                    panic!("{row:?}: parsed {}", parsed.is_some());
                }
                if let Some((t, date_only)) = parsed {
                    let day = t.truncate(Duration(24 * Duration::HOUR.0));
                    if date_only != flag(row, "date_only")
                        || t.unix() != integer(row, "unix")
                        || t.nanosecond() != integer(row, "nanos")
                        || day.unix() != integer(row, "day_unix")
                    {
                        panic!("{row:?}: {} {} {date_only}", t.unix(), t.nanosecond());
                    }
                } else if flag(row, "date_only") {
                    panic!("{row:?}: a failed parse is not date-only");
                }
                let sorted = docpath::parse_sort_date(input);
                if sorted.is_some() != flag(row, "sort_ok") {
                    panic!("{row:?}: sort parse {}", sorted.is_some());
                }
                if let Some(t) = sorted
                    && t.unix() != integer(row, "sort_unix")
                {
                    panic!("{row:?}: sort unix {}", t.unix());
                }
            }
            "MatchGlob" => {
                let got = docpath::match_glob(
                    str_field(row, "pattern").as_bytes(),
                    str_field(row, "subject").as_bytes(),
                );
                if got != flag(row, "out") {
                    panic!("{row:?}: got {got}");
                }
            }
            "ParseFloat" => {
                let input = str_field(row, "in").as_bytes();
                let (value, err) = match strconv::parse_float(input, 64) {
                    Ok(f) => (f, None),
                    Err(e) => (
                        e.value,
                        Some(match e.kind {
                            ErrorKind::Range => "range",
                            _ => "syntax",
                        }),
                    ),
                };
                let want_err = row.get("error").and_then(Value::as_str);
                if value.to_bits() != bits_of(row) || err != want_err {
                    panic!("{row:?}: {:016x} {err:?}", value.to_bits());
                }
            }
            "FormatFloat" => {
                let f = f64::from_bits(bits_of(row));
                let fmt_f = strconv::format_float(f, b'f', -1, 64);
                let fmt_g = strconv::format_float(f, b'g', -1, 64);
                if fmt_f != str_field(row, "f") || fmt_g != str_field(row, "g") {
                    panic!("{row:?}: {fmt_f} {fmt_g}");
                }
            }
            "Sprintf_v" => {
                let v = go_decode(str_field(row, "in").as_bytes());
                let got = element_string(&v);
                if got != str_field(row, "out").as_bytes() {
                    panic!("{row:?}: {}", String::from_utf8_lossy(&got));
                }
            }
            "ToLower+TrimSpace" => {
                let input = unhex(str_field(row, "in"));
                let lower = hex(&strings::to_lower(&input));
                let trim = hex(strings::trim_space(&input));
                if lower != str_field(row, "lower") || trim != str_field(row, "trim") {
                    panic!("{row:?}: {lower} {trim}");
                }
            }
            "SearchPattern" => {
                let input = unhex(str_field(row, "in"));
                let pattern = hex(&api::search_pattern(&input));
                let lowered = hex(&api::lowered_search_pattern(&input));
                if pattern != str_field(row, "pattern") || lowered != str_field(row, "lowered") {
                    panic!("{row:?}: {pattern} {lowered}");
                }
            }
            other => panic!("unknown function {other}"),
        }
        checked += 1;
    }

    assert_eq!(checked, 85);
    assert_eq!(checked, rows.len());
}
