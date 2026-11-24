use super::iter::Iter;
use super::normalize::Form;

#[test]
fn example_form_next_boundary() {
    let s = Form::Nfd.string("M\u{ea}l\u{e9}e".as_bytes()).into_owned();
    let mut got: Vec<&[u8]> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let d = Form::Nfc.next_boundary_in_string(&s[i..], true) as usize;
        got.push(&s[i..i + d]);
        i += d;
    }
    let want: [&[u8]; 5] = [
        b"M",
        "e\u{302}".as_bytes(),
        b"l",
        "e\u{301}".as_bytes(),
        b"e",
    ];
    assert_eq!(got, want);
}

fn equal_simple(a: &[u8], b: &[u8]) -> bool {
    let mut ia = Iter::new();
    let mut ib = Iter::new();
    ia.init_string(Form::Nfkd, a);
    ib.init_string(Form::Nfkd, b);
    while !ia.done() && !ib.done() {
        if ia.next() != ib.next() {
            return false;
        }
    }
    ia.done() && ib.done()
}

fn find_prefix(a: &[u8], b: &[u8]) -> usize {
    let mut i = 0;
    while i < a.len() && i < b.len() && a[i] < 0x80 && a[i] == b[i] {
        i += 1;
    }
    i
}

fn equal_opt(a: &[u8], b: &[u8]) -> bool {
    let n = find_prefix(a, b);
    let (a, b) = (&a[n..], &b[n..]);
    let mut ia = Iter::new();
    let mut ib = Iter::new();
    ia.init_string(Form::Nfkd, a);
    ib.init_string(Form::Nfkd, b);
    while !ia.done() && !ib.done() {
        if ia.next() != ib.next() {
            return false;
        }
        let n = find_prefix(&a[ia.pos()..], &b[ib.pos()..]) as i64;
        if n != 0 {
            let _ = ia.seek(n, 1);
            let _ = ib.seek(n, 1);
        }
    }
    ia.done() && ib.done()
}

#[test]
fn example_iter() {
    let compare_tests: [(&str, &str); 6] = [
        ("aaa", "aaa"),
        ("aaa", "aab"),
        ("a\u{300}a", "\u{e0}a"),
        ("a\u{300}\u{320}b", "a\u{320}\u{300}b"),
        ("\u{1e0a}\u{323}", "\u{44}\u{323}\u{307}"),
        ("\u{3304}", "\u{30a4}\u{30cb}\u{30f3}\u{30af}\u{3099}"),
    ];
    let want = [
        (true, true),
        (false, false),
        (true, true),
        (true, true),
        (true, true),
        (true, true),
    ];
    for (i, t) in compare_tests.iter().enumerate() {
        let r0 = equal_simple(t.0.as_bytes(), t.1.as_bytes());
        let r1 = equal_opt(t.0.as_bytes(), t.1.as_bytes());
        assert_eq!((r0, r1), want[i], "{i}");
    }
}
