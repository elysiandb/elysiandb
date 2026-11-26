use gocompat::unicode::{self, RangeTable};
use gocompat::utf8::{self, Rune};

use super::context::{Context, lower, upper};
use super::tables::UNICODE_VERSION;
use super::tables_test::{BREAK_PROP, CASE_IGNORABLE, SPECIAL};
use super::trieval::{C_LOWER, C_UPPER, CCC_ABOVE, CCC_OTHER, CCC_ZERO};
use crate::deps::xtext::norm::{self, Form, transform::Error};

const LAST_RUNE_FOR_TESTING: Rune = 0x1FFFF;
const ABOVE: u8 = 230;

static ASSIGNED_CATEGORIES: [&RangeTable; 7] = [
    &unicode::C,
    &unicode::L,
    &unicode::M,
    &unicode::N,
    &unicode::P,
    &unicode::S,
    &unicode::Z,
];

fn assigned(r: Rune) -> bool {
    unicode::is_one_of(&ASSIGNED_CATEGORIES, r)
}

fn go_string(r: Rune) -> Vec<u8> {
    let mut b = Vec::new();
    utf8::append_rune(
        &mut b,
        if utf8::valid_rune(r) {
            r
        } else {
            utf8::RUNE_ERROR
        },
    );
    b
}

fn prop_cased(r: Rune) -> bool {
    prop_lower(r) || prop_upper(r) || unicode::is_title(r)
}

fn prop_lower(r: Rune) -> bool {
    unicode::is_lower(r) || unicode::is(&unicode::OTHER_LOWERCASE, r)
}

fn prop_upper(r: Rune) -> bool {
    unicode::is_upper(r) || unicode::is(&unicode::OTHER_UPPERCASE, r)
}

fn prop_ignore(r: Rune) -> bool {
    if unicode::is_one_of(
        &[
            &unicode::MN,
            &unicode::ME,
            &unicode::CF,
            &unicode::LM,
            &unicode::SK,
        ],
        r,
    ) {
        return true;
    }
    CASE_IGNORABLE.contains(&r)
}

fn has_break_prop(r: Rune) -> bool {
    let mut lo = 0;
    let mut hi = BREAK_PROP.len();
    while lo < hi {
        let m = lo + (hi - lo) / 2;
        let bp = BREAK_PROP[m];
        if bp.0 <= r && r <= bp.1 {
            return true;
        }
        if r < bp.0 {
            hi = m;
        } else {
            lo = m + 1;
        }
    }
    false
}

fn apply(r: Rune, f: fn(&mut Context<'_, '_>) -> bool) -> Vec<u8> {
    let mut dst = vec![0u8; 128];
    let src = go_string(r);
    let mut c = Context::new(&mut dst, &src, true);
    c.next();
    f(&mut c);
    let n = c.p_dst;
    dst[..n].to_vec()
}

#[test]
fn test_case_properties() {
    assert_eq!(unicode::VERSION, UNICODE_VERSION);
    let mut tested = 0;
    for r in 0..=LAST_RUNE_FOR_TESTING {
        if !assigned(r) {
            continue;
        }
        let mut dst = vec![0u8; 128];
        let src = go_string(r);
        let mut c = Context::new(&mut dst, &src, true);
        c.next();
        assert_eq!(
            c.info.is_case_ignorable(),
            prop_ignore(r),
            "caseIgnorable(U+{r:04X}) ({:x})",
            c.info.0
        );
        let rf = unicode::simple_fold(r);
        if rf != r && assigned(rf) {
            assert_eq!(
                c.info.is_cased(),
                prop_cased(r),
                "cased(U+{r:04X}) ({:x})",
                c.info.0
            );
            assert_eq!(
                c.case_type() == C_UPPER,
                prop_upper(r),
                "upper(U+{r:04X}) ({:x})",
                c.info.0
            );
            assert_eq!(
                c.case_type() == C_LOWER,
                prop_lower(r),
                "lower(U+{r:04X}) ({:x})",
                c.info.0
            );
        }
        assert_eq!(
            c.info.is_break(),
            has_break_prop(r),
            "isBreak(U+{r:04X}) ({:x})",
            c.info.0
        );
        tested += 1;
    }
    assert!(tested > 0);
}

#[test]
fn test_mapping() {
    for &(r, to_lower, _to_title, to_upper) in SPECIAL {
        assert_eq!(
            apply(r, lower),
            to_lower.as_bytes(),
            "lowerSpecial:(U+{r:04X})"
        );
        assert_eq!(
            apply(r, upper),
            to_upper.as_bytes(),
            "upperSpecial:(U+{r:04X})"
        );
    }
    for r in 0..=LAST_RUNE_FOR_TESTING {
        if !assigned(r) {
            continue;
        }
        let rf = unicode::simple_fold(r);
        if rf == r || !assigned(rf) {
            continue;
        }
        if SPECIAL.iter().any(|s| s.0 == r) {
            continue;
        }
        assert_eq!(
            apply(r, lower),
            go_string(unicode::to_lower(r)),
            "lower:(U+{r:04X})"
        );
        assert_eq!(
            apply(r, upper),
            go_string(unicode::to_upper(r)),
            "upper:(U+{r:04X})"
        );
    }
}

