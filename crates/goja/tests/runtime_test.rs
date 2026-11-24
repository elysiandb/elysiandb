use goja::ipow::ipow;

#[test]
fn test_i_pow() {
    assert_eq!(ipow(-9223372036854775808, 1), -9223372036854775808);
    assert_eq!(ipow(9223372036854775807, 1), 9223372036854775807);
    assert_eq!(ipow(-9223372036854775807, 1), -9223372036854775807);
    assert_eq!(ipow(9223372036854775807, 0), 1);
    assert_eq!(ipow(-9223372036854775807, 0), 1);
    assert_eq!(ipow(-9223372036854775808, 0), 1);
}

#[test]
fn test_await_in_parameters() {
    let res = goja::compile(
        "",
        b"
	async function g() {
	    async function inner(a = 1 + await 1) {
	    }
	}
	",
        false,
    );
    assert!(res.is_err(), "Expected error");
}
