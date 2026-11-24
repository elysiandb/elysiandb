use super::option::{AlternateHandling, Options};
use super::{Buffer, process_weights};
use crate::deps::xtext::colltab::{Elem, IGNORE, Level, NUM_LEVELS, make_elem, make_quaternary};

const DEFAULT_SECONDARY: i32 = 0x20;
const DEFAULT_TERTIARY: i32 = 0x2;
const MAX_Q: i32 = 0x1FFFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Weights {
    primary: i32,
    secondary: i32,
    tertiary: i32,
    quaternary: i32,
}

fn w(ce: &[i32]) -> Weights {
    let mut w = Weights {
        primary: ce[0],
        secondary: DEFAULT_SECONDARY,
        tertiary: DEFAULT_TERTIARY,
        quaternary: 0,
    };
    if ce.len() > 1 {
        w.secondary = ce[1];
    }
    if ce.len() > 2 {
        w.tertiary = ce[2];
    }
    if ce.len() > 3 {
        w.quaternary = ce[3];
    }
    w
}

fn wpq(p: i32, q: i32) -> Weights {
    w(&[p, DEFAULT_SECONDARY, DEFAULT_TERTIARY, q])
}

fn wsq(s: i32, q: i32) -> Weights {
    w(&[0, s, DEFAULT_TERTIARY, q])
}

fn wq(q: i32) -> Weights {
    w(&[0, 0, 0, q])
}

fn zero() -> Weights {
    w(&[0, 0, 0, 0])
}

fn convert_from_weights(ws: &[Weights]) -> Vec<Elem> {
    ws.iter()
        .map(|w| {
            let mut e = make_elem(w.primary, w.secondary, w.tertiary, 0).unwrap_or_default();
            if e == IGNORE && w.quaternary > 0 {
                e = make_quaternary(w.quaternary);
            }
            e
        })
        .collect()
}

pub(super) fn ignore(level: i32) -> [bool; NUM_LEVELS] {
    let level = if level < 0 {
        Level::Quaternary as i32
    } else {
        level
    };
    let mut ignore = [false; NUM_LEVELS];
    for (i, v) in ignore.iter_mut().enumerate() {
        *v = level < i as i32;
    }
    ignore
}

#[derive(Clone, Copy, Default)]
struct Opts {
    lev: i32,
    alt: AlternateHandling,
    top: u32,
    backwards: bool,
    case_level: bool,
}

impl Opts {
    fn collator(self) -> Options {
        Options {
            ignore: ignore(self.lev - 1),
            alternate: self.alt,
            backwards: self.backwards,
            case_level: self.case_level,
        }
    }
}

fn alt(a: AlternateHandling, top: u32) -> Opts {
    Opts {
        alt: a,
        top,
        ..Opts::default()
    }
}

