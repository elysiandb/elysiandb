use super::context::{Context, is_lower, is_upper, lower, upper};
use super::{Options, UndCaser};
use crate::deps::xtext::language::Tag;
use crate::deps::xtext::norm::transform::Error;

const MAX_IGNORABLE: usize = 30;

pub(super) fn make_upper(_t: Tag, _o: Options) -> UndCaser {
    UndCaser::Upper
}

pub(super) fn make_lower(_t: Tag, o: Options) -> UndCaser {
    if o.ignore_final_sigma {
        return UndCaser::LowerIgnoreSigma;
    }
    UndCaser::Lower
}

pub(super) fn und_upper_transform(
    dst: &mut [u8],
    src: &[u8],
    at_eof: bool,
) -> (usize, usize, Result<(), Error>) {
    let mut c = Context::new(dst, src, at_eof);
    while c.next() {
        upper(&mut c);
        c.checkpoint();
    }
    c.ret()
}

pub(super) fn und_upper_span(src: &[u8], at_eof: bool) -> (usize, Result<(), Error>) {
    let mut c = Context::new(&mut [], src, at_eof);
    while c.next() && is_upper(&mut c) {
        c.checkpoint();
    }
    c.ret_span()
}

pub(super) fn und_lower_ignore_sigma_transform(
    dst: &mut [u8],
    src: &[u8],
    at_eof: bool,
) -> (usize, usize, Result<(), Error>) {
    let mut c = Context::new(dst, src, at_eof);
    while c.next() && lower(&mut c) {
        c.checkpoint();
    }
    c.ret()
}

pub(super) fn und_lower_span(src: &[u8], at_eof: bool) -> (usize, Result<(), Error>) {
    let mut c = Context::new(&mut [], src, at_eof);
    while c.next() && is_lower(&mut c) {
        c.checkpoint();
    }
    c.ret_span()
}

pub(super) fn und_lower_transform(
    dst: &mut [u8],
    src: &[u8],
    at_eof: bool,
) -> (usize, usize, Result<(), Error>) {
    let mut c = Context::new(dst, src, at_eof);
    let mut is_inter_word = true;
    while c.next() {
        if is_inter_word {
            if c.info.is_cased() {
                if !lower(&mut c) {
                    break;
                }
                is_inter_word = false;
            } else if !c.copy() {
                break;
            }
        } else if c.info.is_not_cased_and_not_case_ignorable() {
            if !c.copy() {
                break;
            }
            is_inter_word = true;
        } else if !c.has_prefix("\u{03A3}".as_bytes()) {
            if !lower(&mut c) {
                break;
            }
        } else if !final_sigma_body(&mut c) {
            break;
        }
        c.checkpoint();
    }
    c.ret()
}

pub(super) fn final_sigma_body(c: &mut Context<'_, '_>) -> bool {
    let p = c.p_dst;
    c.write_string("\u{03C2}".as_bytes());
    for _ in 0..MAX_IGNORABLE + 1 {
        if !c.next() {
            return false;
        }
        if !c.info.is_case_ignorable() {
            if c.info.is_cased() {
                c.dst[p + 1] = c.dst[p + 1].wrapping_add(1);
            }
            c.unread_rune();
            return true;
        }
        c.copy();
    }
    true
}
