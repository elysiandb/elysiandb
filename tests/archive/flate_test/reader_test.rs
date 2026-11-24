use elysiandb::archive::flate;
use std::io;

#[test]
fn nlit_out_of_range() {
    let data: &[u8] = b"\xfc\xfe\x36\xe7\x5e\x1c\xef\xb3\x55\x58\x77\xb6\x56\xb5\x43\xf4\
\x6f\xf2\xd2\xe6\x3d\x99\xa0\x85\x8c\x48\xeb\xf8\xda\x83\x04\x2a\
\x75\xc4\xf8\x0f\x12\x11\xb9\xb4\x4b\x09\xa0\xbe\x8b\x91\x4c";
    let mut r = flate::new_reader(data);
    let err = io::copy(&mut r, &mut io::sink()).expect_err("bogus data");
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(
        err.to_string()
            .starts_with("corrupt compressed data at byte "),
        "{err}"
    );
}