#[test]
fn test_process_weights() {
    use AlternateHandling::{Blanked, NonIgnorable, ShiftTrimmed, Shifted};
    let mut process_tests: Vec<(Opts, Vec<Weights>, Vec<Weights>)> = Vec::new();
    for a in [Shifted, ShiftTrimmed] {
        process_tests.extend([
            (
                alt(a, 100),
                vec![w(&[200]), w(&[300]), w(&[400])],
                vec![wpq(200, MAX_Q), wpq(300, MAX_Q), wpq(400, MAX_Q)],
            ),
            (
                alt(a, 250),
                vec![w(&[200]), w(&[300]), w(&[400])],
                vec![wq(200), wpq(300, MAX_Q), wpq(400, MAX_Q)],
            ),
            (
                alt(a, 999),
                vec![w(&[1000]), w(&[200]), w(&[300]), w(&[400])],
                vec![wpq(1000, MAX_Q), wq(200), wq(300), wq(400)],
            ),
            (
                alt(a, 999),
                vec![w(&[0, 10]), w(&[1000])],
                vec![wsq(10, MAX_Q), wpq(1000, MAX_Q)],
            ),
            (
                alt(a, 250),
                vec![w(&[200]), w(&[0, 10]), w(&[300]), w(&[0, 15]), w(&[400])],
                vec![
                    wq(200),
                    zero(),
                    wpq(300, MAX_Q),
                    wsq(15, MAX_Q),
                    wpq(400, MAX_Q),
                ],
            ),
            (
                alt(a, 250),
                vec![
                    w(&[200]),
                    w(&[0, 0, 10]),
                    w(&[300]),
                    w(&[0, 0, 15]),
                    w(&[400]),
                ],
                vec![
                    wq(200),
                    zero(),
                    wpq(300, MAX_Q),
                    w(&[0, 0, 15, MAX_Q]),
                    wpq(400, MAX_Q),
                ],
            ),
            (
                alt(a, 250),
                vec![w(&[200]), zero(), w(&[300]), zero(), w(&[400])],
                vec![wq(200), zero(), wpq(300, MAX_Q), zero(), wpq(400, MAX_Q)],
            ),
        ]);
    }
    process_tests.extend([
        (
            alt(Blanked, 100),
            vec![w(&[200]), w(&[300]), w(&[400])],
            vec![w(&[200]), w(&[300]), w(&[400])],
        ),
        (
            alt(Blanked, 250),
            vec![w(&[200]), w(&[300]), w(&[400])],
            vec![zero(), w(&[300]), w(&[400])],
        ),
        (
            alt(Blanked, 999),
            vec![w(&[1000]), w(&[200]), w(&[300]), w(&[400])],
            vec![w(&[1000]), zero(), zero(), zero()],
        ),
        (
            alt(Blanked, 999),
            vec![w(&[0, 10]), w(&[1000])],
            vec![w(&[0, 10]), w(&[1000])],
        ),
        (
            alt(Blanked, 250),
            vec![w(&[200]), w(&[0, 10]), w(&[300]), w(&[0, 15]), w(&[400])],
            vec![zero(), zero(), w(&[300]), w(&[0, 15]), w(&[400])],
        ),
        (
            alt(Blanked, 250),
            vec![
                w(&[200]),
                w(&[0, 0, 10]),
                w(&[300]),
                w(&[0, 0, 15]),
                w(&[400]),
            ],
            vec![zero(), zero(), w(&[300]), w(&[0, 0, 15]), w(&[400])],
        ),
        (
            alt(Blanked, 250),
            vec![w(&[200]), zero(), w(&[300]), zero(), w(&[400])],
            vec![zero(), zero(), w(&[300]), zero(), w(&[400])],
        ),
        (
            alt(NonIgnorable, 999),
            vec![w(&[1000]), w(&[200]), w(&[300]), w(&[400])],
            vec![w(&[1000]), w(&[200]), w(&[300]), w(&[400])],
        ),
        (
            alt(NonIgnorable, 250),
            vec![w(&[200]), w(&[0, 10]), w(&[300]), w(&[0, 15]), w(&[400])],
            vec![w(&[200]), w(&[0, 10]), w(&[300]), w(&[0, 15]), w(&[400])],
        ),
        (
            alt(NonIgnorable, 250),
            vec![
                w(&[200]),
                w(&[0, 0, 10]),
                w(&[300]),
                w(&[0, 0, 15]),
                w(&[400]),
            ],
            vec![
                w(&[200]),
                w(&[0, 0, 10]),
                w(&[300]),
                w(&[0, 0, 15]),
                w(&[400]),
            ],
        ),
        (
            alt(NonIgnorable, 250),
            vec![w(&[200]), zero(), w(&[300]), zero(), w(&[400])],
            vec![w(&[200]), zero(), w(&[300]), zero(), w(&[400])],
        ),
    ]);
    assert_eq!(process_tests.len(), 25);
    for (i, (opt, input, output)) in process_tests.iter().enumerate() {
        let mut inp = convert_from_weights(input);
        let out = convert_from_weights(output);
        process_weights(opt.alt, opt.top, &mut inp);
        for (j, w) in inp.iter().enumerate() {
            assert_eq!(*w, out[j], "{i}: Weights {j}");
        }
    }
}

