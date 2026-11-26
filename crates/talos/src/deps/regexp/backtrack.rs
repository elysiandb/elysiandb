use super::exec::lazy_match;
use super::regexp::{END_OF_TEXT, Input, InputBytes, Regexp};
use super::syntax::{EMPTY_BEGIN_TEXT, EmptyOp, InstOp, Prog};
use gocompat::utf8::Rune;
use std::cell::RefCell;

#[derive(Clone, Copy)]
struct Job {
    pc: u32,
    arg: bool,
    pos: isize,
}

const VISITED_BITS: usize = 32;
pub const MAX_BACKTRACK_PROG: usize = 500;
pub const MAX_BACKTRACK_VECTOR: usize = 256 * 1024;

#[derive(Default)]
struct BitState {
    end: usize,
    cap: Vec<isize>,
    matchcap: Vec<isize>,
    jobs: Vec<Job>,
    visited: Vec<u32>,
}

thread_local! {
    static BIT_STATE_POOL: RefCell<Vec<BitState>> = const { RefCell::new(Vec::new()) };
}

fn new_bit_state() -> BitState {
    BIT_STATE_POOL
        .try_with(|p| p.borrow_mut().pop())
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn free_bit_state(b: BitState) {
    let _ = BIT_STATE_POOL.try_with(|p| {
        let mut p = p.borrow_mut();
        if p.len() < 4 {
            p.push(b);
        }
    });
}

pub fn max_bit_state_len(prog: &Prog) -> usize {
    if !should_backtrack(prog) {
        return 0;
    }
    MAX_BACKTRACK_VECTOR / prog.inst.len()
}

pub fn should_backtrack(prog: &Prog) -> bool {
    prog.inst.len() <= MAX_BACKTRACK_PROG
}

impl BitState {
    fn reset(&mut self, prog: &Prog, end: usize, ncap: usize) {
        self.end = end;
        self.jobs.clear();
        let visited_size = (prog.inst.len() * (end + 1)).div_ceil(VISITED_BITS);
        self.visited.clear();
        self.visited.resize(visited_size, 0);
        self.cap.clear();
        self.cap.resize(ncap, -1);
        self.matchcap.clear();
        self.matchcap.resize(ncap, -1);
    }

    fn should_visit(&mut self, pc: u32, pos: usize) -> bool {
        let n = pc as usize * (self.end + 1) + pos;
        let bit = 1u32 << (n & (VISITED_BITS - 1));
        if self.visited[n / VISITED_BITS] & bit != 0 {
            return false;
        }
        self.visited[n / VISITED_BITS] |= bit;
        true
    }

    fn push(&mut self, re: &Regexp, pc: u32, pos: isize, arg: bool) {
        if re.prog.inst[pc as usize].op != InstOp::Fail
            && (arg || self.should_visit(pc, pos as usize))
        {
            self.jobs.push(Job { pc, arg, pos });
        }
    }
}

impl Regexp {
    fn try_backtrack<I: Input>(&self, b: &mut BitState, i: &mut I, pc: u32, pos: usize) -> bool {
        let longest = self.longest;
        b.push(self, pc, pos as isize, false);
        while let Some(job) = b.jobs.pop() {
            let mut pc = job.pc;
            let mut pos = job.pos;
            let mut arg = job.arg;
            let mut check = false;
            loop {
                if check && !b.should_visit(pc, pos as usize) {
                    break;
                }
                check = true;
                let inst = &self.prog.inst[pc as usize];
                match inst.op {
                    InstOp::Fail => panic!("unexpected fail instruction in backtracker"),
                    InstOp::Alt => {
                        if arg {
                            arg = false;
                            pc = inst.arg;
                        } else {
                            b.push(self, pc, pos, true);
                            pc = inst.out;
                        }
                    }
                    InstOp::AltMatch => {
                        match self.prog.inst[inst.out as usize].op {
                            InstOp::Rune
                            | InstOp::Rune1
                            | InstOp::RuneAny
                            | InstOp::RuneAnyNotNL => {
                                b.push(self, inst.arg, pos, false);
                                pc = inst.arg;
                                pos = b.end as isize;
                                continue;
                            }
                            _ => {}
                        }
                        b.push(self, inst.out, b.end as isize, false);
                        pc = inst.out;
                    }
                    InstOp::Rune => {
                        let (r, width) = i.step(pos as usize);
                        if !inst.match_rune(r) {
                            break;
                        }
                        pos += width as isize;
                        pc = inst.out;
                    }
                    InstOp::Rune1 => {
                        let (r, width) = i.step(pos as usize);
                        if r != inst.rune[0] {
                            break;
                        }
                        pos += width as isize;
                        pc = inst.out;
                    }
                    InstOp::RuneAnyNotNL => {
                        let (r, width) = i.step(pos as usize);
                        if r == '\n' as Rune || r == END_OF_TEXT {
                            break;
                        }
                        pos += width as isize;
                        pc = inst.out;
                    }
                    InstOp::RuneAny => {
                        let (r, width) = i.step(pos as usize);
                        if r == END_OF_TEXT {
                            break;
                        }
                        pos += width as isize;
                        pc = inst.out;
                    }
                    InstOp::Capture => {
                        if arg {
                            b.cap[inst.arg as usize] = pos;
                            break;
                        }
                        if (inst.arg as usize) < b.cap.len() {
                            let old = b.cap[inst.arg as usize];
                            b.push(self, pc, old, true);
                            b.cap[inst.arg as usize] = pos;
                        }
                        pc = inst.out;
                    }
                    InstOp::EmptyWidth => {
                        let flag = i.context(pos as usize);
                        if !lazy_match(flag, inst.arg as EmptyOp) {
                            break;
                        }
                        pc = inst.out;
                    }
                    InstOp::Nop => pc = inst.out,
                    InstOp::Match => {
                        if b.cap.is_empty() {
                            return true;
                        }
                        if b.cap.len() > 1 {
                            b.cap[1] = pos;
                        }
                        let old = b.matchcap[1];
                        if old == -1 || (longest && pos > 0 && pos > old) {
                            let n = b.matchcap.len().min(b.cap.len());
                            let (cap, matchcap) = (&b.cap, &mut b.matchcap);
                            matchcap[..n].copy_from_slice(&cap[..n]);
                        }
                        if !longest {
                            return true;
                        }
                        if pos == b.end as isize {
                            return true;
                        }
                        break;
                    }
                }
            }
        }
        longest && b.matchcap.len() > 1 && b.matchcap[1] >= 0
    }

    pub(crate) fn backtrack(&self, ib: &[u8], pos: usize, ncap: usize) -> Option<Vec<isize>> {
        let start_cond = self.cond;
        if start_cond == !0 {
            return None;
        }
        if start_cond & EMPTY_BEGIN_TEXT != 0 && pos != 0 {
            return None;
        }
        let mut b = new_bit_state();
        let mut i = InputBytes { str: ib };
        let end = ib.len();
        b.reset(&self.prog, end, ncap);
        let mut pos = pos;
        let matched = if start_cond & EMPTY_BEGIN_TEXT != 0 {
            if !b.cap.is_empty() {
                b.cap[0] = pos as isize;
            }
            self.try_backtrack(&mut b, &mut i, self.prog.start as u32, pos)
        } else {
            let mut found = false;
            let mut width: isize = -1;
            while pos <= end && width != 0 {
                if !self.prefix.is_empty() {
                    let advance = i.index(self, pos);
                    if advance < 0 {
                        break;
                    }
                    pos += advance as usize;
                }
                if !b.cap.is_empty() {
                    b.cap[0] = pos as isize;
                }
                if self.try_backtrack(&mut b, &mut i, self.prog.start as u32, pos) {
                    found = true;
                    break;
                }
                width = i.step(pos).1 as isize;
                pos += width as usize;
            }
            found
        };
        let result = if matched {
            Some(b.matchcap.clone())
        } else {
            None
        };
        free_bit_state(b);
        result
    }
}
