use std::cell::RefCell;
use std::sync::Arc;

use crate::gocompat::math::{frexp, ldexp};

use super::view::Strs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub(crate) enum VKind {
    #[default]
    Undef = 0,
    Null,
    Bool,
    Num,
    Str,
    Tmp,
    Arr,
    Opaque,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Val {
    pub(crate) n: f64,
    pub(crate) id: u32,
    pub(crate) k: VKind,
}

pub(crate) const UNDEF_VAL: Val = Val {
    n: 0.0,
    id: 0,
    k: VKind::Undef,
};

pub(crate) const NULL_VAL: Val = Val {
    n: 0.0,
    id: 0,
    k: VKind::Null,
};

pub(crate) fn num_val(n: f64) -> Val {
    Val {
        k: VKind::Num,
        n,
        id: 0,
    }
}

pub(crate) fn bool_val(b: bool) -> Val {
    if b {
        return Val {
            k: VKind::Bool,
            n: 1.0,
            id: 0,
        };
    }

    Val {
        k: VKind::Bool,
        n: 0.0,
        id: 0,
    }
}

#[derive(Default)]
pub(crate) struct Frame {
    pub(crate) regs: Vec<Val>,
    pub(crate) local: Vec<Val>,
    pub(crate) lits: Vec<Val>,
    pub(crate) free: Vec<Val>,
    pub(crate) tmp: Vec<Arc<str>>,
    pub(crate) strs: Option<Arc<Strs>>,
    pub(crate) ret: Val,
    pub(crate) bail: bool,
    pub(crate) touched: bool,
}

pub(crate) fn is_string(v: Val) -> bool {
    v.k == VKind::Str || v.k == VKind::Tmp
}

pub(crate) fn ascii_only(s: &str) -> bool {
    s.bytes().all(|b| b < 0x80)
}

impl Frame {
    pub(crate) fn str(&self, v: Val) -> &str {
        if v.k == VKind::Str {
            return self.strs.as_ref().expect("intern table").get(v.id as usize);
        }

        &self.tmp[v.id as usize]
    }

    fn push_tmp(&mut self, s: &str) -> u32 {
        self.tmp.push(Arc::from(s));
        (self.tmp.len() - 1) as u32
    }

    pub(crate) fn truthy(&mut self, v: Val) -> bool {
        match v.k {
            VKind::Undef | VKind::Null => false,
            VKind::Bool => v.n != 0.0,
            VKind::Num => v.n != 0.0 && !v.n.is_nan(),
            VKind::Str => v.id != 0,
            VKind::Tmp => !self.tmp[v.id as usize].is_empty(),
            VKind::Arr => true,
            VKind::Opaque => {
                self.bail = true;
                false
            }
        }
    }

    pub(crate) fn strict_eq(&mut self, a: Val, b: Val) -> bool {
        if a.k == b.k {
            match a.k {
                VKind::Num | VKind::Bool => return a.n == b.n,
                VKind::Str => return a.id == b.id,
                VKind::Tmp => return self.tmp[a.id as usize] == self.tmp[b.id as usize],
                VKind::Undef | VKind::Null => return true,
                _ => {}
            }

            self.bail = true;

            return false;
        }

        if is_string(a) && is_string(b) {
            return self.str(a) == self.str(b);
        }

        if a.k == VKind::Arr || b.k == VKind::Arr || a.k == VKind::Opaque || b.k == VKind::Opaque {
            self.bail = true;
        }

        false
    }

    pub(crate) fn loose_eq(&mut self, a: Val, b: Val) -> bool {
        let a_null = a.k == VKind::Undef || a.k == VKind::Null;
        let b_null = b.k == VKind::Undef || b.k == VKind::Null;
        if a_null || b_null {
            if a.k == VKind::Arr
                || b.k == VKind::Arr
                || a.k == VKind::Opaque
                || b.k == VKind::Opaque
            {
                self.bail = a.k == VKind::Opaque || b.k == VKind::Opaque;
                return false;
            }
            return a_null && b_null;
        }

        if a.k == b.k || (is_string(a) && is_string(b)) {
            return self.strict_eq(a, b);
        }

        self.bail = true;

        false
    }

    pub(crate) fn type_name(&mut self, v: Val) -> Val {
        let name = match v.k {
            VKind::Undef => "undefined",
            VKind::Bool => "boolean",
            VKind::Num => "number",
            VKind::Str | VKind::Tmp => "string",
            VKind::Null | VKind::Arr => "object",
            VKind::Opaque => {
                self.bail = true;
                return UNDEF_VAL;
            }
        };

        let id = self.push_tmp(name);

        Val {
            k: VKind::Tmp,
            id,
            n: 0.0,
        }
    }

    pub(crate) fn split(&mut self, v: Val, sep: &str) -> Val {
        if !is_string(v) {
            self.bail = true;
            return UNDEF_VAL;
        }

        let s: Arc<str> = Arc::from(self.str(v));
        let start = self.tmp.len();
        let mut rest: &str = &s;
        while let Some(i) = rest.find(sep) {
            let part = &rest[..i];
            self.push_tmp(part);
            rest = &rest[i + sep.len()..];
        }
        self.push_tmp(rest);

        Val {
            k: VKind::Arr,
            id: start as u32,
            n: (self.tmp.len() - start) as f64,
        }
    }

    pub(crate) fn element(&mut self, a: Val, i: i64) -> Val {
        if a.k != VKind::Arr {
            self.bail = true;
            return UNDEF_VAL;
        }

        if i < 0 || i >= a.n as i64 {
            return UNDEF_VAL;
        }

        Val {
            k: VKind::Tmp,
            id: a.id + i as u32,
            n: 0.0,
        }
    }

    pub(crate) fn parse_int10(&mut self, v: Val) -> Val {
        if !is_string(v) {
            self.bail = true;
            return UNDEF_VAL;
        }

        let s = self.str(v).as_bytes();
        let mut n = 0.0;
        let mut digits = 0;
        while digits < s.len() && s[digits].is_ascii_digit() {
            n = n * 10.0 + (s[digits] - b'0') as f64;
            digits += 1;
        }

        if digits == 0 || digits > MAX_PARSED_DIGITS {
            self.bail = true;
            return UNDEF_VAL;
        }

        num_val(n)
    }

    pub(crate) fn length(&mut self, v: Val) -> Val {
        if v.k == VKind::Arr {
            return num_val(v.n);
        }
        if is_string(v) {
            let s = self.str(v);
            if ascii_only(s) {
                return num_val(s.len() as f64);
            }
        }

        self.bail = true;

        UNDEF_VAL
    }

    pub(crate) fn arith(&mut self, op: u8, a: Val, b: Val) -> Val {
        if a.k != VKind::Num || b.k != VKind::Num {
            self.bail = true;
            return UNDEF_VAL;
        }

        match op {
            b'+' => num_val(a.n + b.n),
            b'-' => num_val(a.n - b.n),
            b'*' => num_val(a.n * b.n),
            b'/' => num_val(a.n / b.n),
            _ => num_val(go_mod(a.n, b.n)),
        }
    }

    pub(crate) fn less(&mut self, a: Val, b: Val, strict: bool, swap: bool) -> bool {
        if a.k == VKind::Num && b.k == VKind::Num {
            let (mut x, mut y) = (a.n, b.n);
            if swap {
                std::mem::swap(&mut x, &mut y);
            }
            if strict {
                return x < y;
            }
            return x <= y;
        }

        if is_string(a) && is_string(b) {
            let (x, y) = (self.str(a), self.str(b));
            if !ascii_only(x) || !ascii_only(y) {
                self.bail = true;
                return false;
            }
            let (x, y) = if swap { (y, x) } else { (x, y) };
            if strict {
                return x.as_bytes() < y.as_bytes();
            }
            return x.as_bytes() <= y.as_bytes();
        }

        self.bail = true;

        false
    }
}

const MAX_PARSED_DIGITS: usize = 15;

fn go_mod(x: f64, y: f64) -> f64 {
    if y == 0.0 || x.is_infinite() || x.is_nan() || y.is_nan() {
        return f64::NAN;
    }

    let y = y.abs();
    let (yfr, yexp) = frexp(y);
    let mut r = if x < 0.0 { -x } else { x };
    while r >= y {
        let (rfr, mut rexp) = frexp(r);
        if rfr < yfr {
            rexp -= 1;
        }
        r -= ldexp(y, rexp - yexp);
    }

    if x < 0.0 {
        r = -r;
    }

    r
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ctl {
    Next,
    Continue,
    Return,
    Bail,
}

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Const(Val),
    Lit(usize),
    Local(usize),
    Reg(usize),
    Free(usize),
    Row(usize),
    Length(Box<Expr>),
    Element(Box<Expr>, i64),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Plus(Box<Expr>),
    TypeOf(Box<Expr>),
    Arith(u8, Box<Expr>, Box<Expr>),
    StrictEq(bool, Box<Expr>, Box<Expr>),
    LooseEq(bool, Box<Expr>, Box<Expr>),
    Less(bool, bool, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    ParseInt(Box<Expr>),
    Split(Box<Expr>, String),
    Helper(Vec<(usize, Expr)>, Box<Stmt>),
}

#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    Nop,
    List(Vec<Stmt>),
    Continue,
    Return(Option<Expr>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    SetLocal(usize, Expr),
    UpdateReg(usize, f64),
    UpdateLocal(usize, f64),
    AssignReg(usize, u8, Expr),
    AssignLocal(usize, u8, Expr),
}

impl Expr {
    pub(crate) fn eval(&self, fr: &mut Frame, row: &[Val]) -> Val {
        match self {
            Expr::Const(v) => *v,
            Expr::Lit(i) => fr.lits[*i],
            Expr::Local(i) => fr.local[*i],
            Expr::Reg(i) => fr.regs[*i],
            Expr::Free(i) => fr.free[*i],
            Expr::Row(col) => {
                let v = row[*col];
                if v.k == VKind::Opaque {
                    fr.bail = true;
                }
                v
            }
            Expr::Length(e) => {
                let v = e.eval(fr, row);
                fr.length(v)
            }
            Expr::Element(e, idx) => {
                let v = e.eval(fr, row);
                fr.element(v, *idx)
            }
            Expr::Not(e) => {
                let v = e.eval(fr, row);
                bool_val(!fr.truthy(v))
            }
            Expr::Neg(e) => {
                let v = e.eval(fr, row);
                if v.k != VKind::Num {
                    fr.bail = true;
                    return UNDEF_VAL;
                }
                num_val(-v.n)
            }
            Expr::Plus(e) => {
                let v = e.eval(fr, row);
                if v.k != VKind::Num {
                    fr.bail = true;
                    return UNDEF_VAL;
                }
                v
            }
            Expr::TypeOf(e) => {
                let v = e.eval(fr, row);
                fr.type_name(v)
            }
            Expr::Arith(op, l, r) => {
                let a = l.eval(fr, row);
                let b = r.eval(fr, row);
                fr.arith(*op, a, b)
            }
            Expr::StrictEq(neg, l, r) => {
                let a = l.eval(fr, row);
                let b = r.eval(fr, row);
                bool_val(fr.strict_eq(a, b) != *neg)
            }
            Expr::LooseEq(neg, l, r) => {
                let a = l.eval(fr, row);
                let b = r.eval(fr, row);
                bool_val(fr.loose_eq(a, b) != *neg)
            }
            Expr::Less(strict, swap, l, r) => {
                let a = l.eval(fr, row);
                let b = r.eval(fr, row);
                bool_val(fr.less(a, b, *strict, *swap))
            }
            Expr::And(l, r) => {
                let v = l.eval(fr, row);
                if !fr.truthy(v) {
                    return v;
                }
                r.eval(fr, row)
            }
            Expr::Or(l, r) => {
                let v = l.eval(fr, row);
                if fr.truthy(v) {
                    return v;
                }
                r.eval(fr, row)
            }
            Expr::Cond(t, c, a) => {
                let v = t.eval(fr, row);
                if fr.truthy(v) {
                    return c.eval(fr, row);
                }
                a.eval(fr, row)
            }
            Expr::ParseInt(e) => {
                let v = e.eval(fr, row);
                fr.parse_int10(v)
            }
            Expr::Split(e, sep) => {
                let v = e.eval(fr, row);
                fr.split(v, sep)
            }
            Expr::Helper(args, body) => {
                for (slot, e) in args {
                    let v = e.eval(fr, row);
                    fr.local[*slot] = v;
                }
                if body.run(fr, row) == Ctl::Return {
                    return fr.ret;
                }
                UNDEF_VAL
            }
        }
    }
}

impl Stmt {
    pub(crate) fn run(&self, fr: &mut Frame, row: &[Val]) -> Ctl {
        match self {
            Stmt::Nop => Ctl::Next,
            Stmt::List(list) => {
                for s in list {
                    let r = s.run(fr, row);
                    if r != Ctl::Next {
                        return r;
                    }
                }
                Ctl::Next
            }
            Stmt::Continue => Ctl::Continue,
            Stmt::Return(e) => {
                fr.ret = match e {
                    Some(e) => e.eval(fr, row),
                    None => UNDEF_VAL,
                };
                Ctl::Return
            }
            Stmt::If(test, cons, alt) => {
                let v = test.eval(fr, row);
                if fr.truthy(v) {
                    return cons.run(fr, row);
                }
                match alt {
                    Some(a) => a.run(fr, row),
                    None => Ctl::Next,
                }
            }
            Stmt::SetLocal(slot, e) => {
                let v = e.eval(fr, row);
                fr.local[*slot] = v;
                Ctl::Next
            }
            Stmt::UpdateReg(i, delta) => {
                let r = &mut fr.regs[*i];
                if r.k != VKind::Num {
                    return Ctl::Bail;
                }
                r.n += delta;
                fr.touched = true;
                Ctl::Next
            }
            Stmt::UpdateLocal(slot, delta) => {
                let r = &mut fr.local[*slot];
                if r.k != VKind::Num {
                    return Ctl::Bail;
                }
                r.n += delta;
                Ctl::Next
            }
            Stmt::AssignReg(i, op, right) => {
                let mut v = right.eval(fr, row);
                if fr.bail {
                    return Ctl::Bail;
                }
                if *op != 0 {
                    let cur = fr.regs[*i];
                    v = fr.arith(*op, cur, v);
                    if fr.bail {
                        return Ctl::Bail;
                    }
                } else if v.k == VKind::Tmp || v.k == VKind::Arr {
                    return Ctl::Bail;
                }
                fr.regs[*i] = v;
                fr.touched = true;
                Ctl::Next
            }
            Stmt::AssignLocal(slot, op, right) => {
                let mut v = right.eval(fr, row);
                if *op != 0 {
                    let cur = fr.local[*slot];
                    v = fr.arith(*op, cur, v);
                }
                fr.local[*slot] = v;
                Ctl::Next
            }
        }
    }
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

fn sized_vals(s: &mut Vec<Val>, n: usize) {
    s.clear();
    s.resize(n, UNDEF_VAL);
}

pub(crate) fn acquire_frame(
    regs: usize,
    locals: usize,
    lits: usize,
    free: usize,
    strs: Arc<Strs>,
) -> Frame {
    let mut fr = FRAMES.with(|f| f.borrow_mut().pop()).unwrap_or_default();
    sized_vals(&mut fr.regs, regs);
    sized_vals(&mut fr.local, locals);
    sized_vals(&mut fr.lits, lits);
    sized_vals(&mut fr.free, free);
    fr.tmp.clear();
    fr.strs = Some(strs);
    fr.bail = false;
    fr.touched = false;
    fr.ret = UNDEF_VAL;

    fr
}

pub(crate) fn release_frame(mut fr: Frame) {
    fr.tmp.clear();
    fr.strs = None;
    FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        if f.len() < 64 {
            f.push(fr);
        }
    });
}