#[test]
fn test_key_from_elems() {
    use AlternateHandling::{ShiftTrimmed, Shifted};
    let def_s = DEFAULT_SECONDARY as u8;
    let def_t = DEFAULT_TERTIARY as u8;
    let sep = 0u8;
    let shifted = Opts {
        alt: Shifted,
        ..Opts::default()
    };
    let key_from_elem_tests: Vec<(Opts, Vec<Weights>, Vec<u8>)> = vec![
        (
            shifted,
            vec![w(&[0x200]), w(&[0x7FFF]), w(&[0, 0x30]), w(&[0x100])],
            vec![
                0x2, 0, 0x7F, 0xFF, 0x1, 0x00, sep, sep, 0, def_s, 0, def_s, 0, 0x30, 0, def_s,
                sep, sep, def_t, def_t, def_t, def_t, sep, 0xFF, 0xFF, 0xFF, 0xFF,
            ],
        ),
        (
            shifted,
            vec![
                w(&[0x200]),
                zero(),
                w(&[0x7FFF]),
                w(&[0, 0x30]),
                zero(),
                w(&[0x100]),
            ],
            vec![
                0x2, 0, 0x7F, 0xFF, 0x1, 0x00, sep, sep, 0, def_s, 0, def_s, 0, 0x30, 0, def_s,
                sep, sep, def_t, def_t, def_t, def_t, sep, 0xFF, 0xFF, 0xFF, 0xFF,
            ],
        ),
        (
            shifted,
            vec![w(&[0x200]), w(&[0x8000]), w(&[0, 0x30]), w(&[0x12345])],
            vec![
                0x2, 0, 0x80, 0x80, 0x00, 0x81, 0x23, 0x45, sep, sep, 0, def_s, 0, def_s, 0, 0x30,
                0, def_s, sep, sep, def_t, def_t, def_t, def_t, sep, 0xFF, 0xFF, 0xFF, 0xFF,
            ],
        ),
        (
            Opts {
                alt: Shifted,
                backwards: true,
                ..Opts::default()
            },
            vec![w(&[0x200]), w(&[0x7FFF]), w(&[0, 0x30]), w(&[0x100])],
            vec![
                0x2, 0, 0x7F, 0xFF, 0x1, 0x00, sep, sep, 0, def_s, 0, 0x30, 0, def_s, 0, def_s,
                sep, sep, def_t, def_t, def_t, def_t, sep, 0xFF, 0xFF, 0xFF, 0xFF,
            ],
        ),
        (
            Opts {
                alt: Shifted,
                lev: 3,
                ..Opts::default()
            },
            vec![
                w(&[0x200]),
                zero(),
                w(&[0x7FFF]),
                w(&[0, 0x30]),
                zero(),
                w(&[0x100]),
            ],
            vec![
                0x2, 0, 0x7F, 0xFF, 0x1, 0x00, sep, sep, 0, def_s, 0, def_s, 0, 0x30, 0, def_s,
                sep, sep, def_t, def_t, def_t, def_t,
            ],
        ),
        (
            Opts {
                alt: Shifted,
                lev: 2,
                ..Opts::default()
            },
            vec![
                w(&[0x200]),
                zero(),
                w(&[0x7FFF]),
                w(&[0, 0x30]),
                zero(),
                w(&[0x100]),
            ],
            vec![
                0x2, 0, 0x7F, 0xFF, 0x1, 0x00, sep, sep, 0, def_s, 0, def_s, 0, 0x30, 0, def_s,
            ],
        ),
        (
            Opts {
                alt: Shifted,
                lev: 1,
                ..Opts::default()
            },
            vec![
                w(&[0x200]),
                zero(),
                w(&[0x7FFF]),
                w(&[0, 0x30]),
                zero(),
                w(&[0x100]),
            ],
            vec![0x2, 0, 0x7F, 0xFF, 0x1, 0x00],
        ),
        (
            Opts {
                alt: ShiftTrimmed,
                top: 0x250,
                ..Opts::default()
            },
            vec![
                w(&[0x300]),
                w(&[0x200]),
                w(&[0x7FFF]),
                w(&[0, 0x30]),
                w(&[0x800]),
            ],
            vec![
                0x3, 0, 0x7F, 0xFF, 0x8, 0x00, sep, sep, 0, def_s, 0, def_s, 0, 0x30, 0, def_s,
                sep, sep, def_t, def_t, def_t, def_t, sep, 0xFF, 0x2, 0,
            ],
        ),
        (
            Opts {
                alt: Shifted,
                lev: 1,
                case_level: true,
                ..Opts::default()
            },
            vec![w(&[0x200]), w(&[0x7FFF]), w(&[0, 0x30]), w(&[0x100])],
            vec![
                0x2, 0, 0x7F, 0xFF, 0x1, 0x00, sep, sep, sep, sep, def_t, def_t, def_t, def_t,
            ],
        ),
    ];
    let mut buf = Buffer::default();
    for (i, (opt, input, output)) in key_from_elem_tests.iter().enumerate() {
        buf.reset();
        let mut inp = convert_from_weights(input);
        process_weights(opt.alt, opt.top, &mut inp);
        opt.collator().key_from_elems(&mut buf, &inp);
        assert_eq!(buf.key.len(), output.len(), "{i}: len(ws)");
        assert_eq!(&buf.key, output, "{i}");
    }
}
