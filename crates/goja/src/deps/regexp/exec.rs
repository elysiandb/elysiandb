use super::onepass::one_pass_next;
use super::regexp::{END_OF_TEXT, Input, InputBytes, InputReader, MATCH_SIZE, Regexp, RuneReader};
use super::syntax::{
    EMPTY_BEGIN_LINE, EMPTY_BEGIN_TEXT, EMPTY_END_LINE, EMPTY_END_TEXT, EMPTY_NO_WORD_BOUNDARY,
    EMPTY_WORD_BOUNDARY, EmptyOp, InstOp, is_word_char,
};
use gocompat::utf8::Rune;
use std::cell::RefCell;

pub type LazyFlag = u64;

pub fn new_lazy_flag(r1: Rune, r2: Rune) -> LazyFlag {
    ((r1 as i64 as u64) << 32) | u64::from(r2 as u32)
}

pub fn lazy_match(f: LazyFlag, op: EmptyOp) -> bool {
    let mut op = op;
    if op == 0 {
        return true;
    }
    let r1 = (f >> 32) as u32 as Rune;
    if op & EMPTY_BEGIN_LINE != 0 {
        if r1 != '\n' as Rune && r1 >= 0 {
            return false;
        }
        op &= !EMPTY_BEGIN_LINE;
    }
    if op & EMPTY_BEGIN_TEXT != 0 {
        if r1 >= 0 {
            return false;
        }
        op &= !EMPTY_BEGIN_TEXT;
    }
    if op == 0 {
        return true;
    }
    let r2 = f as u32 as Rune;
    if op & EMPTY_END_LINE != 0 {
        if r2 != '\n' as Rune && r2 >= 0 {
            return false;
        }
        op &= !EMPTY_END_LINE;
    }
    if op & EMPTY_END_TEXT != 0 {
        if r2 >= 0 {
            return false;
        }
        op &= !EMPTY_END_TEXT;
    }
    if op == 0 {
        return true;
    }
    if is_word_char(r1) != is_word_char(r2) {
        op &= !EMPTY_WORD_BOUNDARY;
    } else {
        op &= !EMPTY_NO_WORD_BOUNDARY;
    }
    op == 0
}

