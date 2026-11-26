mod rand;
mod subtle;

use elysiandb::crypto::bcrypt::{
    self, DEFAULT_COST, Error, MIN_COST, base64_decode, base64_encode, compare_hash_and_password,
    generate_from_password, new_from_hash, new_from_password,
};

#[test]
fn bcrypting_is_easy() {
    let pass = b"mypassword";
    let hp = match generate_from_password(pass, 0) {
        Ok(hp) => hp,
        Err(err) => panic!("generate_from_password error: {err}"),
    };

    assert!(
        compare_hash_and_password(&hp, pass).is_ok(),
        "{hp:?} should hash {pass:?} correctly"
    );

    let not_pass = b"notthepass";
    assert_eq!(
        compare_hash_and_password(&hp, not_pass),
        Err(Error::MismatchedHashAndPassword),
        "{hp:?} and {not_pass:?} should be mismatched"
    );
}

#[test]
fn bcrypting_is_correct() {
    let pass = b"allmine";
    let salt = b"XajjQvNhvvRt5GSeFk1xFe";
    let expected_hash = b"$2a$10$XajjQvNhvvRt5GSeFk1xFeyqRrsxkhBkUiQeg0dt.wU1qD4aFDcga";

    let hash = match bcrypt::bcrypt(pass, 10, salt) {
        Ok(hash) => hash,
        Err(err) => panic!("bcrypt blew up: {err}"),
    };
    assert!(
        expected_hash.ends_with(&hash),
        "{hash:?} should be the suffix of {expected_hash:?}"
    );

    let h = new_from_hash(expected_hash);
    match h {
        Ok(h) => assert_eq!(expected_hash.to_vec(), h.hash()),
        Err(err) => panic!("unable to parse the hash: {err}"),
    }
}

#[test]
fn very_short_passwords() {
    let key = b"k";
    let salt = b"XajjQvNhvvRt5GSeFk1xFe";
    if let Err(err) = bcrypt::bcrypt(key, 10, salt) {
        panic!("one byte key resulted in error: {err}");
    }
}

#[test]
fn too_long_passwords_work() {
    let salt = b"XajjQvNhvvRt5GSeFk1xFe";
    let too_long_pass = b"012345678901234567890123456789012345678901234567890123456";
    let too_long_expected = b"$2a$10$XajjQvNhvvRt5GSeFk1xFe5l47dONXg781AmZtd869sO8zfsHuw7C";
    let hash = match bcrypt::bcrypt(too_long_pass, 10, salt) {
        Ok(hash) => hash,
        Err(err) => panic!("bcrypt blew up on long password: {err}"),
    };
    assert!(
        too_long_expected.ends_with(&hash),
        "{hash:?} should be the suffix of {too_long_expected:?}"
    );
}

#[test]
fn invalid_hash_errors() {
    let invalid_tests: [(Error, &[u8]); 5] = [
        (Error::HashTooShort, b"$2a$10$fooo"),
        (Error::HashTooShort, b"$2a"),
        (
            Error::HashVersionTooNew(b'3'),
            b"$3a$10$sssssssssssssssssssssshhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh",
        ),
        (
            Error::InvalidHashPrefix(b'%'),
            b"%2a$10$sssssssssssssssssssssshhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh",
        ),
        (
            Error::InvalidCost(32),
            b"$2a$32$sssssssssssssssssssssshhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh",
        ),
    ];
    for (expected, hash) in &invalid_tests {
        assert_eq!(
            new_from_hash(hash).err().as_ref(),
            Some(expected),
            "new_from_hash"
        );
        assert_eq!(
            compare_hash_and_password(hash, b"anything").err().as_ref(),
            Some(expected),
            "compare_hash_and_password"
        );
    }
}

#[test]
fn unpadded_base64_encoding() {
    let original = [
        101u8, 201, 101, 75, 19, 227, 199, 20, 239, 236, 133, 32, 30, 109, 243, 30,
    ];
    let encoded_original = b"XajjQvNhvvRt5GSeFk1xFe";

    let encoded = base64_encode(&original);

    assert_eq!(encoded_original.to_vec(), encoded);

    let decoded = match base64_decode(encoded_original) {
        Ok(decoded) => decoded,
        Err(err) => panic!("base64_decode blew up: {err}"),
    };

    assert_eq!(decoded, original.to_vec());
}

#[test]
fn cost() {
    let suffix = "XajjQvNhvvRt5GSeFk1xFe5l47dONXg781AmZtd869sO8zfsHuw7C";
    for vers in ["2a", "2"] {
        for cost in [4i64, 10] {
            let s = format!("${vers}${cost:02}${suffix}");
            match bcrypt::cost(s.as_bytes()) {
                Ok(actual) => assert_eq!(actual, cost),
                Err(err) => panic!("cost, error: {err}"),
            }
        }
    }
    assert!(
        bcrypt::cost(format!("$a$a${suffix}").as_bytes()).is_err(),
        "cost, malformed but no error returned"
    );
}

