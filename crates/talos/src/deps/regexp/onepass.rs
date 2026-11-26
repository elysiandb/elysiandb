use super::syntax::{
    EMPTY_BEGIN_TEXT, EMPTY_END_TEXT, EmptyOp, FOLD_CASE, Flags, Inst, InstOp, Prog,
};
use gocompat::unicode;
use gocompat::utf8::{self, Rune};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnePassProg {
    pub inst: Vec<OnePassInst>,
    pub start: usize,
    pub num_cap: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnePassInst {
    pub inst: Inst,
    pub next: Vec<u32>,
}

pub fn one_pass_prefix(p: &Prog) -> (Vec<u8>, bool, u32) {
    let i = &p.inst[p.start];
    if i.op != InstOp::EmptyWidth || (i.arg as EmptyOp) & EMPTY_BEGIN_TEXT == 0 {
        return (Vec::new(), i.op == InstOp::Match, p.start as u32);
    }
    let mut pc = i.out;
    let mut i = &p.inst[pc as usize];
    while i.op == InstOp::Nop {
        pc = i.out;
        i = &p.inst[pc as usize];
    }
    if iop(i) != InstOp::Rune || i.rune.len() != 1 {
        return (Vec::new(), i.op == InstOp::Match, p.start as u32);
    }
    let mut buf = Vec::new();
    while iop(i) == InstOp::Rune
        && i.rune.len() == 1
        && (i.arg as Flags) & FOLD_CASE == 0
        && i.rune[0] != utf8::RUNE_ERROR
    {
        utf8::append_rune(&mut buf, i.rune[0]);
        pc = i.out;
        i = &p.inst[i.out as usize];
    }
    let complete = i.op == InstOp::EmptyWidth
        && (i.arg as EmptyOp) & EMPTY_END_TEXT != 0
        && p.inst[i.out as usize].op == InstOp::Match;
    (buf, complete, pc)
}

pub fn one_pass_next(i: &OnePassInst, r: Rune) -> u32 {
    if let Some(next) = i.inst.match_rune_pos(r) {
        return i.next[next];
    }
    if i.inst.op == InstOp::AltMatch {
        return i.inst.out;
    }
    0
}

fn iop(i: &Inst) -> InstOp {
    match i.op {
        InstOp::Rune1 | InstOp::RuneAny | InstOp::RuneAnyNotNL => InstOp::Rune,
        op => op,
    }
}

struct QueueOnePass {
    sparse: Vec<u32>,
    dense: Vec<u32>,
    size: u32,
    next_index: u32,
}

impl QueueOnePass {
    fn new(size: usize) -> QueueOnePass {
        QueueOnePass {
            sparse: vec![0; size],
            dense: vec![0; size],
            size: 0,
            next_index: 0,
        }
    }

    fn empty(&self) -> bool {
        self.next_index >= self.size
    }

    fn next(&mut self) -> u32 {
        let n = self.dense[self.next_index as usize];
        self.next_index += 1;
        n
    }

    fn clear(&mut self) {
        self.size = 0;
        self.next_index = 0;
    }

    fn contains(&self, u: u32) -> bool {
        if u as usize >= self.sparse.len() {
            return false;
        }
        let s = self.sparse[u as usize];
        s < self.size && self.dense[s as usize] == u
    }

    fn insert(&mut self, u: u32) {
        if !self.contains(u) {
            self.insert_new(u);
        }
    }

    fn insert_new(&mut self, u: u32) {
        if u as usize >= self.sparse.len() {
            return;
        }
        self.sparse[u as usize] = self.size;
        self.dense[self.size as usize] = u;
        self.size += 1;
    }
}

pub const MERGE_FAILED: u32 = 0xffffffff;

const ANY_RUNE_NOT_NL: [Rune; 4] = [0, '\n' as Rune - 1, '\n' as Rune + 1, unicode::MAX_RUNE];
const ANY_RUNE: [Rune; 2] = [0, unicode::MAX_RUNE];

fn extend(
    merged: &mut Vec<Rune>,
    next: &mut Vec<u32>,
    ix: &mut isize,
    new_low: &mut usize,
    new_array: &[Rune],
    pc: u32,
) -> bool {
    if *ix > 0 && new_array[*new_low] <= merged[*ix as usize] {
        return false;
    }
    merged.push(new_array[*new_low]);
    merged.push(new_array[*new_low + 1]);
    *new_low += 2;
    *ix += 2;
    next.push(pc);
    true
}

pub fn merge_rune_sets(
    left_runes: &[Rune],
    right_runes: &[Rune],
    left_pc: u32,
    right_pc: u32,
) -> (Vec<Rune>, Vec<u32>) {
    let left_len = left_runes.len();
    let right_len = right_runes.len();
    if left_len & 1 != 0 || right_len & 1 != 0 {
        panic!("odd length rune list in one-pass merge");
    }
    let mut lx = 0;
    let mut rx = 0;
    let mut merged = Vec::new();
    let mut next = Vec::new();
    let mut ix: isize = -1;
    while lx < left_len || rx < right_len {
        let ok = if rx >= right_len {
            extend(
                &mut merged,
                &mut next,
                &mut ix,
                &mut lx,
                left_runes,
                left_pc,
            )
        } else if lx >= left_len || right_runes[rx] < left_runes[lx] {
            extend(
                &mut merged,
                &mut next,
                &mut ix,
                &mut rx,
                right_runes,
                right_pc,
            )
        } else {
            extend(
                &mut merged,
                &mut next,
                &mut ix,
                &mut lx,
                left_runes,
                left_pc,
            )
        };
        if !ok {
            return (Vec::new(), vec![MERGE_FAILED]);
        }
    }
    (merged, next)
}

fn cleanup_one_pass(prog: &mut OnePassProg, original: &Prog) {
    for (ix, inst_original) in original.inst.iter().enumerate() {
        match inst_original.op {
            InstOp::Alt | InstOp::AltMatch | InstOp::Rune => {}
            InstOp::Capture | InstOp::EmptyWidth | InstOp::Nop | InstOp::Match | InstOp::Fail => {
                prog.inst[ix].next = Vec::new();
            }
            InstOp::Rune1 | InstOp::RuneAny | InstOp::RuneAnyNotNL => {
                prog.inst[ix] = OnePassInst {
                    inst: inst_original.clone(),
                    next: Vec::new(),
                };
            }
        }
    }
}

const OUT: bool = false;
const ARG: bool = true;

fn get_field(p: &OnePassProg, (idx, field): (usize, bool)) -> u32 {
    if field == ARG {
        p.inst[idx].inst.arg
    } else {
        p.inst[idx].inst.out
    }
}

fn set_field(p: &mut OnePassProg, (idx, field): (usize, bool), v: u32) {
    if field == ARG {
        p.inst[idx].inst.arg = v;
    } else {
        p.inst[idx].inst.out = v;
    }
}

fn is_alt(op: InstOp) -> bool {
    op == InstOp::Alt || op == InstOp::AltMatch
}

fn one_pass_copy(prog: &Prog) -> OnePassProg {
    let mut p = OnePassProg {
        start: prog.start,
        num_cap: prog.num_cap,
        inst: prog
            .inst
            .iter()
            .map(|i| OnePassInst {
                inst: i.clone(),
                next: Vec::new(),
            })
            .collect(),
    };
    for pc in 0..p.inst.len() {
        if !is_alt(p.inst[pc].inst.op) {
            continue;
        }
        let mut a_other = (pc, OUT);
        let mut a_alt = (pc, ARG);
        let mut inst_alt = {
            let i = &p.inst[get_field(&p, a_alt) as usize].inst;
            (i.op, i.out, i.arg)
        };
        if !is_alt(inst_alt.0) {
            std::mem::swap(&mut a_alt, &mut a_other);
            inst_alt = {
                let i = &p.inst[get_field(&p, a_alt) as usize].inst;
                (i.op, i.out, i.arg)
            };
            if !is_alt(inst_alt.0) {
                continue;
            }
        }
        let other_op = p.inst[get_field(&p, a_other) as usize].inst.op;
        if is_alt(other_op) {
            continue;
        }
        let b_idx = get_field(&p, a_alt) as usize;
        let mut b_alt = (b_idx, OUT);
        let mut b_other = (b_idx, ARG);
        let mut patch = false;
        if inst_alt.1 == pc as u32 {
            patch = true;
        } else if inst_alt.2 == pc as u32 {
            patch = true;
            std::mem::swap(&mut b_alt, &mut b_other);
        }
        if patch {
            let v = get_field(&p, a_other);
            set_field(&mut p, b_alt, v);
        }
        if get_field(&p, a_other) == get_field(&p, b_alt) {
            let v = get_field(&p, b_other);
            set_field(&mut p, a_alt, v);
        }
    }
    p
}

struct Builder {
    p: OnePassProg,
    inst_queue: QueueOnePass,
    visit_queue: QueueOnePass,
    one_pass_runes: Vec<Vec<Rune>>,
}

fn fold_runes(r0: Rune) -> Vec<Rune> {
    let mut runes = vec![r0, r0];
    let mut r1 = unicode::simple_fold(r0);
    while r1 != r0 {
        runes.push(r1);
        runes.push(r1);
        r1 = unicode::simple_fold(r1);
    }
    runes.sort();
    runes
}

impl Builder {
    fn check(&mut self, pc: u32, m: &mut [bool]) -> bool {
        let mut ok = true;
        let pcu = pc as usize;
        if self.visit_queue.contains(pc) {
            return ok;
        }
        self.visit_queue.insert(pc);
        let op = self.p.inst[pcu].inst.op;
        match op {
            InstOp::Alt | InstOp::AltMatch => {
                let (out, arg) = (self.p.inst[pcu].inst.out, self.p.inst[pcu].inst.arg);
                ok = self.check(out, m) && self.check(arg, m);
                let inst = &mut self.p.inst[pcu].inst;
                let mut match_out = m[inst.out as usize];
                let mut match_arg = m[inst.arg as usize];
                if match_out && match_arg {
                    return false;
                }
                if match_arg {
                    std::mem::swap(&mut inst.out, &mut inst.arg);
                    std::mem::swap(&mut match_out, &mut match_arg);
                }
                if match_out {
                    m[pcu] = true;
                    inst.op = InstOp::AltMatch;
                }
                let (out, arg) = (inst.out, inst.arg);
                let (merged, next) = merge_rune_sets(
                    &self.one_pass_runes[out as usize],
                    &self.one_pass_runes[arg as usize],
                    out,
                    arg,
                );
                self.one_pass_runes[pcu] = merged;
                let failed = !next.is_empty() && next[0] == MERGE_FAILED;
                self.p.inst[pcu].next = next;
                if failed {
                    ok = false;
                }
            }
            InstOp::Capture | InstOp::Nop | InstOp::EmptyWidth => {
                let out = self.p.inst[pcu].inst.out;
                ok = self.check(out, m);
                m[pcu] = m[out as usize];
                self.one_pass_runes[pcu] = self.one_pass_runes[out as usize].clone();
                let n = self.one_pass_runes[pcu].len() / 2 + 1;
                self.p.inst[pcu].next = vec![out; n];
            }
            InstOp::Match | InstOp::Fail => {
                m[pcu] = op == InstOp::Match;
            }
            InstOp::Rune => {
                m[pcu] = false;
                if !self.p.inst[pcu].next.is_empty() {
                    return ok;
                }
                let inst = &self.p.inst[pcu].inst;
                let out = inst.out;
                self.inst_queue.insert(out);
                if inst.rune.is_empty() {
                    self.one_pass_runes[pcu] = Vec::new();
                    self.p.inst[pcu].next = vec![out];
                    return ok;
                }
                let runes = if inst.rune.len() == 1 && (inst.arg as Flags) & FOLD_CASE != 0 {
                    fold_runes(inst.rune[0])
                } else {
                    inst.rune.to_vec()
                };
                let n = runes.len() / 2 + 1;
                self.one_pass_runes[pcu] = runes;
                self.p.inst[pcu].next = vec![out; n];
                self.p.inst[pcu].inst.op = InstOp::Rune;
            }
            InstOp::Rune1 => {
                m[pcu] = false;
                if !self.p.inst[pcu].next.is_empty() {
                    return ok;
                }
                let inst = &self.p.inst[pcu].inst;
                let out = inst.out;
                self.inst_queue.insert(out);
                let runes = if (inst.arg as Flags) & FOLD_CASE != 0 {
                    fold_runes(inst.rune[0])
                } else {
                    vec![inst.rune[0], inst.rune[0]]
                };
                let n = runes.len() / 2 + 1;
                self.one_pass_runes[pcu] = runes;
                self.p.inst[pcu].next = vec![out; n];
                self.p.inst[pcu].inst.op = InstOp::Rune;
            }
            InstOp::RuneAny => {
                m[pcu] = false;
                if !self.p.inst[pcu].next.is_empty() {
                    return ok;
                }
                let out = self.p.inst[pcu].inst.out;
                self.inst_queue.insert(out);
                self.one_pass_runes[pcu] = ANY_RUNE.to_vec();
                self.p.inst[pcu].next = vec![out];
            }
            InstOp::RuneAnyNotNL => {
                m[pcu] = false;
                if !self.p.inst[pcu].next.is_empty() {
                    return ok;
                }
                let out = self.p.inst[pcu].inst.out;
                self.inst_queue.insert(out);
                self.one_pass_runes[pcu] = ANY_RUNE_NOT_NL.to_vec();
                let n = ANY_RUNE_NOT_NL.len() / 2 + 1;
                self.p.inst[pcu].next = vec![out; n];
            }
        }
        ok
    }
}

fn make_one_pass(p: OnePassProg) -> Option<OnePassProg> {
    if p.inst.len() >= 1000 {
        return None;
    }
    let n = p.inst.len();
    let start = p.start as u32;
    let mut b = Builder {
        p,
        inst_queue: QueueOnePass::new(n),
        visit_queue: QueueOnePass::new(n),
        one_pass_runes: vec![Vec::new(); n],
    };
    b.inst_queue.clear();
    b.inst_queue.insert(start);
    let mut m = vec![false; n];
    while !b.inst_queue.empty() {
        b.visit_queue.clear();
        let pc = b.inst_queue.next();
        if !b.check(pc, &mut m) {
            return None;
        }
    }
    for (inst, runes) in b.p.inst.iter_mut().zip(b.one_pass_runes) {
        inst.inst.rune = Arc::from(runes);
    }
    Some(b.p)
}

pub fn compile_one_pass(prog: &Prog) -> Option<OnePassProg> {
    if prog.start == 0 {
        return None;
    }
    let start = &prog.inst[prog.start];
    if start.op != InstOp::EmptyWidth
        || (start.arg as EmptyOp) & EMPTY_BEGIN_TEXT != EMPTY_BEGIN_TEXT
    {
        return None;
    }
    let has_alt = prog.inst.iter().any(|i| is_alt(i.op));
    for inst in &prog.inst {
        let op_out = prog.inst[inst.out as usize].op;
        match inst.op {
            InstOp::Alt | InstOp::AltMatch => {
                if op_out == InstOp::Match || prog.inst[inst.arg as usize].op == InstOp::Match {
                    return None;
                }
            }
            InstOp::EmptyWidth => {
                if op_out == InstOp::Match {
                    if (inst.arg as EmptyOp) & EMPTY_END_TEXT == EMPTY_END_TEXT {
                        continue;
                    }
                    return None;
                }
            }
            _ => {
                if op_out == InstOp::Match && has_alt {
                    return None;
                }
            }
        }
    }
    let p = one_pass_copy(prog);
    let mut p = make_one_pass(p)?;
    cleanup_one_pass(&mut p, prog);
    Some(p)
}
