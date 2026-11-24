use elysiandb::crypto::subtle::{constant_time_byte_eq, constant_time_compare};

#[test]
fn constant_time_compare_matches_go() {
    let data: [(&[u8], &[u8], i32); 5] = [
        (&[], &[], 1),
        (&[0x11], &[0x11], 1),
        (&[0x12], &[0x11], 0),
        (&[0x11], &[0x11, 0x12], 0),
        (&[0x11, 0x12], &[0x11], 0),
    ];
    for (i, (a, b, out)) in data.iter().enumerate() {
        assert_eq!(constant_time_compare(a, b), *out, "#{i}");
    }
}

#[test]
fn constant_time_byte_eq_matches_go() {
    let data: [(u8, u8, i32); 5] = [
        (0, 0, 1),
        (0, 1, 0),
        (1, 0, 0),
        (0xff, 0xff, 1),
        (0xff, 0xfe, 0),
    ];
    for (i, (a, b, out)) in data.iter().enumerate() {
        assert_eq!(constant_time_byte_eq(*a, *b), *out, "#{i}");
    }
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            assert_eq!(constant_time_byte_eq(a, b), i32::from(a == b), "{a} {b}");
        }
    }
}

#[test]
fn constant_time_compare_checks_every_byte() {
    let a = vec![0x5au8; 4096];
    for i in 0..a.len() {
        let mut b = a.clone();
        b[i] ^= 0x01;
        assert_eq!(constant_time_compare(&a, &b), 0, "{i}");
    }
    assert_eq!(constant_time_compare(&a, &a.clone()), 1);
}
