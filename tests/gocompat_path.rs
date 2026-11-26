use elysiandb::gocompat::{path, url};

#[test]
fn test_clean() {
    let cases = [
        ("", "."),
        ("abc", "abc"),
        ("abc/def", "abc/def"),
        ("a/b/c", "a/b/c"),
        (".", "."),
        ("..", ".."),
        ("../..", "../.."),
        ("../../abc", "../../abc"),
        ("/abc", "/abc"),
        ("/", "/"),
        ("abc/", "abc"),
        ("abc/def/", "abc/def"),
        ("a/b/c/", "a/b/c"),
        ("./", "."),
        ("../", ".."),
        ("../../", "../.."),
        ("/abc/", "/abc"),
        ("abc//def//ghi", "abc/def/ghi"),
        ("//abc", "/abc"),
        ("///abc", "/abc"),
        ("//abc//", "/abc"),
        ("abc//", "abc"),
        ("abc/./def", "abc/def"),
        ("/./abc/def", "/abc/def"),
        ("abc/.", "abc"),
        ("abc/def/ghi/../jkl", "abc/def/jkl"),
        ("abc/def/../ghi/../jkl", "abc/jkl"),
        ("abc/def/..", "abc"),
        ("abc/def/../..", "."),
        ("/abc/def/../..", "/"),
        ("abc/def/../../..", ".."),
        ("/abc/def/../../..", "/"),
        ("abc/def/../../../ghi/jkl/../../../mno", "../../mno"),
        ("abc/./../def", "def"),
        ("abc//./../def", "def"),
        ("abc/../../././../def", "../../def"),
    ];

    for (input, want) in cases {
        let got = path::clean(input.as_bytes());
        assert_eq!(String::from_utf8_lossy(&got), want, "Clean({input:?})");
        let again = path::clean(want.as_bytes());
        assert_eq!(String::from_utf8_lossy(&again), want, "Clean({want:?})");
    }
}

#[test]
fn test_ext() {
    let cases = [
        ("path.go", ".go"),
        ("path.pb.go", ".go"),
        ("a.dir/b", ""),
        ("a.dir/b.go", ".go"),
        ("a.dir/", ""),
        ("dist/.gitkeep", ".gitkeep"),
    ];

    for (input, want) in cases {
        let got = path::ext(input.as_bytes());
        assert_eq!(String::from_utf8_lossy(got), want, "Ext({input:?})");
    }
}

#[test]
fn test_path_unescape() {
    let ok = [
        ("", ""),
        ("abc", "abc"),
        ("1%41", "1A"),
        ("1%41%42%43", "1ABC"),
        ("%4a", "J"),
        ("%6F", "o"),
        ("a+b", "a+b"),
        ("a%20b", "a b"),
        ("a%2Fb%20c", "a/b c"),
    ];
    for (input, want) in ok {
        match url::path_unescape(input.as_bytes()) {
            Ok(got) => assert_eq!(
                String::from_utf8_lossy(&got),
                want,
                "PathUnescape({input:?})"
            ),
            Err(e) => panic!("PathUnescape({input:?}) failed: {e}"),
        }
    }

    for input in ["%", "%a", "%1", "123%45%6", "%zzzzz", "%zz"] {
        assert!(
            url::path_unescape(input.as_bytes()).is_err(),
            "PathUnescape({input:?}) must fail"
        );
    }

    match url::path_unescape(b"%FF") {
        Ok(got) => assert_eq!(got, vec![0xff]),
        Err(e) => panic!("PathUnescape(%FF) failed: {e}"),
    }
}
