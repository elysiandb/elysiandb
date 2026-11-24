use crate::*;
use elysiandb::archive::tar::internals;

#[test]
fn fits_in_base256() {
    let vectors: [(i64, usize, bool); 15] = [
        (1, 8, true),
        (0, 8, true),
        (-1, 8, true),
        (1 << 56, 8, false),
        ((1 << 56) - 1, 8, true),
        (-1 << 56, 8, true),
        ((-1 << 56) - 1, 8, false),
        (121654, 8, true),
        (-9849849, 8, true),
        (i64::MAX, 9, true),
        (0, 9, true),
        (i64::MIN, 9, true),
        (i64::MAX, 12, true),
        (0, 12, true),
        (i64::MIN, 12, true),
    ];
    for (input, width, ok) in vectors {
        assert_eq!(
            internals::fits_in_base256(width, input),
            ok,
            "{input} {width}"
        );
    }
}

#[test]
fn parse_numeric() {
    let vectors: Vec<(&[u8], i64, bool)> = vec![
        (b"", 0, true),
        (b"\x80", 0, true),
        (b"\x80\x00", 0, true),
        (b"\x80\x00\x00", 0, true),
        (b"\xbf", (1 << 6) - 1, true),
        (b"\xbf\xff", (1 << 14) - 1, true),
        (b"\xbf\xff\xff", (1 << 22) - 1, true),
        (b"\xff", -1, true),
        (b"\xff\xff", -1, true),
        (b"\xff\xff\xff", -1, true),
        (b"\xc0", -(1 << 6), true),
        (b"\xc0\x00", -(1 << 14), true),
        (b"\xc0\x00\x00", -(1 << 22), true),
        (
            b"\x87\x76\xa2\x22\xeb\x8a\x72\x61",
            537795476381659745,
            true,
        ),
        (
            b"\x80\x00\x00\x00\x07\x76\xa2\x22\xeb\x8a\x72\x61",
            537795476381659745,
            true,
        ),
        (
            b"\xf7\x76\xa2\x22\xeb\x8a\x72\x61",
            -615126028225187231,
            true,
        ),
        (
            b"\xff\xff\xff\xff\xf7\x76\xa2\x22\xeb\x8a\x72\x61",
            -615126028225187231,
            true,
        ),
        (b"\x80\x7f\xff\xff\xff\xff\xff\xff\xff", i64::MAX, true),
        (b"\x80\x80\x00\x00\x00\x00\x00\x00\x00", 0, false),
        (b"\xff\x80\x00\x00\x00\x00\x00\x00\x00", i64::MIN, true),
        (b"\xff\x7f\xff\xff\xff\xff\xff\xff\xff", 0, false),
        (
            b"\xf5\xec\xd1\xc7\x7e\x5f\x26\x48\x81\x9f\x8f\x9b",
            0,
            false,
        ),
        (b"0000000\x00", 0, true),
        (b" \x0000000\x00", 0, true),
        (b" \x0000003\x00", 3, true),
        (b"00000000227\x00", 0o227, true),
        (b"032033\x00 ", 0o32033, true),
        (b"320330\x00 ", 0o320330, true),
        (b"0000660\x00 ", 0o660, true),
        (b"\x00 0000660\x00 ", 0o660, true),
        (b"0123456789abcdef", 0, false),
        (b"0123456789\x00abcdef", 0, false),
        (b"01234567\x0089abcdef", 342391, true),
        (b"0123\x7e\x5f\x264123", 0, false),
    ];
    for (input, want, ok) in vectors {
        let (got, got_ok) = internals::parse_numeric(input);
        assert_eq!(got_ok, ok, "{input:?}");
        if ok {
            assert_eq!(got, want, "{input:?}");
        }
    }
}