#[test]
fn test_ccc() {
    assert_eq!(norm::VERSION, UNICODE_VERSION);
    for r in 0..=LAST_RUNE_FOR_TESTING {
        if !assigned(r) {
            continue;
        }
        let mut dst = vec![0u8; 128];
        let src = go_string(r);
        let mut c = Context::new(&mut dst, &src, true);
        c.next();
        let p = Form::Nfc.properties_string(&src);
        let want = match p.ccc() {
            0 => CCC_ZERO,
            ABOVE => CCC_ABOVE,
            _ => CCC_OTHER,
        };
        assert_eq!(c.info.ccc_type(), want, "U+{r:04X}");
    }
}

struct ContextCase {
    desc: &'static str,
    dst_size: usize,
    at_eof: bool,
    src: &'static str,
    out: &'static str,
    n_src: usize,
    err: Result<(), Error>,
    ops: &'static str,
    prefix_arg: &'static str,
    prefix_want: bool,
}

#[test]
fn test_context() {
    let tests = [
        ContextCase {
            desc: "next: past end, atEOF, no checkpoint",
            dst_size: 10,
            at_eof: true,
            src: "12",
            out: "",
            n_src: 2,
            err: Ok(()),
            ops: "next;next;next",
            prefix_arg: "x",
            prefix_want: false,
        },
        ContextCase {
            desc: "next: not at end, atEOF, no checkpoint",
            dst_size: 10,
            at_eof: false,
            src: "12",
            out: "",
            n_src: 0,
            err: Err(Error::ShortSrc),
            ops: "next;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "next: past end, !atEOF, no checkpoint",
            dst_size: 10,
            at_eof: false,
            src: "12",
            out: "",
            n_src: 0,
            err: Err(Error::ShortSrc),
            ops: "next;next;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "next: past end, !atEOF, checkpoint",
            dst_size: 10,
            at_eof: false,
            src: "12",
            out: "",
            n_src: 2,
            err: Ok(()),
            ops: "next;next;checkpoint;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "copy: exact count, atEOF, no checkpoint",
            dst_size: 2,
            at_eof: true,
            src: "12",
            out: "12",
            n_src: 2,
            err: Ok(()),
            ops: "next;copy;next;copy;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "copy: past end, !atEOF, no checkpoint",
            dst_size: 2,
            at_eof: false,
            src: "12",
            out: "",
            n_src: 0,
            err: Err(Error::ShortSrc),
            ops: "next;copy;next;copy;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "copy: past end, !atEOF, checkpoint",
            dst_size: 2,
            at_eof: false,
            src: "12",
            out: "12",
            n_src: 2,
            err: Ok(()),
            ops: "next;copy;next;copy;checkpoint;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "copy: short dst",
            dst_size: 1,
            at_eof: false,
            src: "12",
            out: "",
            n_src: 0,
            err: Err(Error::ShortDst),
            ops: "next;copy;next;copy;checkpoint;next",
            prefix_arg: "12",
            prefix_want: false,
        },
        ContextCase {
            desc: "copy: short dst, checkpointed",
            dst_size: 1,
            at_eof: false,
            src: "12",
            out: "1",
            n_src: 1,
            err: Err(Error::ShortDst),
            ops: "next;copy;checkpoint;next;copy;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "writeString: simple",
            dst_size: 3,
            at_eof: true,
            src: "1",
            out: "1ab",
            n_src: 1,
            err: Ok(()),
            ops: "next;copy;writeab;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "writeString: short dst",
            dst_size: 2,
            at_eof: true,
            src: "12",
            out: "",
            n_src: 0,
            err: Err(Error::ShortDst),
            ops: "next;copy;writeab;next",
            prefix_arg: "2",
            prefix_want: true,
        },
        ContextCase {
            desc: "writeString: simple",
            dst_size: 3,
            at_eof: true,
            src: "12",
            out: "1ab",
            n_src: 2,
            err: Ok(()),
            ops: "next;copy;next;writeab;next",
            prefix_arg: "",
            prefix_want: true,
        },
        ContextCase {
            desc: "writeString: short dst",
            dst_size: 2,
            at_eof: true,
            src: "12",
            out: "",
            n_src: 0,
            err: Err(Error::ShortDst),
            ops: "next;copy;next;writeab;next",
            prefix_arg: "1",
            prefix_want: false,
        },
        ContextCase {
            desc: "prefix",
            dst_size: 2,
            at_eof: true,
            src: "12",
            out: "",
            n_src: 0,
            err: Err(Error::ShortSrc),
            ops: "",
            prefix_arg: "12",
            prefix_want: true,
        },
    ];
    for tt in &tests {
        let mut dst = vec![0u8; tt.dst_size];
        let mut c = Context::new(&mut dst, tt.src.as_bytes(), tt.at_eof);
        for op in tt.ops.split(';') {
            match op {
                "next" => {
                    c.next();
                }
                "checkpoint" => c.checkpoint(),
                "writeab" => {
                    c.write_string(b"ab");
                }
                "copy" => {
                    c.copy();
                }
                "" => {}
                _ => panic!("unknown op {op:?}"),
            }
        }
        assert_eq!(
            c.has_prefix(tt.prefix_arg.as_bytes()),
            tt.prefix_want,
            "{}: prefix",
            tt.desc
        );
        let (n_dst, n_src, err) = c.ret();
        assert_eq!(err, tt.err, "{}: error", tt.desc);
        assert_eq!(&dst[..n_dst], tt.out.as_bytes(), "{}: out", tt.desc);
        assert_eq!(n_src, tt.n_src, "{}: nSrc", tt.desc);
    }
}
