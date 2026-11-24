use goja::ftoa;

#[test]
fn test_f_to_base_str() {
    let s = ftoa::f_to_base_str(0.8466400793967279, 36);
    assert_eq!(s, "0.uh8u81s3fz", "{s}");
}