#[test]
fn format_numeric() {
    let vectors: Vec<(i64, &[u8], bool)> = vec![
        (0, b"0\x00", true),
        (7, b"7\x00", true),
        (8, b"\x80\x08", true),
        (0o77, b"77\x00", true),
        (0o100, b"\x80\x00\x40", true),
        (0, b"0000000\x00", true),
        (0o123, b"0000123\x00", true),
        (0o7654321, b"7654321\x00", true),
        (0o7777777, b"7777777\x00", true),
        (0o10000000, b"\x80\x00\x00\x00\x00\x20\x00\x00", true),
        (0, b"00000000000\x00", true),
        (0o1234567, b"00001234567\x00", true),
        (0o76543210321, b"76543210321\x00", true),
        (0o12345670123, b"12345670123\x00", true),
        (0o77777777777, b"77777777777\x00", true),
        (
            0o100000000000,
            b"\x80\x00\x00\x00\x00\x00\x00\x02\x00\x00\x00\x00",
            true,
        ),
        (i64::MAX, b"777777777777777777777\x00", true),
        (-1, b"\xff", true),
        (-1, b"\xff\xff", true),
        (-1, b"\xff\xff\xff", true),
        (1, b"0", false),
        ((1 << 8) - 1, b"\x80\xff", true),
        (1 << 8, b"0\x00", false),
        ((1 << 16) - 1, b"\x80\xff\xff", true),
        (1 << 16, b"00\x00", false),
        (-1, b"\xff", true),
        (-2, b"0", false),
        (-(1 << 8), b"\xff\x00", true),
        (-(1 << 8) - 1, b"0\x00", false),
        (-(1 << 16), b"\xff\x00\x00", true),
        (-(1 << 16) - 1, b"00\x00", false),
        (537795476381659745, b"0000000\x00", false),
        (
            537795476381659745,
            b"\x80\x00\x00\x00\x07\x76\xa2\x22\xeb\x8a\x72\x61",
            true,
        ),
        (-615126028225187231, b"0000000\x00", false),
        (
            -615126028225187231,
            b"\xff\xff\xff\xff\xf7\x76\xa2\x22\xeb\x8a\x72\x61",
            true,
        ),
        (i64::MAX, b"0000000\x00", false),
        (
            i64::MAX,
            b"\x80\x00\x00\x00\x7f\xff\xff\xff\xff\xff\xff\xff",
            true,
        ),
        (i64::MIN, b"0000000\x00", false),
        (
            i64::MIN,
            b"\xff\xff\xff\xff\x80\x00\x00\x00\x00\x00\x00\x00",
            true,
        ),
        (i64::MAX, b"\x80\x7f\xff\xff\xff\xff\xff\xff\xff", true),
        (i64::MIN, b"\xff\x80\x00\x00\x00\x00\x00\x00\x00", true),
    ];
    for (input, want, ok) in vectors {
        let mut got = vec![0u8; want.len()];
        let got_ok = internals::format_numeric(&mut got, input);
        assert_eq!(got_ok, ok, "{input}");
        assert_eq!(got, want, "{input}");
    }
}

#[test]
fn fits_in_octal() {
    let vectors: [(i64, usize, bool); 23] = [
        (-1, 1, false),
        (-1, 2, false),
        (-1, 3, false),
        (0, 1, true),
        (1, 1, false),
        (0, 2, true),
        (0o7, 2, true),
        (0o7 + 1, 2, false),
        (0, 4, true),
        (0o777, 4, true),
        (0o777 + 1, 4, false),
        (0, 8, true),
        (0o7777777, 8, true),
        (0o7777777 + 1, 8, false),
        (0, 12, true),
        (0o77777777777, 12, true),
        (0o77777777777 + 1, 12, false),
        (i64::MAX, 22, true),
        (0o12345670123, 12, true),
        (0o1564164, 12, true),
        (-0o12345670123, 12, false),
        (-0o1564164, 12, false),
        (-1564164, 30, false),
    ];
    for (input, width, ok) in vectors {
        assert_eq!(
            internals::fits_in_octal(width, input),
            ok,
            "{input} {width}"
        );
    }
}