pub(crate) enum Source<'a> {
    Bytes(&'a [u8]),
    Reader(&'a mut dyn RuneReader),
}

struct Thread {
    pc: u32,
    cap: Vec<isize>,
}

#[derive(Clone, Copy)]
struct Entry {
    pc: u32,
    t: Option<usize>,
}

#[derive(Default)]
struct Queue {
    sparse: Vec<u32>,
    dense: Vec<Entry>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CapSrc {
    Match,
    Thread(usize),
}

enum Frame {
    Visit(u32),
    Restore(usize, isize, Option<usize>),
}

#[derive(Default)]
pub(crate) struct Machine {
    queues: [Queue; 2],
    threads: Vec<Thread>,
    pool: Vec<usize>,
    matched: bool,
    matchcap: Vec<isize>,
    stack: Vec<Frame>,
}

const POOLED: usize = 4;
const POOL_LIMIT: usize = 4;

thread_local! {
    static MATCH_POOL: RefCell<[Vec<Machine>; POOLED]> = RefCell::new(Default::default());
}

fn get_machine(re: &Regexp) -> Machine {
    let pooled = if re.mpool < POOLED {
        MATCH_POOL
            .try_with(|p| p.borrow_mut()[re.mpool].pop())
            .ok()
            .flatten()
    } else {
        None
    };
    let mut m = pooled.unwrap_or_default();
    let mut n = MATCH_SIZE[re.mpool];
    if n == 0 {
        n = re.prog.inst.len();
    }
    for q in m.queues.iter_mut() {
        if q.sparse.len() < n {
            q.sparse = vec![0; n];
            q.dense = Vec::with_capacity(n);
        }
        q.dense.clear();
    }
    m
}

fn put_machine(re: &Regexp, m: Machine) {
    if re.mpool < POOLED {
        let _ = MATCH_POOL.try_with(|p| {
            let mut p = p.borrow_mut();
            if p[re.mpool].len() < POOL_LIMIT {
                p[re.mpool].push(m);
            }
        });
    }
}

impl Machine {
    fn init(&mut self, ncap: usize) {
        for t in self.threads.iter_mut() {
            t.cap.resize(ncap, 0);
        }
        self.matchcap.clear();
        self.matchcap.resize(ncap, -1);
        self.pool.clear();
        self.pool.extend(0..self.threads.len());
        self.stack.clear();
    }

    fn alloc(&mut self, pc: u32) -> usize {
        if let Some(t) = self.pool.pop() {
            self.threads[t].pc = pc;
            return t;
        }
        self.threads.push(Thread {
            pc,
            cap: vec![0; self.matchcap.len()],
        });
        self.threads.len() - 1
    }

    fn cap_len(&self, cap: CapSrc) -> usize {
        match cap {
            CapSrc::Match => self.matchcap.len(),
            CapSrc::Thread(t) => self.threads[t].cap.len(),
        }
    }

    fn cap_get(&self, cap: CapSrc, i: usize) -> isize {
        match cap {
            CapSrc::Match => self.matchcap[i],
            CapSrc::Thread(t) => self.threads[t].cap[i],
        }
    }

    fn cap_set(&mut self, cap: CapSrc, i: usize, v: isize) {
        match cap {
            CapSrc::Match => self.matchcap[i] = v,
            CapSrc::Thread(t) => self.threads[t].cap[i] = v,
        }
    }

    fn copy_cap(&mut self, src: CapSrc, dst: usize) {
        let mut target = std::mem::take(&mut self.threads[dst].cap);
        {
            let from = match src {
                CapSrc::Match => &self.matchcap,
                CapSrc::Thread(t) => &self.threads[t].cap,
            };
            let n = target.len().min(from.len());
            target[..n].copy_from_slice(&from[..n]);
        }
        self.threads[dst].cap = target;
    }

    fn match_<I: Input>(&mut self, re: &Regexp, i: &mut I, pos: usize) -> bool {
        let start_cond = re.cond;
        if start_cond == !0 {
            return false;
        }
        let mut pos = pos;
        self.matched = false;
        for c in self.matchcap.iter_mut() {
            *c = -1;
        }
        let mut runq = 0;
        let mut nextq = 1;
        let mut r1 = END_OF_TEXT;
        let mut width1 = 0;
        let (mut r, mut width) = i.step(pos);
        if r != END_OF_TEXT {
            (r1, width1) = i.step(pos + width);
        }
        let mut flag = if pos == 0 {
            new_lazy_flag(-1, r)
        } else {
            i.context(pos)
        };
        loop {
            if self.queues[runq].dense.is_empty() {
                if start_cond & EMPTY_BEGIN_TEXT != 0 && pos != 0 {
                    break;
                }
                if self.matched {
                    break;
                }
                if !re.prefix.is_empty() && r1 != re.prefix_rune && i.can_check_prefix() {
                    let advance = i.index(re, pos);
                    if advance < 0 {
                        break;
                    }
                    pos += advance as usize;
                    (r, width) = i.step(pos);
                    (r1, width1) = i.step(pos + width);
                }
            }
            if !self.matched {
                if !self.matchcap.is_empty() {
                    self.matchcap[0] = pos as isize;
                }
                self.add(
                    re,
                    runq,
                    re.prog.start as u32,
                    pos,
                    CapSrc::Match,
                    flag,
                    None,
                );
            }
            flag = new_lazy_flag(r, r1);
            self.step(re, runq, nextq, pos, pos + width, r, flag);
            if width == 0 {
                break;
            }
            if self.matchcap.is_empty() && self.matched {
                break;
            }
            pos += width;
            (r, width) = (r1, width1);
            if r != END_OF_TEXT {
                (r1, width1) = i.step(pos + width);
            }
            std::mem::swap(&mut runq, &mut nextq);
        }
        self.clear(nextq);
        self.matched
    }

    fn clear(&mut self, q: usize) {
        for k in 0..self.queues[q].dense.len() {
            if let Some(t) = self.queues[q].dense[k].t {
                self.pool.push(t);
            }
        }
        self.queues[q].dense.clear();
    }

    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        re: &Regexp,
        runq: usize,
        nextq: usize,
        pos: usize,
        next_pos: usize,
        c: Rune,
        next_cond: LazyFlag,
    ) {
        let longest = re.longest;
        let mut j = 0;
        while j < self.queues[runq].dense.len() {
            let d = self.queues[runq].dense[j];
            let Some(t) = d.t else {
                j += 1;
                continue;
            };
            if longest
                && self.matched
                && !self.threads[t].cap.is_empty()
                && self.matchcap[0] < self.threads[t].cap[0]
            {
                self.pool.push(t);
                j += 1;
                continue;
            }
            let inst = &re.prog.inst[self.threads[t].pc as usize];
            let mut add = false;
            match inst.op {
                InstOp::Match => {
                    if !self.threads[t].cap.is_empty()
                        && (!longest || !self.matched || self.matchcap[1] < pos as isize)
                    {
                        self.threads[t].cap[1] = pos as isize;
                        self.copy_cap_to_match(t);
                    }
                    if !longest {
                        for k in j + 1..self.queues[runq].dense.len() {
                            if let Some(other) = self.queues[runq].dense[k].t {
                                self.pool.push(other);
                            }
                        }
                        self.queues[runq].dense.clear();
                    }
                    self.matched = true;
                }
                InstOp::Rune => add = inst.match_rune(c),
                InstOp::Rune1 => add = c == inst.rune[0],
                InstOp::RuneAny => add = true,
                InstOp::RuneAnyNotNL => add = c != '\n' as Rune,
                _ => panic!("bad instruction in regular expression matcher"),
            }
            let mut thread = Some(t);
            if add {
                thread = self.add(
                    re,
                    nextq,
                    inst.out,
                    next_pos,
                    CapSrc::Thread(t),
                    next_cond,
                    thread,
                );
            }
            if let Some(t) = thread {
                self.pool.push(t);
            }
            j += 1;
        }
        self.queues[runq].dense.clear();
    }

    fn copy_cap_to_match(&mut self, t: usize) {
        let from = &self.threads[t].cap;
        let n = self.matchcap.len().min(from.len());
        self.matchcap[..n].copy_from_slice(&from[..n]);
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        re: &Regexp,
        q: usize,
        pc: u32,
        pos: usize,
        cap: CapSrc,
        cond: LazyFlag,
        t: Option<usize>,
    ) -> Option<usize> {
        let mut t = t;
        let base = self.stack.len();
        self.stack.push(Frame::Visit(pc));
        while self.stack.len() > base {
            match self.stack.pop() {
                Some(Frame::Restore(arg, opos, saved)) => {
                    self.cap_set(cap, arg, opos);
                    t = saved;
                }
                Some(Frame::Visit(start)) => {
                    let mut pc = start;
                    loop {
                        if pc == 0 {
                            break;
                        }
                        let queue = &mut self.queues[q];
                        let j = queue.sparse[pc as usize] as usize;
                        if j < queue.dense.len() && queue.dense[j].pc == pc {
                            break;
                        }
                        let j = queue.dense.len();
                        queue.dense.push(Entry { pc, t: None });
                        queue.sparse[pc as usize] = j as u32;
                        let inst = &re.prog.inst[pc as usize];
                        match inst.op {
                            InstOp::Fail => break,
                            InstOp::Alt | InstOp::AltMatch => {
                                self.stack.push(Frame::Visit(inst.arg));
                                pc = inst.out;
                            }
                            InstOp::EmptyWidth => {
                                if lazy_match(cond, inst.arg as EmptyOp) {
                                    pc = inst.out;
                                } else {
                                    break;
                                }
                            }
                            InstOp::Nop => pc = inst.out,
                            InstOp::Capture => {
                                let arg = inst.arg as usize;
                                if arg < self.cap_len(cap) {
                                    let opos = self.cap_get(cap, arg);
                                    self.cap_set(cap, arg, pos as isize);
                                    self.stack.push(Frame::Restore(arg, opos, t));
                                    t = None;
                                }
                                pc = inst.out;
                            }
                            InstOp::Match
                            | InstOp::Rune
                            | InstOp::Rune1
                            | InstOp::RuneAny
                            | InstOp::RuneAnyNotNL => {
                                let tt = match t {
                                    None => self.alloc(pc),
                                    Some(tt) => {
                                        self.threads[tt].pc = pc;
                                        tt
                                    }
                                };
                                if self.cap_len(cap) > 0 && cap != CapSrc::Thread(tt) {
                                    self.copy_cap(cap, tt);
                                }
                                self.queues[q].dense[j].t = Some(tt);
                                t = None;
                                break;
                            }
                        }
                    }
                }
                None => break,
            }
        }
        t
    }
}

impl Regexp {
    fn do_one_pass<I: Input>(&self, i: &mut I, pos: usize, ncap: usize) -> Option<Vec<isize>> {
        let start_cond = self.cond;
        if start_cond == !0 {
            return None;
        }
        let onepass = self.onepass.as_ref()?;
        let mut matchcap = vec![-1isize; ncap];
        let mut matched = false;
        let mut pos = pos;
        let mut r1 = END_OF_TEXT;
        let mut width1 = 0;
        let (mut r, mut width) = i.step(pos);
        if r != END_OF_TEXT {
            (r1, width1) = i.step(pos + width);
        }
        let mut flag = if pos == 0 {
            new_lazy_flag(-1, r)
        } else {
            i.context(pos)
        };
        let mut pc = onepass.start;
        'ret: {
            let inst = &onepass.inst[pc];
            if pos == 0
                && lazy_match(flag, inst.inst.arg as EmptyOp)
                && !self.prefix.is_empty()
                && i.can_check_prefix()
            {
                if !i.has_prefix(self) {
                    break 'ret;
                }
                pos += self.prefix.len();
                (r, width) = i.step(pos);
                (r1, width1) = i.step(pos + width);
                flag = i.context(pos);
                pc = self.prefix_end as usize;
            }
            loop {
                let inst = &onepass.inst[pc];
                pc = inst.inst.out as usize;
                match inst.inst.op {
                    InstOp::Match => {
                        matched = true;
                        if !matchcap.is_empty() {
                            matchcap[0] = 0;
                            matchcap[1] = pos as isize;
                        }
                        break 'ret;
                    }
                    InstOp::Rune => {
                        if !inst.inst.match_rune(r) {
                            break 'ret;
                        }
                    }
                    InstOp::Rune1 => {
                        if r != inst.inst.rune[0] {
                            break 'ret;
                        }
                    }
                    InstOp::RuneAny => {}
                    InstOp::RuneAnyNotNL => {
                        if r == '\n' as Rune {
                            break 'ret;
                        }
                    }
                    InstOp::Alt | InstOp::AltMatch => {
                        pc = one_pass_next(inst, r) as usize;
                        continue;
                    }
                    InstOp::Fail => break 'ret,
                    InstOp::Nop => continue,
                    InstOp::EmptyWidth => {
                        if !lazy_match(flag, inst.inst.arg as EmptyOp) {
                            break 'ret;
                        }
                        continue;
                    }
                    InstOp::Capture => {
                        let arg = inst.inst.arg as usize;
                        if arg < matchcap.len() {
                            matchcap[arg] = pos as isize;
                        }
                        continue;
                    }
                }
                if width == 0 {
                    break;
                }
                flag = new_lazy_flag(r, r1);
                pos += width;
                (r, width) = (r1, width1);
                if r != END_OF_TEXT {
                    (r1, width1) = i.step(pos + width);
                }
            }
        }
        if !matched {
            return None;
        }
        Some(matchcap)
    }

    fn run_machine<I: Input>(&self, i: &mut I, pos: usize, ncap: usize) -> Option<Vec<isize>> {
        let mut m = get_machine(self);
        m.init(ncap);
        let result = if m.match_(self, i, pos) {
            Some(m.matchcap.clone())
        } else {
            None
        };
        put_machine(self, m);
        result
    }

    pub(crate) fn do_execute(
        &self,
        src: Source<'_>,
        pos: usize,
        ncap: usize,
    ) -> Option<Vec<isize>> {
        match src {
            Source::Bytes(b) => {
                if (b.len() as isize) < self.min_input_len {
                    return None;
                }
                let mut input = InputBytes { str: b };
                if self.onepass.is_some() {
                    return self.do_one_pass(&mut input, pos, ncap);
                }
                if b.len() < self.max_bit_state_len {
                    return self.backtrack(b, pos, ncap);
                }
                self.run_machine(&mut input, pos, ncap)
            }
            Source::Reader(r) => {
                let mut input = InputReader {
                    r,
                    at_eot: false,
                    pos: 0,
                };
                if self.onepass.is_some() {
                    return self.do_one_pass(&mut input, pos, ncap);
                }
                self.run_machine(&mut input, pos, ncap)
            }
        }
    }
}