#[test]
fn cost_validation_in_hash() {
    let pass = b"mypassword";

    for c in 0..MIN_COST {
        let p = new_from_password(pass, c).unwrap();
        assert_eq!(
            p.cost, DEFAULT_COST,
            "new_from_password should default costs below {MIN_COST} to {DEFAULT_COST}"
        );
    }

    let p = new_from_password(pass, 14).unwrap();
    assert_eq!(p.cost, 14);

    let hp = new_from_hash(&p.hash()).unwrap();
    assert_eq!(p.cost, hp.cost);

    match new_from_password(pass, 32) {
        Ok(_) => panic!("new_from_password: should return a cost error"),
        Err(err) => assert_eq!(err, Error::InvalidCost(32)),
    }
}

#[test]
fn cost_returns_with_leading_zeroes() {
    let hp = new_from_password(b"abcdefgh", 7).unwrap();
    let hash = hp.hash();
    assert_eq!(&hash[4..7], b"07$");
}

#[test]
fn minor_not_required() {
    let no_minor_hash = b"$2$10$XajjQvNhvvRt5GSeFk1xFeyqRrsxkhBkUiQeg0dt.wU1qD4aFDcga";
    let h = match new_from_hash(no_minor_hash) {
        Ok(h) => h,
        Err(err) => panic!("no minor hash blew up: {err}"),
    };
    assert_eq!(h.minor, 0);
    assert_eq!(no_minor_hash.to_vec(), h.hash());
}

#[test]
fn no_side_effects_from_compare() {
    let source = b"passw0rd123456".to_vec();
    let password = &source[..source.len() - 6];
    let token = &source[source.len() - 6..];
    let want = source.clone();

    let want_hash = b"$2a$10$LK9XRuhNxHHCvjX3tdkRKei1QiCDUKrJRhZv7WWZPuQGRUM92rOUa";
    let _ = compare_hash_and_password(want_hash, password);

    let got = [password, token].concat();
    assert_eq!(got, want);
}

#[test]
fn password_too_long() {
    assert_eq!(
        generate_from_password(&[0u8; 73], 1),
        Err(Error::PasswordTooLong)
    );
    assert!(generate_from_password(&[0u8; 72], 4).is_ok());
}

#[test]
fn long_passwords_compare_on_their_first_72_bytes() {
    let mut long = vec![b'p'; 72];
    let hash = generate_from_password(&long, 4).unwrap();
    long.extend_from_slice(b"ignored");
    assert_eq!(compare_hash_and_password(&hash, &long), Ok(()));
    long[71] = b'q';
    assert_eq!(
        compare_hash_and_password(&hash, &long),
        Err(Error::MismatchedHashAndPassword)
    );
}

#[test]
fn generated_hashes_use_version_2a_and_the_requested_cost() {
    let hash = generate_from_password(b"secret", DEFAULT_COST).unwrap();
    assert_eq!(hash.len(), 60);
    assert_eq!(&hash[..7], b"$2a$10$");
    let other = generate_from_password(b"secret", DEFAULT_COST).unwrap();
    assert_ne!(hash, other);
    assert_eq!(bcrypt::cost(&hash), Ok(10));
}

#[test]
fn cost_digits_follow_the_integer_parser() {
    let suffix = "XajjQvNhvvRt5GSeFk1xFe5l47dONXg781AmZtd869sO8zfsHuw7C";
    assert_eq!(bcrypt::cost(format!("$2a$+5${suffix}").as_bytes()), Ok(5));
    assert_eq!(
        bcrypt::cost(format!("$2a$-5${suffix}").as_bytes()),
        Err(Error::InvalidCost(-5))
    );
    assert_eq!(bcrypt::cost(format!("$2a$10X{suffix}").as_bytes()), Ok(10));
    let err = bcrypt::cost(format!("$2a$ 5${suffix}").as_bytes()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid bcrypt cost: \" 5\" is not a valid integer"
    );
}

#[test]
fn error_messages_carry_no_go_wording() {
    let cases = [
        (
            Error::MismatchedHashAndPassword,
            "the password does not match the bcrypt hash",
        ),
        (Error::HashTooShort, "the bcrypt hash is too short"),
        (
            Error::HashVersionTooNew(b'3'),
            "bcrypt version '3' is newer than the supported version '2'",
        ),
        (
            Error::InvalidHashPrefix(b'%'),
            "a bcrypt hash must start with '$', not '%'",
        ),
        (
            Error::InvalidCost(32),
            "bcrypt cost 32 is outside the range 4 to 31",
        ),
        (
            Error::PasswordTooLong,
            "the password is longer than 72 bytes",
        ),
    ];
    for (err, want) in cases {
        assert_eq!(err.to_string(), want);
    }
    let salt = compare_hash_and_password(
        b"$2a$10$!ajjQvNhvvRt5GSeFk1xFeyqRrsxkhBkUiQeg0dt.wU1qD4aFDcga",
        b"x",
    )
    .unwrap_err();
    assert_eq!(
        salt.to_string(),
        "invalid bcrypt salt: invalid base64 at byte 0"
    );
    let random = Error::Random(std::io::Error::from_raw_os_error(5));
    assert_eq!(
        random.to_string(),
        "cannot read random bytes: Input/output error (os error 5)"
    );
}