#[test]
fn parse_pax_time() {
    let vectors: Vec<(&str, Option<(i64, i64)>)> = vec![
        ("1350244992.023960108", Some((1350244992, 23960108))),
        ("1350244992.02396010", Some((1350244992, 23960100))),
        ("1350244992.0239601089", Some((1350244992, 23960108))),
        ("1350244992.3", Some((1350244992, 300000000))),
        ("1350244992", Some((1350244992, 0))),
        ("-1.000000001", Some((-1, -1))),
        ("-1.000001", Some((-1, -1000))),
        ("-1.001000", Some((-1, -1_000_000))),
        ("-1", Some((-1, 0))),
        ("-1.999000", Some((-1, -1_000_000_000 + 1_000_000))),
        ("-1.999999", Some((-1, -1_000_000_000 + 1000))),
        ("-1.999999999", Some((-1, -1_000_000_000 + 1))),
        ("0.000000001", Some((0, 1))),
        ("0.000001", Some((0, 1000))),
        ("0.001000", Some((0, 1_000_000))),
        ("0", Some((0, 0))),
        ("0.999000", Some((0, 1_000_000_000 - 1_000_000))),
        ("0.999999", Some((0, 1_000_000_000 - 1000))),
        ("0.999999999", Some((0, 1_000_000_000 - 1))),
        ("1.000000001", Some((1, 1))),
        ("1.000001", Some((1, 1000))),
        ("1.001000", Some((1, 1_000_000))),
        ("1", Some((1, 0))),
        ("1.999000", Some((1, 1_000_000_000 - 1_000_000))),
        ("1.999999", Some((1, 1_000_000_000 - 1000))),
        ("1.999999999", Some((1, 1_000_000_000 - 1))),
        ("-1350244992.023960108", Some((-1350244992, -23960108))),
        ("-1350244992.02396010", Some((-1350244992, -23960100))),
        ("-1350244992.0239601089", Some((-1350244992, -23960108))),
        ("-1350244992.3", Some((-1350244992, -300000000))),
        ("-1350244992", Some((-1350244992, 0))),
        ("", None),
        ("0", Some((0, 0))),
        ("1.", Some((1, 0))),
        ("0.0", Some((0, 0))),
        (".5", None),
        ("-1.3", Some((-1, -300_000_000))),
        ("-1.0", Some((-1, 0))),
        ("-0.0", Some((0, 0))),
        ("-0.1", Some((0, -100_000_000))),
        ("-0.01", Some((0, -10_000_000))),
        ("-0.99", Some((0, -990_000_000))),
        ("-0.98", Some((0, -980_000_000))),
        ("-1.1", Some((-1, -100_000_000))),
        ("-1.01", Some((-1, -10_000_000))),
        ("-2.99", Some((-2, -990_000_000))),
        ("-5.98", Some((-5, -980_000_000))),
        ("-", None),
        ("+", None),
        ("-1.-1", None),
        ("99999999999999999999999999999999999999999999999", None),
        ("0.123456789abcdef", None),
        ("foo", None),
        ("\x00", None),
        ("𝟵𝟴𝟳𝟲𝟱.𝟰𝟯𝟮𝟭𝟬", None),
        ("98765﹒43210", None),
    ];
    for (input, want) in vectors {
        let got = internals::parse_pax_time(input.as_bytes());
        match (got, want) {
            (Some(ts), Some((sec, nsec))) => {
                assert!(ts.equal(&unix(sec, nsec)), "{input:?}: {ts:?}");
            }
            (None, None) => {}
            (got, want) => panic!("{input:?}: got {got:?}, want {want:?}"),
        }
    }
}

#[test]
fn format_pax_time() {
    let vectors: [(i64, i64, &str); 29] = [
        (1350244992, 0, "1350244992"),
        (1350244992, 300000000, "1350244992.3"),
        (1350244992, 23960100, "1350244992.0239601"),
        (1350244992, 23960108, "1350244992.023960108"),
        (1, 1_000_000_000 - 1, "1.999999999"),
        (1, 1_000_000_000 - 1000, "1.999999"),
        (1, 1_000_000_000 - 1_000_000, "1.999"),
        (1, 0, "1"),
        (1, 1_000_000, "1.001"),
        (1, 1000, "1.000001"),
        (1, 1, "1.000000001"),
        (0, 1_000_000_000 - 1, "0.999999999"),
        (0, 1_000_000_000 - 1000, "0.999999"),
        (0, 1_000_000_000 - 1_000_000, "0.999"),
        (0, 0, "0"),
        (0, 1_000_000, "0.001"),
        (0, 1000, "0.000001"),
        (0, 1, "0.000000001"),
        (-1, -1_000_000_000 + 1, "-1.999999999"),
        (-1, -1_000_000_000 + 1000, "-1.999999"),
        (-1, -1_000_000_000 + 1_000_000, "-1.999"),
        (-1, 0, "-1"),
        (-1, -1_000_000, "-1.001"),
        (-1, -1000, "-1.000001"),
        (-1, -1, "-1.000000001"),
        (-1350244992, 0, "-1350244992"),
        (-1350244992, -300000000, "-1350244992.3"),
        (-1350244992, -23960100, "-1350244992.0239601"),
        (-1350244992, -23960108, "-1350244992.023960108"),
    ];
    for (sec, nsec, want) in vectors {
        let got = internals::format_pax_time(&unix(sec, nsec));
        assert_eq!(String::from_utf8(got).unwrap(), want, "{sec}s {nsec}ns");
    }
}

