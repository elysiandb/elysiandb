mod common;

use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::time::{
    self, DATE_ONLY, Duration, Location, Month, ParseErrorKind, RFC3339, RFC3339_NANO, Time,
    ZoneError,
};
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const ZONE_NAMES: [&str; 14] = [
    "UTC",
    "Europe/Paris",
    "America/New_York",
    "America/Sao_Paulo",
    "Asia/Kolkata",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
    "Africa/Casablanca",
    "Asia/Tokyo",
    "Pacific/Kiritimati",
    "America/St_Johns",
    "Europe/Dublin",
    "Antarctica/Troll",
    "Asia/Kathmandu",
];

fn zone_file(name: &str) -> String {
    format!("gocompat/time/zoneinfo/{}.tzif", name.replace('/', "_"))
}

fn zone_data(name: &str) -> Vec<u8> {
    common::read_golden(&zone_file(name))
}

fn zone(name: &str) -> Location {
    time::load_location_from_tzdata(name, &zone_data(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn escaped(s: &str) -> String {
    s.replace('~', "\\")
}

fn line_count(rel: &str) -> usize {
    common::lines(&common::read_golden(rel)).len()
}

fn int(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

fn boolean(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing boolean field {key} in {row:?}"))
}

fn ints(row: &Value, key: &str) -> Vec<i64> {
    row.get(key)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing array field {key} in {row:?}"))
        .iter()
        .map(|v| v.as_i64().expect("integer element"))
        .collect()
}

fn keys(row: &Value) -> BTreeSet<String> {
    row.as_object()
        .unwrap_or_else(|| panic!("not an object: {row:?}"))
        .keys()
        .cloned()
        .collect()
}

fn key_set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn check_instant(row: &Value, t: &Time, extra: &[&str]) {
    let ctx = format!("{row:?}");
    assert_eq!(t.unix(), int(row, "unix"), "unix {ctx}");
    assert_eq!(t.nanosecond(), int(row, "nanos"), "nanos {ctx}");
    let (abbr, offset) = t.zone();
    assert_eq!(abbr, common::str_field(row, "abbr"), "abbr {ctx}");
    assert_eq!(i64::from(offset), int(row, "offset"), "offset {ctx}");
    assert_eq!(t.is_dst(), boolean(row, "dst"), "dst {ctx}");
    let (year, month, day) = t.date();
    let (hour, min, sec) = t.clock();
    assert_eq!(
        vec![year, month.number(), day, hour, min, sec],
        ints(row, "date"),
        "date {ctx}"
    );
    assert_eq!(t.year(), year, "year {ctx}");
    assert_eq!(t.month(), month, "month {ctx}");
    assert_eq!(t.day(), day, "day {ctx}");
    assert_eq!(
        (t.hour(), t.minute(), t.second()),
        (hour, min, sec),
        "clock {ctx}"
    );
    assert_eq!(t.weekday().number(), int(row, "weekday"), "weekday {ctx}");
    assert_eq!(t.year_day(), int(row, "yday"), "yday {ctx}");
    assert_eq!(
        t.format(RFC3339_NANO),
        common::str_field(row, "rfc3339nano"),
        "rfc3339nano {ctx}"
    );
    let (start, end) = t.zone_bounds();
    let mut expected = key_set(&[
        "abbr",
        "date",
        "dst",
        "nanos",
        "offset",
        "rfc3339nano",
        "unix",
        "weekday",
        "yday",
    ]);
    for k in extra {
        expected.insert(k.to_string());
    }
    match start {
        Some(start) => {
            assert_eq!(start.unix(), int(row, "zone_start"), "zone_start {ctx}");
            expected.insert("zone_start".to_string());
        }
        None => assert!(row.get("zone_start").is_none(), "zone_start {ctx}"),
    }
    match end {
        Some(end) => {
            assert_eq!(end.unix(), int(row, "zone_end"), "zone_end {ctx}");
            expected.insert("zone_end".to_string());
        }
        None => assert!(row.get("zone_end").is_none(), "zone_end {ctx}"),
    }
    assert_eq!(keys(row), expected, "keys {ctx}");
}

#[test]
fn civil_golden_rows_match_go() {
    let rel = "gocompat/time/civil.jsonl";
    let rows = common::read_jsonl(rel);
    let mut with_input = 0;
    for row in &rows {
        if row.get("in").is_some() {
            let c = ints(row, "in");
            assert_eq!(c.len(), 7);
            let t = Time::from_date(
                c[0],
                Month(c[1]),
                c[2],
                c[3],
                c[4],
                c[5],
                c[6],
                &Location::utc(),
            );
            assert!(t.location().is_utc());
            check_instant(row, &t, &["in"]);
            with_input += 1;
        } else {
            let t = Time::from_unix(int(row, "unix"), 0).utc();
            check_instant(row, &t, &[]);
        }
    }
    assert_eq!(with_input, 14);
    assert_eq!(rows.len(), line_count(rel));
    assert_eq!(rows.len(), 175);
}

#[test]
fn zone_golden_rows_match_go() {
    let rel = "gocompat/time/zones.jsonl";
    let rows = common::read_jsonl(rel);
    let locs: HashMap<&str, Location> = ZONE_NAMES.iter().map(|&n| (n, zone(n))).collect();
    let mut per_zone: HashMap<String, usize> = HashMap::new();
    for row in &rows {
        let name = common::str_field(row, "zone");
        let loc = locs
            .get(name)
            .unwrap_or_else(|| panic!("unexpected zone {name}"));
        assert_eq!(loc.name(), name);
        let t = Time::from_unix(int(row, "unix"), 0).in_location(loc);
        check_instant(row, &t, &["zone"]);
        *per_zone.entry(name.to_string()).or_default() += 1;
    }
    assert_eq!(per_zone.len(), ZONE_NAMES.len());
    assert_eq!(rows.len(), line_count(rel));
    assert_eq!(rows.len(), 5150);
}

const FORMAT_LAYOUTS: [&str; 7] = [
    RFC3339,
    RFC3339_NANO,
    "2006-01-02",
    "20060102-150405",
    "20060102150405.000000000",
    "20060102T150405",
    "2006-01-02T15:04:05.000Z07:00",
];

fn format_zones() -> Vec<Location> {
    let zones = common::read_json("gocompat/time/format_zones.json");
    let zones = zones.as_array().expect("format zones array");
    assert_eq!(zones.len(), 7);
    zones
        .iter()
        .enumerate()
        .map(|(i, z)| {
            assert_eq!(int(z, "index"), i as i64);
            match common::str_field(z, "kind") {
                "utc" => {
                    assert_eq!(keys(z), key_set(&["index", "kind"]));
                    Location::utc()
                }
                "fixed" => {
                    assert_eq!(keys(z), key_set(&["index", "kind", "name", "offset"]));
                    Location::fixed(common::str_field(z, "name"), int(z, "offset") as i32)
                }
                "tzif" => {
                    assert_eq!(keys(z), key_set(&["index", "kind", "name"]));
                    zone(common::str_field(z, "name"))
                }
                kind => panic!("unknown zone kind {kind}"),
            }
        })
        .collect()
}

#[test]
fn format_golden_rows_match_go() {
    let rel = "gocompat/time/format.jsonl";
    let rows = common::read_jsonl(rel);
    let locs = format_zones();
    let years: [i64; 11] = [-1, 0, 1, 999, 1969, 1970, 2000, 2026, 9999, 10000, 12345];
    let nanos: [i64; 7] = [0, 1, 10, 100000, 120000000, 123456789, 999999999];
    let layouts: BTreeSet<String> = FORMAT_LAYOUTS.iter().map(|s| s.to_string()).collect();
    for (i, row) in rows.iter().enumerate() {
        let zi = i / (years.len() * nanos.len());
        let y = years[(i / nanos.len()) % years.len()];
        let ns = nanos[i % nanos.len()];
        assert_eq!(int(row, "zone_index"), zi as i64);
        assert_eq!(
            keys(row),
            key_set(&["formats", "nanos", "unix", "zone_index"])
        );
        let loc = &locs[zi];
        let built = Time::from_date(
            y,
            Month(1 + (y & 7)),
            1 + (y % 27 + 27) % 27,
            13,
            4,
            5,
            ns,
            loc,
        );
        assert_eq!(built.unix(), int(row, "unix"), "unix {row:?}");
        assert_eq!(built.nanosecond(), int(row, "nanos"), "nanos {row:?}");
        let t = Time::from_unix(int(row, "unix"), int(row, "nanos")).in_location(loc);
        let formats = row.get("formats").expect("formats");
        assert_eq!(keys(formats), layouts, "layouts {row:?}");
        for layout in FORMAT_LAYOUTS {
            let want = common::str_field(formats, layout);
            assert_eq!(t.format(layout), want, "{layout} {row:?}");
            assert_eq!(built.format(layout), want, "{layout} {row:?}");
            let mut b = b"x".to_vec();
            t.append_format(&mut b, layout);
            assert_eq!(&b[1..], want.as_bytes());
        }
    }
    assert_eq!(rows.len(), line_count(rel));
    assert_eq!(rows.len(), 539);
}

#[test]
fn parse_golden_rows_match_go() {
    let rel = "gocompat/time/parse.jsonl";
    let rows = common::read_jsonl(rel);
    let locs = [("UTC", zone("UTC")), ("Europe/Paris", zone("Europe/Paris"))];
    let layouts = [RFC3339, RFC3339_NANO, DATE_ONLY];
    let mut row_keys = key_set(&layouts);
    row_keys.insert("in".to_string());
    let mut accepted = 0;
    let mut rejected = 0;
    for row in &rows {
        assert_eq!(keys(row), row_keys, "keys {row:?}");
        let input = common::unhex(common::str_field(row, "in"));
        for layout in layouts {
            let res = row.get(layout).expect("layout result");
            assert_eq!(keys(res), key_set(&["Europe/Paris", "UTC"]));
            for (zone_name, loc) in &locs {
                let want = res.get(zone_name).expect("zone result");
                let ctx = format!("{layout} {zone_name} {row:?}");
                let got = time::parse_in_location(layout, &input, loc);
                if boolean(want, "ok") {
                    assert_eq!(
                        keys(want),
                        key_set(&["abbr", "nanos", "offset", "ok", "unix", "utc"]),
                        "{ctx}"
                    );
                    let t = got.unwrap_or_else(|e| panic!("{e} {ctx}"));
                    assert_eq!(t.unix(), int(want, "unix"), "unix {ctx}");
                    assert_eq!(t.nanosecond(), int(want, "nanos"), "nanos {ctx}");
                    let (abbr, offset) = t.zone();
                    assert_eq!(abbr, common::str_field(want, "abbr"), "abbr {ctx}");
                    assert_eq!(i64::from(offset), int(want, "offset"), "offset {ctx}");
                    assert_eq!(t.location().is_utc(), boolean(want, "utc"), "utc {ctx}");
                    accepted += 1;
                } else {
                    assert_eq!(keys(want), key_set(&["ok"]), "{ctx}");
                    let err = match got {
                        Ok(t) => panic!("accepted as {} {ctx}", t.format(RFC3339_NANO)),
                        Err(err) => err,
                    };
                    assert_eq!(err.value, input, "{ctx}");
                    assert_eq!(err.layout, layout, "{ctx}");
                    let suffix = if layout == DATE_ONLY {
                        " is not a valid time"
                    } else {
                        " is not a valid RFC 3339 time"
                    };
                    let message = err.to_string();
                    assert!(message.starts_with('"'), "{message} {ctx}");
                    assert!(message.ends_with(suffix), "{message} {ctx}");
                    rejected += 1;
                }
            }
        }
    }
    assert!(accepted > 0 && rejected > 0);
    assert_eq!(accepted + rejected, rows.len() * 6);
    assert_eq!(rows.len(), line_count(rel));
    assert_eq!(rows.len(), 145);
}

#[test]
fn duration_golden_rows_match_go() {
    let rel = "gocompat/time/duration.jsonl";
    let rows = common::read_jsonl(rel);
    let multiples: [i64; 6] = [1_000_000, 10_000_000, 1_000_000_000, 60_000_000_000, 0, -1];
    let mut expected_keys = key_set(&["ms", "ns", "seconds", "string"]);
    for m in multiples {
        expected_keys.insert(format!("round_{m}"));
        expected_keys.insert(format!("trunc_{m}"));
    }
    for row in &rows {
        assert_eq!(keys(row), expected_keys, "keys {row:?}");
        let ns: i64 = common::str_field(row, "ns").parse().expect("ns");
        let d = Duration(ns);
        let string = common::str_field(row, "string");
        assert_eq!(d.to_string(), string, "string {row:?}");
        let mut b = Vec::new();
        d.append_to(&mut b);
        assert_eq!(b, string.as_bytes());
        assert_eq!(
            format!("{:016x}", d.seconds().to_bits()),
            common::str_field(row, "seconds"),
            "seconds {row:?}"
        );
        assert_eq!(
            d.milliseconds().to_string(),
            common::str_field(row, "ms"),
            "ms {row:?}"
        );
        for m in multiples {
            assert_eq!(
                d.round(Duration(m)).0.to_string(),
                common::str_field(row, &format!("round_{m}")),
                "round {m} {row:?}"
            );
            assert_eq!(
                d.truncate(Duration(m)).0.to_string(),
                common::str_field(row, &format!("trunc_{m}")),
                "trunc {m} {row:?}"
            );
        }
        assert_eq!(time::parse_duration(string), Ok(d), "round trip {row:?}");
    }
    assert_eq!(rows.len(), line_count(rel));
    assert_eq!(rows.len(), 24);
}

#[test]
fn parse_duration_matches_go_table() {
    let s = Duration::SECOND.0;
    let ms = Duration::MILLISECOND.0;
    let us = Duration::MICROSECOND.0;
    let min = Duration::MINUTE.0;
    let h = Duration::HOUR.0;
    let cases: Vec<(&str, i64)> = vec![
        ("0", 0),
        ("5s", 5 * s),
        ("30s", 30 * s),
        ("1478s", 1478 * s),
        ("-5s", -5 * s),
        ("+5s", 5 * s),
        ("-0", 0),
        ("+0", 0),
        ("5.0s", 5 * s),
        ("5.6s", 5 * s + 600 * ms),
        ("5.s", 5 * s),
        (".5s", 500 * ms),
        ("1.0s", s),
        ("1.00s", s),
        ("1.004s", s + 4 * ms),
        ("1.0040s", s + 4 * ms),
        ("100.00100s", 100 * s + ms),
        ("10ns", 10),
        ("11us", 11 * us),
        ("12\u{b5}s", 12 * us),
        ("12\u{3bc}s", 12 * us),
        ("13ms", 13 * ms),
        ("14s", 14 * s),
        ("15m", 15 * min),
        ("16h", 16 * h),
        ("3h30m", 3 * h + 30 * min),
        ("10.5s4m", 4 * min + 10 * s + 500 * ms),
        ("-2m3.4s", -(2 * min + 3 * s + 400 * ms)),
        ("1h2m3s4ms5us6ns", h + 2 * min + 3 * s + 4 * ms + 5 * us + 6),
        ("39h9m14.425s", 39 * h + 9 * min + 14 * s + 425 * ms),
        ("52763797000ns", 52763797000),
        ("0.3333333333333333333h", 20 * min),
        ("9007199254740993ns", (1 << 53) + 1),
        ("9223372036854775807ns", i64::MAX),
        ("9223372036854775.807us", i64::MAX),
        ("9223372036s854ms775us807ns", i64::MAX),
        ("-9223372036854775808ns", i64::MIN),
        ("-9223372036854775.808us", i64::MIN),
        ("-9223372036s854ms775us808ns", i64::MIN),
        ("-2562047h47m16.854775808s", i64::MIN),
        ("0.100000000000000000000h", 6 * min),
        ("0.830103483285477580700h", 49 * min + 48 * s + 372539827),
    ];
    for (input, want) in cases {
        assert_eq!(time::parse_duration(input), Ok(Duration(want)), "{input}");
    }
    let errors: [(&[u8], &str); 20] = [
        (b"", r#""""#),
        (b"3", r#""3""#),
        (b"-", r#""-""#),
        (b"s", r#""s""#),
        (b".", r#"".""#),
        (b"-.", r#""-.""#),
        (b".s", r#"".s""#),
        (b"+.s", r#""+.s""#),
        (b"1d", r#""1d""#),
        (b"\x85\x85", "\"~ufffd~ufffd\""),
        (b"\xffff", "\"~ufffdff\""),
        (b"hello \xffff world", "\"hello ~ufffdff world\""),
        ("\u{fffd}".as_bytes(), "\"\u{fffd}\""),
        (b"9223372036854775810ns", r#""9223372036854775810ns""#),
        (b"9223372036854775808ns", r#""9223372036854775808ns""#),
        (b"-9223372036854775809ns", r#""-9223372036854775809ns""#),
        (b"9223372036854776us", r#""9223372036854776us""#),
        (b"3000000h", r#""3000000h""#),
        (b"9223372036854775.808us", r#""9223372036854775.808us""#),
        (
            b"9223372036854ms775us808ns",
            r#""9223372036854ms775us808ns""#,
        ),
    ];
    for (input, quoted) in errors {
        let err = time::parse_duration(input).expect_err("invalid duration");
        assert_eq!(err.input, input);
        assert_eq!(
            err.to_string(),
            format!("{} is not a valid duration", escaped(quoted))
        );
    }
}

#[test]
fn parse_error_messages_follow_the_catalog() {
    let utc = Location::utc();
    let err = time::parse_in_location(RFC3339, "2024-02-30T10:00:00Z", &utc).unwrap_err();
    assert_eq!(
        err.to_string(),
        r#""2024-02-30T10:00:00Z" is not a valid RFC 3339 time"#
    );
    assert_eq!(err.kind, ParseErrorKind::DayOutOfRange);
    let err = time::parse_in_location(RFC3339_NANO, "x", &utc).unwrap_err();
    assert_eq!(err.to_string(), r#""x" is not a valid RFC 3339 time"#);
    assert_eq!(err.kind, ParseErrorKind::Syntax);
    let err = time::parse_in_location(DATE_ONLY, "2026/11/23", &utc).unwrap_err();
    assert_eq!(err.to_string(), r#""2026/11/23" is not a valid time"#);
    assert_eq!(err.layout_elem, "-");
    assert_eq!(err.value_elem, b"/11/23");
    let err = time::parse_in_location(DATE_ONLY, b"\xff<\"\n\x01\xe2\x80\xa8", &utc).unwrap_err();
    assert_eq!(
        err.to_string(),
        escaped("\"~ufffd<~\"~n~u0001~u2028\" is not a valid time")
    );
    let err = time::parse_in_location(DATE_ONLY, "2026-11-23x", &utc).unwrap_err();
    assert_eq!(err.kind, ParseErrorKind::ExtraText);
    let err = time::parse_in_location(RFC3339, "2026-11-23T23:60:00Z", &utc).unwrap_err();
    assert_eq!(err.kind, ParseErrorKind::OutOfRange("minute"));
}

fn parse_date_like(s: &str) -> Option<Time> {
    let utc = Location::utc();
    time::parse_in_location(RFC3339, s, &utc)
        .or_else(|_| time::parse_in_location(DATE_ONLY, s, &utc))
        .ok()
}

#[test]
fn dates_accepted_and_rejected_as_observed() {
    let accepted = [
        ("2024-01-01T10:00:00Z", 1704103200, 0),
        ("2024-01-01T10:00:00-00:00", 1704103200, 0),
        ("2024-01-01T10:00:00+23:59", 1704016860, 0),
        ("2024-01-01T1:00:00Z", 1704070800, 0),
        ("2024-01-01T10:00:00,5Z", 1704103200, 500_000_000),
        (
            "2024-01-01T10:00:00.1234567891234Z",
            1704103200,
            123_456_789,
        ),
        ("2024-01-01T10:00:00+24:00", 1704016800, 0),
        ("0000-01-01", -62167219200, 0),
    ];
    for (input, unix, nanos) in accepted {
        let t = parse_date_like(input).unwrap_or_else(|| panic!("{input} rejected"));
        assert_eq!((t.unix(), t.nanosecond()), (unix, nanos), "{input}");
    }
    let rejected = [
        "2024-01-01t10:00:00z",
        "2024-01-01 10:00:00Z",
        "2024-01-01T10:00:00+0100",
        "2024-01-01T10:00:00",
        "2024-01-01T10:00:60Z",
        "2024-01-01T24:00:00Z",
        "2024-01-01T10:00:00.Z",
        "2024-01-01T10:00:00+99:00",
        "2024-01-01T10:0:00Z",
        "2024-01-01T10:00:0Z",
        "2024-01-01T10:00:00ZZ",
        "2024-02-30",
        "2024-1-01",
        "2024-01-01x",
    ];
    for input in rejected {
        assert!(parse_date_like(input).is_none(), "{input} accepted");
    }
}

#[test]
fn day_truncation_floors_unix_time() {
    let day = Duration::HOUR * 24;
    let t = parse_date_like("2024-01-01T23:30:00-05:00").expect("parsed");
    assert_eq!(
        t.truncate(day).utc().format(RFC3339),
        "2024-01-02T00:00:00Z"
    );
    let t = parse_date_like("2024-01-01T10:00:00+24:00").expect("parsed");
    assert_eq!(t.truncate(day).utc().format(DATE_ONLY), "2023-12-31");
    let t = Time::from_unix(-1, 999_999_999).utc();
    assert_eq!(t.truncate(day).unix(), -86400);
    assert_eq!(t.truncate(day).nanosecond(), 0);
    let t = Time::from_unix(1_700_000_123, 987_654_321).utc();
    assert_eq!(
        t.truncate(Duration::SECOND).unix_nano(),
        1_700_000_123_000_000_000
    );
    let back = t.unix_nano() - t.truncate(Duration(7)).unix_nano();
    assert!((0..7).contains(&back));
    assert_eq!(t.round(Duration::SECOND).unix(), 1_700_000_124);
    assert_eq!(t.truncate(Duration(0)).unix_nano(), t.unix_nano());
    let paris = zone("Europe/Paris");
    let t = Time::from_unix(1_782_900_000, 0).in_location(&paris);
    assert_eq!(t.truncate(day).unix() % 86400, 0);
}

#[test]
fn iso_weeks_match_go_table() {
    let table: &[(i64, i64, i64, i64, i64)] = &[
        (1981, 1, 1, 1981, 1),
        (1982, 1, 1, 1981, 53),
        (1983, 1, 1, 1982, 52),
        (1984, 1, 1, 1983, 52),
        (1985, 1, 1, 1985, 1),
        (1986, 1, 1, 1986, 1),
        (1987, 1, 1, 1987, 1),
        (1988, 1, 1, 1987, 53),
        (1989, 1, 1, 1988, 52),
        (1990, 1, 1, 1990, 1),
        (1991, 1, 1, 1991, 1),
        (1992, 1, 1, 1992, 1),
        (1993, 1, 1, 1992, 53),
        (1994, 1, 1, 1993, 52),
        (1995, 1, 2, 1995, 1),
        (1996, 1, 1, 1996, 1),
        (1996, 1, 7, 1996, 1),
        (1996, 1, 8, 1996, 2),
        (1997, 1, 1, 1997, 1),
        (1998, 1, 1, 1998, 1),
        (1999, 1, 1, 1998, 53),
        (2000, 1, 1, 1999, 52),
        (2001, 1, 1, 2001, 1),
        (2002, 1, 1, 2002, 1),
        (2003, 1, 1, 2003, 1),
        (2004, 1, 1, 2004, 1),
        (2005, 1, 1, 2004, 53),
        (2006, 1, 1, 2005, 52),
        (2007, 1, 1, 2007, 1),
        (2008, 1, 1, 2008, 1),
        (2009, 1, 1, 2009, 1),
        (2010, 1, 1, 2009, 53),
        (2010, 1, 1, 2009, 53),
        (2011, 1, 1, 2010, 52),
        (2011, 1, 2, 2010, 52),
        (2011, 1, 3, 2011, 1),
        (2011, 1, 4, 2011, 1),
        (2011, 1, 5, 2011, 1),
        (2011, 1, 6, 2011, 1),
        (2011, 1, 7, 2011, 1),
        (2011, 1, 8, 2011, 1),
        (2011, 1, 9, 2011, 1),
        (2011, 1, 10, 2011, 2),
        (2011, 1, 11, 2011, 2),
        (2011, 6, 12, 2011, 23),
        (2011, 6, 13, 2011, 24),
        (2011, 12, 25, 2011, 51),
        (2011, 12, 26, 2011, 52),
        (2011, 12, 27, 2011, 52),
        (2011, 12, 28, 2011, 52),
        (2011, 12, 29, 2011, 52),
        (2011, 12, 30, 2011, 52),
        (2011, 12, 31, 2011, 52),
        (1995, 1, 1, 1994, 52),
        (2012, 1, 1, 2011, 52),
        (2012, 1, 2, 2012, 1),
        (2012, 1, 8, 2012, 1),
        (2012, 1, 9, 2012, 2),
        (2012, 12, 23, 2012, 51),
        (2012, 12, 24, 2012, 52),
        (2012, 12, 30, 2012, 52),
        (2012, 12, 31, 2013, 1),
        (2013, 1, 1, 2013, 1),
        (2013, 1, 6, 2013, 1),
        (2013, 1, 7, 2013, 2),
        (2013, 12, 22, 2013, 51),
        (2013, 12, 23, 2013, 52),
        (2013, 12, 29, 2013, 52),
        (2013, 12, 30, 2014, 1),
        (2014, 1, 1, 2014, 1),
        (2014, 1, 5, 2014, 1),
        (2014, 1, 6, 2014, 2),
        (2015, 1, 1, 2015, 1),
        (2016, 1, 1, 2015, 53),
        (2017, 1, 1, 2016, 52),
        (2018, 1, 1, 2018, 1),
        (2019, 1, 1, 2019, 1),
        (2020, 1, 1, 2020, 1),
        (2021, 1, 1, 2020, 53),
        (2022, 1, 1, 2021, 52),
        (2023, 1, 1, 2022, 52),
        (2024, 1, 1, 2024, 1),
        (2025, 1, 1, 2025, 1),
        (2026, 1, 1, 2026, 1),
        (2027, 1, 1, 2026, 53),
        (2028, 1, 1, 2027, 52),
        (2029, 1, 1, 2029, 1),
        (2030, 1, 1, 2030, 1),
        (2031, 1, 1, 2031, 1),
        (2032, 1, 1, 2032, 1),
        (2033, 1, 1, 2032, 53),
        (2034, 1, 1, 2033, 52),
        (2035, 1, 1, 2035, 1),
        (2036, 1, 1, 2036, 1),
        (2037, 1, 1, 2037, 1),
        (2038, 1, 1, 2037, 53),
        (2039, 1, 1, 2038, 52),
    ];
    let utc = Location::utc();
    for &(y, m, d, wy, ww) in table {
        let t = Time::from_date(y, Month(m), d, 0, 0, 0, 0, &utc);
        assert_eq!(t.iso_week(), (wy, ww), "{y}-{m}-{d}");
    }
    for year in 1950..2100 {
        let t = Time::from_date(year, Month::JANUARY, 4, 0, 0, 0, 0, &utc);
        assert_eq!(t.iso_week(), (year, 1));
    }
}

#[test]
fn time_arithmetic_and_display() {
    let utc = Location::utc();
    let a = Time::from_date(2026, Month::NOVEMBER, 23, 18, 39, 1, 500, &utc);
    let b = a.add(Duration::HOUR * 25 + Duration(600));
    assert_eq!(b.format(RFC3339_NANO), "2026-11-24T19:39:01.0000011Z");
    assert_eq!(b.sub(&a), Duration::HOUR * 25 + Duration(600));
    assert!(a.before(&b) && b.after(&a) && !a.equal(&b));
    assert_eq!(a.compare(&b), std::cmp::Ordering::Less);
    assert_eq!(a.add_date(0, 3, 10).format(DATE_ONLY), "2027-03-05");
    assert_eq!(a.to_string(), "2026-11-23 18:39:01.0000005 +0000 UTC");
    let fixed = Location::fixed("", -34200);
    assert_eq!(
        a.in_location(&fixed).to_string(),
        "2026-11-23 09:09:01.0000005 -0930 -0930"
    );
    assert_eq!(a.unix_milli(), 1795459141000);
    assert_eq!(a.unix_micro(), 1795459141000000);
    assert_eq!(Time::from_unix_milli(-1).utc().unix_nano(), -1_000_000);
    assert_eq!(
        Time::from_unix_micro(1_500_001).utc().nanosecond(),
        500_001_000
    );
    assert!(Time::default().is_zero());
    assert_eq!(Time::default().format(RFC3339), "0001-01-01T00:00:00Z");
    let far = Time::default().add(Duration::MAX).add(Duration::MAX);
    assert_eq!(far.sub(&Time::default()), Duration::MAX);
    assert_eq!(Time::default().sub(&far), Duration::MIN);
    assert_eq!(Duration(-1_500_000_000).abs(), Duration(1_500_000_000));
    assert_eq!(Duration::MIN.abs(), Duration::MAX);
    assert_eq!(Duration::HOUR.hours().to_bits(), 1.0f64.to_bits());
    assert_eq!(Duration::MINUTE.minutes().to_bits(), 1.0f64.to_bits());
    assert_eq!(Duration(1_500).microseconds(), 1);
    assert_eq!(-Duration::SECOND, Duration(-1_000_000_000));
    assert_eq!(2 * Duration::SECOND - Duration::SECOND, Duration::SECOND);
    let mut out = Vec::new();
    a.append_strict_rfc3339(&mut out).expect("in range");
    assert_eq!(out, b"2026-11-23T18:39:01.0000005Z");
    let mut out = Vec::new();
    let big = Time::from_date(10000, Month::JANUARY, 1, 0, 0, 0, 0, &utc);
    assert_eq!(
        big.append_strict_rfc3339(&mut out),
        Err(time::StrictRfc3339Error::YearOutOfRange)
    );
    let mut out = Vec::new();
    let odd = a.in_location(&Location::fixed("", 24 * 3600));
    assert_eq!(
        odd.append_strict_rfc3339(&mut out),
        Err(time::StrictRfc3339Error::ZoneHourOutOfRange)
    );
}

#[test]
fn generic_layouts_parse_like_go() {
    let utc = Location::utc();
    let paris = zone("Europe/Paris");
    let t = time::parse_in_location(time::RFC1123, "Mon, 23 Nov 2026 18:39:01 CET", &paris)
        .expect("parsed");
    assert_eq!(t.unix(), 1795459141 - 3600);
    assert_eq!(t.zone(), ("CET", 3600));
    let t = time::parse_in_location(time::RFC1123, "Mon, 23 Nov 2026 18:39:01 PST", &utc)
        .expect("parsed");
    assert_eq!(t.zone(), ("PST", 0));
    let t = time::parse_in_location(time::RFC850, "Monday, 23-Nov-26 18:39:01 GMT+2", &utc)
        .expect("parsed");
    assert_eq!(t.zone(), ("GMT+2", 7200));
    let t = time::parse_in_location(time::KITCHEN, "12:04AM", &utc).expect("parsed");
    assert_eq!(t.clock(), (0, 4, 0));
    let t = time::parse_in_location("2006 002 15:04:05.000", "2024 060 01:02:03.456", &utc)
        .expect("parsed");
    assert_eq!(t.date(), (2024, Month::FEBRUARY, 29));
    assert_eq!(t.nanosecond(), 456_000_000);
    let t =
        time::parse_in_location(time::STAMP_MICRO, "Feb  3 04:05:06.000007", &utc).expect("parsed");
    assert_eq!(t.format(time::STAMP_MICRO), "Feb  3 04:05:06.000007");
    let t = time::parse_in_location(
        "2006-01-02 15:04:05 -07:00:00",
        "2026-11-23 18:39:01 +05:30:15",
        &utc,
    )
    .expect("parsed");
    assert_eq!(t.zone(), ("", 19815));
    assert_eq!(
        t.format("Monday January _2 __2 002 3:4:5 pm Z07 -07 -070000"),
        "Monday November 23 327 327 6:39:1 pm +05 +05 +053015"
    );
    let err = time::parse_in_location("2006 002", "2023 060x", &utc).unwrap_err();
    assert_eq!(err.kind, ParseErrorKind::ExtraText);
    let err = time::parse_in_location("2006 002 Jan", "2023 060 Feb", &utc).unwrap_err();
    assert_eq!(err.kind, ParseErrorKind::DayOfYearMonthMismatch);
}

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "elysiandb-gocompat-time-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temporary folder");
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn put(&self, rel: &str, data: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(&path, data).expect("write");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

const SUMMER_2026: i64 = 1_782_900_000;

fn offset_in_summer(loc: &Location) -> i32 {
    Time::from_unix(SUMMER_2026, 0).in_location(loc).zone().1
}

fn os(p: &Path) -> &OsStr {
    p.as_os_str()
}

#[test]
fn local_zone_lookup_follows_go_order() {
    let paris = zone_data("Europe/Paris");
    let tokyo = zone_data("Asia/Tokyo");
    let kolkata = zone_data("Asia/Kolkata");

    let etc = TempDir::new();
    etc.put("localtime", &paris);
    let empty_etc = TempDir::new();
    let first = TempDir::new();
    let second = TempDir::new();
    first.put("Europe/Paris", &paris);
    second.put("Europe/Paris", &tokyo);
    second.put("Asia/Kolkata", &kolkata);
    first.put("Asia/Kolkata", b"not a zone");
    std::fs::create_dir_all(first.path().join("Asia/Tokyo")).expect("folder");
    second.put("Asia/Tokyo", &tokyo);
    let platform = [os(first.path()), os(second.path())];

    let unset = time::local_location_with(None, os(etc.path()), &platform);
    assert_eq!(unset.name(), "Local");
    assert_eq!(offset_in_summer(&unset), 7200);
    assert_eq!(
        Time::from_unix(SUMMER_2026, 0).in_location(&unset).zone(),
        ("CEST", 7200)
    );

    let missing = time::local_location_with(None, os(empty_etc.path()), &platform);
    assert_eq!(missing.name(), "UTC");
    assert_eq!(
        Time::from_unix(SUMMER_2026, 0).in_location(&missing).zone(),
        ("UTC", 0)
    );

    let empty = time::local_location_with(Some(OsStr::new("")), os(etc.path()), &platform);
    assert_eq!(empty.name(), "UTC");
    assert_eq!(offset_in_summer(&empty), 0);

    let utc = time::local_location_with(Some(OsStr::new("UTC")), os(etc.path()), &platform);
    assert_eq!(utc.name(), "UTC");
    assert_eq!(offset_in_summer(&utc), 0);

    let named =
        time::local_location_with(Some(OsStr::new("Europe/Paris")), os(etc.path()), &platform);
    assert_eq!(named.name(), "Europe/Paris");
    assert_eq!(offset_in_summer(&named), 7200);

    let colon =
        time::local_location_with(Some(OsStr::new(":Europe/Paris")), os(etc.path()), &platform);
    assert_eq!(colon.name(), "Europe/Paris");
    assert_eq!(offset_in_summer(&colon), 7200);

    let malformed_first =
        time::local_location_with(Some(OsStr::new("Asia/Kolkata")), os(etc.path()), &platform);
    assert_eq!(offset_in_summer(&malformed_first), 19800);

    let folder_first =
        time::local_location_with(Some(OsStr::new("Asia/Tokyo")), os(etc.path()), &platform);
    assert_eq!(offset_in_summer(&folder_first), 32400);

    let absolute_path = second.path().join("Asia/Kolkata");
    let absolute = time::local_location_with(Some(os(&absolute_path)), os(etc.path()), &platform);
    assert_eq!(absolute.name(), absolute_path.to_string_lossy());
    assert_eq!(offset_in_summer(&absolute), 19800);

    let mut colon_absolute_tz = std::ffi::OsString::from(":");
    colon_absolute_tz.push(absolute_path.as_os_str());
    let colon_absolute = time::local_location_with(
        Some(colon_absolute_tz.as_os_str()),
        os(etc.path()),
        &platform,
    );
    assert_eq!(colon_absolute.name(), absolute_path.to_string_lossy());
    assert_eq!(offset_in_summer(&colon_absolute), 19800);

    let absolute_missing = time::local_location_with(
        Some(first.path().join("Nowhere").as_os_str()),
        os(etc.path()),
        &platform,
    );
    assert_eq!(absolute_missing.name(), "UTC");
    assert_eq!(offset_in_summer(&absolute_missing), 0);

    let absolute_only = time::local_location_with(
        Some(first.path().join("Asia/Kolkata").as_os_str()),
        os(etc.path()),
        &platform,
    );
    assert_eq!(absolute_only.name(), "UTC");

    let unknown =
        time::local_location_with(Some(OsStr::new("Nowhere/City")), os(etc.path()), &platform);
    assert_eq!(unknown.name(), "UTC");
    assert_eq!(offset_in_summer(&unknown), 0);
}

const LOCAL_CHILD: &str = "ELYSIAN_TIME_LOCAL_CHILD";
const CHILD_TEST: &str = "process_local_zone_child";

#[test]
fn process_local_zone_child() {
    let Some(expected) = std::env::var_os(LOCAL_CHILD) else {
        return;
    };
    let local = Location::local();
    let now = Time::from_unix(SUMMER_2026, 0);
    if expected == "utc" {
        assert_eq!(local.name(), "UTC");
        assert_eq!(now.zone(), ("UTC", 0));
    } else {
        let tz = std::env::var_os("TZ").expect("TZ is set for the child");
        assert_eq!(local.name(), tz.to_string_lossy());
        assert_eq!(now.zone(), ("JST", 32400));
    }
}

fn run_local_child(tz: &OsStr, zoneinfo: &Path, expected: &str) {
    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([CHILD_TEST, "--exact", "--test-threads=1"])
        .env(LOCAL_CHILD, expected)
        .env("TZ", tz)
        .env("ZONEINFO", zoneinfo)
        .output()
        .expect("child test run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("1 passed"), "{stdout}");
}

#[test]
fn zoneinfo_has_no_effect_on_the_local_zone() {
    let tokyo = zone_data("Asia/Tokyo");
    let zoneinfo = TempDir::new();
    let name = "Elysian_Nowhere/Only_In_Zoneinfo";
    zoneinfo.put(name, &tokyo);
    let zip_dir = TempDir::new();
    let zip_path = zip_dir.put("zones.zip", &stored_zip(&[(name, tokyo.as_slice())]));
    let empty_platform = TempDir::new();
    let etc = TempDir::new();

    let loaded = time::load_location_with(name, Some(os(zoneinfo.path())), &[])
        .expect("LoadLocation reads ZONEINFO");
    assert_eq!(offset_in_summer(&loaded), 32400);
    let loaded = time::load_location_with(name, Some(os(&zip_path)), &[])
        .expect("LoadLocation reads a ZONEINFO zip");
    assert_eq!(offset_in_summer(&loaded), 32400);

    let local = time::local_location_with(
        Some(OsStr::new(name)),
        os(etc.path()),
        &[os(empty_platform.path())],
    );
    assert_eq!(local.name(), "UTC");
    assert_eq!(offset_in_summer(&local), 0);

    run_local_child(OsStr::new(name), zoneinfo.path(), "utc");
    run_local_child(OsStr::new(name), &zip_path, "utc");
    let tokyo_file = zoneinfo.path().join(name);
    run_local_child(os(&tokyo_file), zoneinfo.path(), "tokyo");
}

#[test]
fn load_location_follows_go_rules() {
    let paris = zone_data("Europe/Paris");
    let tokyo = zone_data("Asia/Tokyo");
    let first = TempDir::new();
    let second = TempDir::new();
    first.put("Europe/Paris", b"TZif broken");
    second.put("Europe/Paris", &paris);
    second.put("Asia/Tokyo", &tokyo);
    let zoneinfo = TempDir::new();
    zoneinfo.put("Asia/Tokyo", &paris);
    zoneinfo.put("Europe/Paris", b"broken");
    let platform = [os(first.path()), os(second.path())];

    assert!(
        time::load_location_with("", None, &platform)
            .unwrap()
            .is_utc()
    );
    assert!(
        time::load_location_with("UTC", None, &platform)
            .unwrap()
            .is_utc()
    );
    assert!(
        time::load_location_with("Local", None, &platform)
            .unwrap()
            .is_local()
    );
    for bad in ["../etc/passwd", "a/../b", "/etc/localtime", "\\x"] {
        match time::load_location_with(bad, None, &platform) {
            Err(e @ ZoneError::InvalidName(_)) => {
                assert!(e.to_string().ends_with(" is not a valid time zone name"));
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
    let paris_loc = time::load_location_with("Europe/Paris", None, &platform).expect("paris");
    assert_eq!(paris_loc.name(), "Europe/Paris");
    assert_eq!(offset_in_summer(&paris_loc), 7200);
    let tokyo_loc = time::load_location_with("Asia/Tokyo", Some(os(zoneinfo.path())), &platform)
        .expect("tokyo");
    assert_eq!(offset_in_summer(&tokyo_loc), 7200);
    let broken_zoneinfo =
        time::load_location_with("Europe/Paris", Some(os(zoneinfo.path())), &platform)
            .expect("falls back to the platform folders");
    assert_eq!(offset_in_summer(&broken_zoneinfo), 7200);
    match time::load_location_with("Nowhere/City", None, &platform) {
        Err(e @ ZoneError::Unknown(_)) => {
            assert_eq!(e.to_string(), r#"unknown time zone "Nowhere/City""#)
        }
        other => panic!("{other:?}"),
    }
    match time::load_location_with("Europe/Paris", None, &[os(first.path())]) {
        Err(e @ ZoneError::Malformed) => assert_eq!(e.to_string(), "invalid time zone data"),
        other => panic!("{other:?}"),
    }
    let from_file =
        time::load_location_from_file("Paris", &second.path().join("Europe/Paris")).expect("file");
    assert_eq!(from_file.name(), "Paris");
    assert_eq!(offset_in_summer(&from_file), 7200);
    match time::load_location_from_file("x", &first.path().join("missing")) {
        Err(e) => {
            assert!(e.is_not_found());
            assert!(e.to_string().starts_with("open "));
        }
        Ok(_) => panic!("missing file loaded"),
    }
    assert!(matches!(
        time::load_location_from_tzdata("x", b"TZif"),
        Err(ZoneError::Malformed)
    ));
}

#[test]
fn fixed_zones_and_location_identity() {
    let fixed = Location::fixed("X", 45296);
    assert_eq!(fixed.name(), "X");
    assert!(!fixed.is_utc());
    let t = Time::from_unix(0, 0).in_location(&fixed);
    assert_eq!(t.zone(), ("X", 45296));
    assert_eq!(t.zone_bounds().0.map(|s| s.unix()), None);
    assert_eq!(t.zone_bounds().1.map(|s| s.unix()), None);
    assert_eq!(t.format("-07:00:00 Z070000 MST"), "+12:34:56 +123456 X");
    assert!(Time::from_unix(0, 0).location().is_local());
    assert!(Time::from_unix(0, 0).utc().location().is_utc());
    assert_eq!(Location::utc().to_string(), "UTC");
    let tz_utc = zone("UTC");
    assert!(!tz_utc.is_utc());
    assert_eq!(
        Time::from_unix(0, 0).in_location(&tz_utc).zone(),
        ("UTC", 0)
    );
}

#[test]
fn set_local_replaces_the_process_zone_once() {
    let paris = zone("Europe/Paris");
    assert!(time::set_local(&paris));
    assert!(!time::set_local(&Location::utc()));
    assert_eq!(Location::local().name(), "Europe/Paris");
    let t = Time::from_unix(SUMMER_2026, 0);
    assert_eq!(t.zone(), ("CEST", 7200));
    let parsed = time::parse(RFC3339, "2026-07-01T12:00:00+02:00").expect("parsed");
    assert!(parsed.location().is_local());
    let parsed = time::parse(DATE_ONLY, "2026-07-01").expect("parsed");
    assert!(parsed.location().is_utc());
}
