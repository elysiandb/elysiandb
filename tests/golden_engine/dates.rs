use std::collections::BTreeSet;

use crate::common;
use elysiandb::docpath;
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::time::Duration;

const REL: &str = "engine/dates.jsonl";

fn int(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer {key} in {row:?}"))
}

fn flag(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing boolean {key} in {row:?}"))
}

fn date_inputs() -> Vec<Vec<u8>> {
    let mut input: Vec<Vec<u8>> = [
        "2026-11-23",
        "2026-11-23T18:39:01Z",
        "2026-11-23T18:39:01+01:00",
        "2026-11-23T18:39:01-05:30",
        "2026-11-23T18:39:01.123Z",
        "2026-11-23 18:39:01Z",
        "2026-11-23t18:39:01z",
        "2026-11-23T18:39:01",
        "2026-1-23",
        "2026-11-3",
        "26-11-23",
        "2026/11/23",
        "2026-11-23x",
        "2026-11-23T",
        "",
        "short",
        "abcd-11-23",
        "2026-13-01",
        "2026-00-01",
        "2026-11-31",
        "2026-11-00",
        "0000-01-01",
        "9999-12-31",
        "9999-12-31T23:59:59.999999999Z",
        "0000-01-01T00:00:00Z",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect();
    for y in ["0000", "1900", "2000", "2023", "2024"] {
        for tail in ["-02-29", "-02-29T12:00:00Z", "-02-28", "-03-01"] {
            input.push(format!("{y}{tail}").into_bytes());
        }
    }
    for sep in ["T", "t", " "] {
        for tm in [
            "1:2:3", "01:02:03", "1:02:03", "01:2:03", "01:02:3", "23:59:59", "24:00:00",
            "23:60:00", "23:59:60",
        ] {
            input.push(format!("2026-06-15{sep}{tm}Z").into_bytes());
        }
    }
    for frac in 0..=12 {
        let d = "5".repeat(frac);
        input.push(format!("2026-06-15T12:00:00.{d}Z").into_bytes());
        input.push(format!("2026-06-15T12:00:00,{d}Z").into_bytes());
    }
    for sign in ["+", "-"] {
        for hh in ["00", "01", "12", "23", "24", "25"] {
            for mm in ["00", "30", "59", "60", "61"] {
                input.push(format!("2026-06-15T12:00:00{sign}{hh}:{mm}").into_bytes());
            }
        }
    }
    for tail in ["Z ", "Zx", "+01:00x", " ", "\x00", "+0100", "+01"] {
        input.push(format!("2026-06-15T12:00:00{tail}").into_bytes());
    }
    input
}

#[test]
fn golden_dates() {
    let rows = common::read_jsonl(REL);
    assert_eq!(rows.len(), common::lines(&common::read_golden(REL)).len());

    let inputs = date_inputs();
    assert_eq!(rows.len(), inputs.len(), "input list");

    let day = Duration::HOUR * 24;
    let mut checked = 0;
    for (row, input) in rows.iter().zip(&inputs) {
        let s = common::unhex(common::str_field(row, "in"));
        assert_eq!(&s, input, "input order");
        let shown = String::from_utf8_lossy(&s).into_owned();

        let mut fields: BTreeSet<&str> = ["in", "ok", "date_only", "sort_ok"].into();

        let parsed = docpath::parse_date(&s);
        assert_eq!(flag(row, "ok"), parsed.is_some(), "ParseDate({shown:?}) ok");
        match &parsed {
            Some((t, date_only)) => {
                fields.extend(["unix", "nanos"]);
                assert_eq!(
                    flag(row, "date_only"),
                    *date_only,
                    "ParseDate({shown:?}) date only"
                );
                assert_eq!(int(row, "unix"), t.unix(), "ParseDate({shown:?}) unix");
                assert_eq!(
                    int(row, "nanos"),
                    t.nanosecond(),
                    "ParseDate({shown:?}) nanos"
                );
            }
            None => assert!(!flag(row, "date_only"), "ParseDate({shown:?}) date only"),
        }

        let sorted = docpath::parse_sort_date(&s);
        assert_eq!(
            flag(row, "sort_ok"),
            sorted.is_some(),
            "ParseSortDate({shown:?}) ok"
        );
        if let Some(t) = &sorted {
            fields.extend(["sort_unix", "sort_nanos", "sort_day_unix"]);
            assert_eq!(
                int(row, "sort_unix"),
                t.unix(),
                "ParseSortDate({shown:?}) unix"
            );
            assert_eq!(
                int(row, "sort_nanos"),
                t.nanosecond(),
                "ParseSortDate({shown:?}) nanos"
            );
            assert_eq!(
                int(row, "sort_day_unix"),
                t.truncate(day).unix(),
                "ParseSortDate({shown:?}) day"
            );
        }

        let got: BTreeSet<&str> = row
            .as_object()
            .expect("object row")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(got, fields, "fields of {shown:?}");
        checked += 1;
    }

    assert_eq!(checked, rows.len());
    assert_eq!(checked, 165);
}