#[test]
fn parse_pax_record() {
    let med_name = "CD".repeat(50);
    let long_name = "AB".repeat(100);

    let vectors: Vec<(String, &str, &str, &str, bool)> = vec![
        ("6 k=v\n\n".into(), "\n", "k", "v", true),
        ("19 path=/etc/hosts\n".into(), "", "path", "/etc/hosts", true),
        (
            format!("210 path={long_name}\nabc"),
            "abc",
            "path",
            &long_name,
            true,
        ),
        (format!("110 path={med_name}\n"), "", "path", &med_name, true),
        ("9 foo=ba\n".into(), "", "foo", "ba", true),
        ("11 foo=bar\n\x00".into(), "\x00", "foo", "bar", true),
        (
            "18 foo=b=\nar=\n==\x00\n".into(),
            "",
            "foo",
            "b=\nar=\n==\x00",
            true,
        ),
        (
            "27 foo=hello9 foo=ba\nworld\n".into(),
            "",
            "foo",
            "hello9 foo=ba\nworld",
            true,
        ),
        (
            "27 ☺☻☹=日a本b語ç\nmeow mix".into(),
            "meow mix",
            "☺☻☹",
            "日a本b語ç",
            true,
        ),
        ("17 \x00hello=\x00world\n".into(), "", "", "", false),
        ("1 k=1\n".into(), "", "", "", false),
        ("6 k~1\n".into(), "", "", "", false),
        ("6_k=1\n".into(), "", "", "", false),
        ("6 k=1 ".into(), "", "", "", false),
        ("632 k=1\n".into(), "", "", "", false),
        ("16 longkeyname=hahaha\n".into(), "", "", "", false),
        ("3 somelongkey=\n".into(), "", "", "", false),
        ("50 tooshort=\n".into(), "", "", "", false),
        (
            "0000000000000000000000000000000030 mtime=1432668921.098285006\n30 ctime=2147483649.15163319".into(),
            "",
            "",
            "",
            false,
        ),
        ("06 k=v\n".into(), "", "", "", false),
        ("00006 k=v\n".into(), "", "", "", false),
        ("000006 k=v\n".into(), "", "", "", false),
        ("000000 k=v\n".into(), "", "", "", false),
        ("0 k=v\n".into(), "", "", "", false),
        ("+0000005 x=\n".into(), "", "", "", false),
    ];

    for (input, want_res, want_key, want_val, ok) in &vectors {
        match internals::parse_pax_record(input.as_bytes()) {
            Some((k, v, res)) => {
                assert!(ok, "{input:?}: got success, want failure");
                assert_eq!(k, want_key.as_bytes(), "{input:?}");
                assert_eq!(v, want_val.as_bytes(), "{input:?}");
                assert_eq!(res, want_res.as_bytes(), "{input:?}");
            }
            None => assert!(!ok, "{input:?}: got failure, want success"),
        }
    }
}

#[test]
fn format_pax_record() {
    let med_name = "CD".repeat(50);
    let long_name = "AB".repeat(100);

    let vectors: Vec<(String, String, String, bool)> = vec![
        ("k".into(), "v".into(), "6 k=v\n".into(), true),
        (
            "path".into(),
            "/etc/hosts".into(),
            "19 path=/etc/hosts\n".into(),
            true,
        ),
        (
            "path".into(),
            long_name.clone(),
            format!("210 path={long_name}\n"),
            true,
        ),
        (
            "path".into(),
            med_name.clone(),
            format!("110 path={med_name}\n"),
            true,
        ),
        ("foo".into(), "ba".into(), "9 foo=ba\n".into(), true),
        ("foo".into(), "bar".into(), "11 foo=bar\n".into(), true),
        (
            "foo".into(),
            "b=\nar=\n==\x00".into(),
            "18 foo=b=\nar=\n==\x00\n".into(),
            true,
        ),
        (
            "foo".into(),
            "hello9 foo=ba\nworld".into(),
            "27 foo=hello9 foo=ba\nworld\n".into(),
            true,
        ),
        (
            "☺☻☹".into(),
            "日a本b語ç".into(),
            "27 ☺☻☹=日a本b語ç\n".into(),
            true,
        ),
        (
            "xhello".into(),
            "\x00world".into(),
            "17 xhello=\x00world\n".into(),
            true,
        ),
        ("path".into(), "null\x00".into(), String::new(), false),
        ("null\x00".into(), "value".into(), String::new(), false),
        (
            "SCHILY.xattr.key".into(),
            "null\x00".into(),
            "26 SCHILY.xattr.key=null\x00\n".into(),
            true,
        ),
    ];

    for (k, v, want, ok) in &vectors {
        match internals::format_pax_record(k.as_bytes(), v.as_bytes()) {
            Some(got) => {
                assert!(ok, "{k:?} {v:?}: got success, want failure");
                assert_eq!(got, want.as_bytes(), "{k:?} {v:?}");
            }
            None => assert!(!ok, "{k:?} {v:?}: got failure, want success"),
        }
    }
}
