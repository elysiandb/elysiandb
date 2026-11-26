use std::sync::Arc;

use talos::deps::regexp2::{ECMASCRIPT, Regexp, must_compile};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn compiled_patterns_are_shared_between_threads() {
    assert_send_sync::<Regexp>();
    let re = Arc::new(must_compile(r"(?<=(\d+))x(a|b)*?c\1", ECMASCRIPT));
    let text: Vec<i32> = "12xababc12 7xc7".chars().map(|c| c as i32).collect();
    let text: Arc<[i32]> = text.into();
    let expected: Vec<(isize, isize)> = {
        let mut out = Vec::new();
        let mut m = re.find_runes_match(text.clone()).expect("no error");
        while let Some(cur) = m {
            out.push((cur.index, cur.length));
            m = re.find_next_match(&cur).expect("no error");
        }
        out
    };
    assert_eq!(expected, vec![(2, 8), (12, 3)]);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let re = Arc::clone(&re);
            let text = text.clone();
            std::thread::spawn(move || {
                let mut all = Vec::new();
                for _ in 0..200 {
                    let mut out = Vec::new();
                    let mut m = re.find_runes_match(text.clone()).expect("no error");
                    while let Some(cur) = m {
                        out.push((cur.index, cur.length));
                        m = re.find_next_match(&cur).expect("no error");
                    }
                    all.push(out);
                }
                all
            })
        })
        .collect();
    for h in handles {
        for out in h.join().expect("thread") {
            assert_eq!(out, expected);
        }
    }
}
