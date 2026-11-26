use std::sync::Arc;

use gocompat::unicode;
use gocompat::utf8::Rune;

use super::r#match::MatchState;
use super::regexp::{Error, Regexp};
use super::syntax::{
    ANCHOR_BEGINNING, ANCHOR_END, ANCHOR_END_Z, ANCHOR_START, BACK, BACK2, BACKJUMP, BEGINNING,
    BOL, BOUNDARY, BRANCHCOUNT, BRANCHMARK, CAPTUREMARK, CI, Code, ECMA_BOUNDARY, ECMASCRIPT, END,
    END_Z, EOL, FOREJUMP, GETMARK, GOTO, InstOp, LAZYBRANCH, LAZYBRANCHCOUNT, LAZYBRANCHMARK,
    MULTI, NON_ECMA_BOUNDARY, NONBOUNDARY, NOTHING, NOTONE, NOTONELAZY, NOTONELOOP, NOTONEREP,
    NULLCOUNT, NULLMARK, ONE, ONELAZY, ONELOOP, ONEREP, RE2, REF, RTL, SET, SETCOUNT, SETJUMP,
    SETLAZY, SETLOOP, SETMARK, SETREP, START, STOP, TESTREF, char_description, is_ecma_word_char,
    is_word_char, write_stdout,
};

const LAZYBRANCH_BACK: InstOp = LAZYBRANCH | BACK;
const SETMARK_BACK: InstOp = SETMARK | BACK;
const NULLMARK_BACK: InstOp = NULLMARK | BACK;
const GETMARK_BACK: InstOp = GETMARK | BACK;
const CAPTUREMARK_BACK: InstOp = CAPTUREMARK | BACK;
const BRANCHMARK_BACK: InstOp = BRANCHMARK | BACK;
const BRANCHMARK_BACK2: InstOp = BRANCHMARK | BACK2;
const LAZYBRANCHMARK_BACK: InstOp = LAZYBRANCHMARK | BACK;
const LAZYBRANCHMARK_BACK2: InstOp = LAZYBRANCHMARK | BACK2;
const SETCOUNT_BACK: InstOp = SETCOUNT | BACK;
const NULLCOUNT_BACK: InstOp = NULLCOUNT | BACK;
const BRANCHCOUNT_BACK: InstOp = BRANCHCOUNT | BACK;
const BRANCHCOUNT_BACK2: InstOp = BRANCHCOUNT | BACK2;
const LAZYBRANCHCOUNT_BACK: InstOp = LAZYBRANCHCOUNT | BACK;
const LAZYBRANCHCOUNT_BACK2: InstOp = LAZYBRANCHCOUNT | BACK2;
const SETJUMP_BACK: InstOp = SETJUMP | BACK;
const FOREJUMP_BACK: InstOp = FOREJUMP | BACK;
const ONELOOP_BACK: InstOp = ONELOOP | BACK;
const NOTONELOOP_BACK: InstOp = NOTONELOOP | BACK;
const SETLOOP_BACK: InstOp = SETLOOP | BACK;
const ONELAZY_BACK: InstOp = ONELAZY | BACK;
const NOTONELAZY_BACK: InstOp = NOTONELAZY | BACK;
const SETLAZY_BACK: InstOp = SETLAZY | BACK;

pub(crate) enum Scanned {
    None,
    Quick,
    Found(MatchState, isize),
}

#[derive(Debug, Default)]
pub(crate) struct Runner {
    runtextstart: isize,
    runtext: Option<Arc<[Rune]>>,
    runtextpos: isize,
    runtextend: isize,
    runtrack: Vec<isize>,
    runtrackpos: isize,
    runstack: Vec<isize>,
    runstackpos: isize,
    runcrawl: Vec<isize>,
    runcrawlpos: isize,
    runtrackcount: isize,
    runmatch: Option<MatchState>,
    operator: InstOp,
    codepos: isize,
    right_to_left: bool,
    case_insensitive: bool,
}

impl Runner {
    pub(crate) fn release(&mut self) {
        self.runtext = None;
    }

    pub(crate) fn scan(
        &mut self,
        re: &Regexp,
        rt: Arc<[Rune]>,
        textstart: isize,
        quick: bool,
    ) -> Result<Scanned, Error> {
        self.runtextstart = textstart;
        self.runtextend = rt.len() as isize;
        self.runtext = Some(rt);
        let mut stoppos = self.runtextend;
        let mut bump = 1;
        if re.right_to_left() {
            bump = -1;
            stoppos = 0;
        }
        self.runtextpos = textstart;
        let mut initted = false;
        let code = re.code();
        loop {
            if re.debug() {
                write_stdout(&format!("\nSearch range: from 0 to {}\n", self.runtextend));
                write_stdout(&format!(
                    "Firstchar search starting at {} stopping at {}\n",
                    self.runtextpos, stoppos
                ));
            }
            if self.find_first_char(code) {
                if !initted {
                    self.init_match(re);
                    initted = true;
                }
                if re.debug() {
                    write_stdout(&format!(
                        "Executing engine starting at {}\n\n",
                        self.runtextpos
                    ));
                }
                self.execute(re)?;
                if self.runmatch().matchcount[0] > 0 {
                    return Ok(self.tidy_match(quick));
                }
                self.runtrackpos = self.runtrack.len() as isize;
                self.runstackpos = self.runstack.len() as isize;
                self.runcrawlpos = self.runcrawl.len() as isize;
            }
            if self.runtextpos == stoppos {
                self.tidy_match(true);
                return Ok(Scanned::None);
            }
            self.runtextpos += bump;
        }
    }

    fn runmatch(&self) -> &MatchState {
        self.runmatch.as_ref().expect("an initialized match")
    }

    fn runmatch_mut(&mut self) -> &mut MatchState {
        self.runmatch.as_mut().expect("an initialized match")
    }

    fn text(&self) -> &[Rune] {
        self.runtext.as_deref().expect("a text to scan")
    }

    fn execute(&mut self, re: &Regexp) -> Result<(), Error> {
        let code = re.code();
        let debug = re.debug();
        let options = re.options();
        self.go_to(code, 0);
        loop {
            if debug {
                self.dump_state(code);
            }
            let backtrack = match self.operator {
                STOP => return Ok(()),
                NOTHING => true,
                GOTO => {
                    let o = self.operand(code, 0);
                    self.go_to(code, o);
                    false
                }
                TESTREF => {
                    if !self.runmatch().is_matched(self.operand(code, 0)) {
                        true
                    } else {
                        self.advance(code, 1);
                        false
                    }
                }
                LAZYBRANCH => {
                    let p = self.text_pos();
                    self.track_push1(p);
                    self.advance(code, 1);
                    false
                }
                LAZYBRANCH_BACK => {
                    self.track_pop();
                    let p = self.track_peek();
                    self.textto(p);
                    let o = self.operand(code, 0);
                    self.go_to(code, o);
                    false
                }
                SETMARK => {
                    let p = self.text_pos();
                    self.stack_push(p);
                    self.track_push();
                    self.advance(code, 0);
                    false
                }
                NULLMARK => {
                    self.stack_push(-1);
                    self.track_push();
                    self.advance(code, 0);
                    false
                }
                SETMARK_BACK | NULLMARK_BACK => {
                    self.stack_pop();
                    true
                }
                GETMARK => {
                    self.stack_pop();
                    let p = self.stack_peek();
                    self.track_push1(p);
                    self.textto(p);
                    self.advance(code, 0);
                    false
                }
                GETMARK_BACK => {
                    self.track_pop();
                    let p = self.track_peek();
                    self.stack_push(p);
                    true
                }
                CAPTUREMARK => {
                    let o1 = self.operand(code, 1);
                    if o1 != -1 && !self.runmatch().is_matched(o1) {
                        true
                    } else {
                        self.stack_pop();
                        let o0 = self.operand(code, 0);
                        let mark = self.stack_peek();
                        let pos = self.text_pos();
                        if o1 != -1 {
                            self.transfer_capture(o0, o1, mark, pos);
                        } else {
                            self.capture(o0, mark, pos);
                        }
                        let mark = self.stack_peek();
                        self.track_push1(mark);
                        self.advance(code, 2);
                        false
                    }
                }
                CAPTUREMARK_BACK => {
                    self.track_pop();
                    let p = self.track_peek();
                    self.stack_push(p);
                    self.uncapture();
                    if self.operand(code, 0) != -1 && self.operand(code, 1) != -1 {
                        self.uncapture();
                    }
                    true
                }
                BRANCHMARK => {
                    self.stack_pop();
                    let matched = self.text_pos() - self.stack_peek();
                    if matched != 0 {
                        let (mark, pos) = (self.stack_peek(), self.text_pos());
                        self.track_push2(mark, pos);
                        self.stack_push(pos);
                        let o = self.operand(code, 0);
                        self.go_to(code, o);
                    } else {
                        let mark = self.stack_peek();
                        self.track_push_neg1(mark);
                        self.advance(code, 1);
                    }
                    false
                }
                BRANCHMARK_BACK => {
                    self.track_pop_n(2);
                    self.stack_pop();
                    let p = self.track_peek_n(1);
                    self.textto(p);
                    let mark = self.track_peek();
                    self.track_push_neg1(mark);
                    self.advance(code, 1);
                    false
                }
                BRANCHMARK_BACK2 => {
                    self.track_pop();
                    let mark = self.track_peek();
                    self.stack_push(mark);
                    true
                }
                LAZYBRANCHMARK => {
                    self.stack_pop();
                    let old_mark_pos = self.stack_peek();
                    let pos = self.text_pos();
                    if pos != old_mark_pos {
                        if old_mark_pos != -1 {
                            self.track_push2(old_mark_pos, pos);
                        } else {
                            self.track_push2(pos, pos);
                        }
                    } else {
                        self.stack_push(old_mark_pos);
                        let mark = self.stack_peek();
                        self.track_push_neg1(mark);
                    }
                    self.advance(code, 1);
                    false
                }
                LAZYBRANCHMARK_BACK => {
                    self.track_pop_n(2);
                    let pos = self.track_peek_n(1);
                    let mark = self.track_peek();
                    self.track_push_neg1(mark);
                    self.stack_push(pos);
                    self.textto(pos);
                    let o = self.operand(code, 0);
                    self.go_to(code, o);
                    false
                }
                LAZYBRANCHMARK_BACK2 => {
                    self.stack_pop();
                    self.track_pop();
                    let mark = self.track_peek();
                    self.stack_push(mark);
                    true
                }
                SETCOUNT => {
                    let (pos, o) = (self.text_pos(), self.operand(code, 0));
                    self.stack_push2(pos, o);
                    self.track_push();
                    self.advance(code, 1);
                    false
                }
                NULLCOUNT => {
                    let o = self.operand(code, 0);
                    self.stack_push2(-1, o);
                    self.track_push();
                    self.advance(code, 1);
                    false
                }
                SETCOUNT_BACK | NULLCOUNT_BACK => {
                    self.stack_pop_n(2);
                    true
                }
                BRANCHCOUNT => {
                    self.stack_pop_n(2);
                    let mark = self.stack_peek();
                    let count = self.stack_peek_n(1);
                    let matched = self.text_pos() - mark;
                    if count >= self.operand(code, 1) || (matched == 0 && count >= 0) {
                        self.track_push_neg2(mark, count);
                        self.advance(code, 2);
                    } else {
                        self.track_push1(mark);
                        let pos = self.text_pos();
                        self.stack_push2(pos, count + 1);
                        let o = self.operand(code, 0);
                        self.go_to(code, o);
                    }
                    false
                }
                BRANCHCOUNT_BACK => {
                    self.track_pop();
                    self.stack_pop_n(2);
                    if self.stack_peek_n(1) > 0 {
                        let p = self.stack_peek();
                        self.textto(p);
                        let (mark, count) = (self.track_peek(), self.stack_peek_n(1) - 1);
                        self.track_push_neg2(mark, count);
                        self.advance(code, 2);
                        false
                    } else {
                        let (mark, count) = (self.track_peek(), self.stack_peek_n(1) - 1);
                        self.stack_push2(mark, count);
                        true
                    }
                }
                BRANCHCOUNT_BACK2 => {
                    self.track_pop_n(2);
                    let (mark, count) = (self.track_peek(), self.track_peek_n(1));
                    self.stack_push2(mark, count);
                    true
                }
                LAZYBRANCHCOUNT => {
                    self.stack_pop_n(2);
                    let mark = self.stack_peek();
                    let count = self.stack_peek_n(1);
                    if count < 0 {
                        self.track_push_neg1(mark);
                        let pos = self.text_pos();
                        self.stack_push2(pos, count + 1);
                        let o = self.operand(code, 0);
                        self.go_to(code, o);
                    } else {
                        let pos = self.text_pos();
                        self.track_push3(mark, count, pos);
                        self.advance(code, 2);
                    }
                    false
                }
                LAZYBRANCHCOUNT_BACK => {
                    self.track_pop_n(3);
                    let mark = self.track_peek();
                    let textpos = self.track_peek_n(2);
                    if self.track_peek_n(1) < self.operand(code, 1) && textpos != mark {
                        self.textto(textpos);
                        let count = self.track_peek_n(1) + 1;
                        self.stack_push2(textpos, count);
                        self.track_push_neg1(mark);
                        let o = self.operand(code, 0);
                        self.go_to(code, o);
                        false
                    } else {
                        let (m, c) = (self.track_peek(), self.track_peek_n(1));
                        self.stack_push2(m, c);
                        true
                    }
                }
                LAZYBRANCHCOUNT_BACK2 => {
                    self.track_pop();
                    self.stack_pop_n(2);
                    let (mark, count) = (self.track_peek(), self.stack_peek_n(1) - 1);
                    self.stack_push2(mark, count);
                    true
                }
                SETJUMP => {
                    let (t, c) = (self.trackpos(), self.crawlpos());
                    self.stack_push2(t, c);
                    self.track_push();
                    self.advance(code, 0);
                    false
                }
                SETJUMP_BACK => {
                    self.stack_pop_n(2);
                    true
                }
                BACKJUMP => {
                    self.stack_pop_n(2);
                    let t = self.stack_peek();
                    self.trackto(t);
                    while self.crawlpos() != self.stack_peek_n(1) {
                        self.uncapture();
                    }
                    true
                }
                FOREJUMP => {
                    self.stack_pop_n(2);
                    let t = self.stack_peek();
                    self.trackto(t);
                    let c = self.stack_peek_n(1);
                    self.track_push1(c);
                    self.advance(code, 0);
                    false
                }
                FOREJUMP_BACK => {
                    self.track_pop();
                    while self.crawlpos() != self.track_peek() {
                        self.uncapture();
                    }
                    true
                }
                BOL => {
                    if self.leftchars() > 0 && self.char_at(self.text_pos() - 1) != '\n' as Rune {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                EOL => {
                    if self.rightchars() > 0 && self.char_at(self.text_pos()) != '\n' as Rune {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                BOUNDARY => {
                    if !self.is_boundary(self.text_pos(), 0, self.runtextend) {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                NONBOUNDARY => {
                    if self.is_boundary(self.text_pos(), 0, self.runtextend) {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                ECMA_BOUNDARY => {
                    if !self.is_ecma_boundary(self.text_pos(), 0, self.runtextend) {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                NON_ECMA_BOUNDARY => {
                    if self.is_ecma_boundary(self.text_pos(), 0, self.runtextend) {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                BEGINNING => {
                    if self.leftchars() > 0 {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                START => {
                    if self.text_pos() != self.textstart() {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                END_Z => {
                    let rchars = self.rightchars();
                    if rchars > 1 {
                        true
                    } else if options & (RE2 | ECMASCRIPT) != 0 {
                        if rchars > 0 {
                            true
                        } else {
                            self.advance(code, 0);
                            false
                        }
                    } else if rchars == 1 && self.char_at(self.text_pos()) != '\n' as Rune {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                END => {
                    if self.rightchars() > 0 {
                        true
                    } else {
                        self.advance(code, 0);
                        false
                    }
                }
                ONE => {
                    if self.forwardchars() < 1
                        || self.forwardcharnext() != self.operand(code, 0) as Rune
                    {
                        true
                    } else {
                        self.advance(code, 1);
                        false
                    }
                }
                NOTONE => {
                    if self.forwardchars() < 1
                        || self.forwardcharnext() == self.operand(code, 0) as Rune
                    {
                        true
                    } else {
                        self.advance(code, 1);
                        false
                    }
                }
                SET => {
                    if self.forwardchars() < 1 {
                        true
                    } else {
                        let ch = self.forwardcharnext();
                        if !code.sets[self.operand(code, 0) as usize].char_in(ch) {
                            true
                        } else {
                            self.advance(code, 1);
                            false
                        }
                    }
                }
                MULTI => {
                    let s = &code.strings[self.operand(code, 0) as usize];
                    if !self.runematch(s) {
                        true
                    } else {
                        self.advance(code, 1);
                        false
                    }
                }
                REF => {
                    let capnum = self.operand(code, 0);
                    if self.runmatch().is_matched(capnum) {
                        let index = self.runmatch().match_index(capnum);
                        let len = self.runmatch().match_length(capnum);
                        if !self.refmatch(index, len) {
                            true
                        } else {
                            self.advance(code, 1);
                            false
                        }
                    } else if options & ECMASCRIPT == 0 {
                        true
                    } else {
                        self.advance(code, 1);
                        false
                    }
                }
                ONEREP => {
                    let mut c = self.operand(code, 1);
                    if self.forwardchars() < c {
                        true
                    } else {
                        let ch = self.operand(code, 0) as Rune;
                        let mut failed = false;
                        while c > 0 {
                            if self.forwardcharnext() != ch {
                                failed = true;
                                break;
                            }
                            c -= 1;
                        }
                        if !failed {
                            self.advance(code, 2);
                        }
                        failed
                    }
                }
                NOTONEREP => {
                    let mut c = self.operand(code, 1);
                    if self.forwardchars() < c {
                        true
                    } else {
                        let ch = self.operand(code, 0) as Rune;
                        let mut failed = false;
                        while c > 0 {
                            if self.forwardcharnext() == ch {
                                failed = true;
                                break;
                            }
                            c -= 1;
                        }
                        if !failed {
                            self.advance(code, 2);
                        }
                        failed
                    }
                }
                SETREP => {
                    let mut c = self.operand(code, 1);
                    if self.forwardchars() < c {
                        true
                    } else {
                        let set = &code.sets[self.operand(code, 0) as usize];
                        let mut failed = false;
                        while c > 0 {
                            let ch = self.forwardcharnext();
                            if !set.char_in(ch) {
                                failed = true;
                                break;
                            }
                            c -= 1;
                        }
                        if !failed {
                            self.advance(code, 2);
                        }
                        failed
                    }
                }
                ONELOOP | NOTONELOOP => {
                    let not = self.operator == NOTONELOOP;
                    let mut c = self.operand(code, 1);
                    if c > self.forwardchars() {
                        c = self.forwardchars();
                    }
                    let ch = self.operand(code, 0) as Rune;
                    let mut i = c;
                    while i > 0 {
                        let next = self.forwardcharnext();
                        if (next != ch) != not {
                            self.backwardnext();
                            break;
                        }
                        i -= 1;
                    }
                    if c > i {
                        let pos = self.text_pos() - self.bump();
                        self.track_push2(c - i - 1, pos);
                    }
                    self.advance(code, 2);
                    false
                }
                SETLOOP => {
                    let mut c = self.operand(code, 1);
                    if c > self.forwardchars() {
                        c = self.forwardchars();
                    }
                    let set = &code.sets[self.operand(code, 0) as usize];
                    let mut i = c;
                    while i > 0 {
                        let ch = self.forwardcharnext();
                        if !set.char_in(ch) {
                            self.backwardnext();
                            break;
                        }
                        i -= 1;
                    }
                    if c > i {
                        let pos = self.text_pos() - self.bump();
                        self.track_push2(c - i - 1, pos);
                    }
                    self.advance(code, 2);
                    false
                }
                ONELOOP_BACK | NOTONELOOP_BACK | SETLOOP_BACK => {
                    self.track_pop_n(2);
                    let i = self.track_peek();
                    let pos = self.track_peek_n(1);
                    self.textto(pos);
                    if i > 0 {
                        let p = pos - self.bump();
                        self.track_push2(i - 1, p);
                    }
                    self.advance(code, 2);
                    false
                }
                ONELAZY | NOTONELAZY | SETLAZY => {
                    let mut c = self.operand(code, 1);
                    if c > self.forwardchars() {
                        c = self.forwardchars();
                    }
                    if c > 0 {
                        let pos = self.text_pos();
                        self.track_push2(c - 1, pos);
                    }
                    self.advance(code, 2);
                    false
                }
                ONELAZY_BACK | NOTONELAZY_BACK | SETLAZY_BACK => {
                    self.track_pop_n(2);
                    let pos = self.track_peek_n(1);
                    self.textto(pos);
                    let ch = self.forwardcharnext();
                    let rejected = match self.operator {
                        ONELAZY_BACK => ch != self.operand(code, 0) as Rune,
                        NOTONELAZY_BACK => ch == self.operand(code, 0) as Rune,
                        _ => !code.sets[self.operand(code, 0) as usize].char_in(ch),
                    };
                    if rejected {
                        true
                    } else {
                        let i = self.track_peek();
                        if i > 0 {
                            let p = pos + self.bump();
                            self.track_push2(i - 1, p);
                        }
                        self.advance(code, 2);
                        false
                    }
                }
                _ => return Err(Error::UnknownRunnerState),
            };
            if backtrack {
                self.backtrack(code, debug);
            }
        }
    }

    fn ensure_storage(&mut self) {
        if self.runstackpos < self.runtrackcount * 4 {
            double_int_slice(&mut self.runstack, &mut self.runstackpos);
        }
        if self.runtrackpos < self.runtrackcount * 4 {
            double_int_slice(&mut self.runtrack, &mut self.runtrackpos);
        }
    }

    fn crawl(&mut self, i: isize) {
        if self.runcrawlpos == 0 {
            double_int_slice(&mut self.runcrawl, &mut self.runcrawlpos);
        }
        self.runcrawlpos -= 1;
        self.runcrawl[self.runcrawlpos as usize] = i;
    }

    fn popcrawl(&mut self) -> isize {
        let val = self.runcrawl[self.runcrawlpos as usize];
        self.runcrawlpos += 1;
        val
    }

    fn crawlpos(&self) -> isize {
        self.runcrawl.len() as isize - self.runcrawlpos
    }

    fn advance(&mut self, code: &Code, i: isize) {
        self.codepos += i + 1;
        self.set_operator(code.codes[self.codepos as usize]);
    }

    fn go_to(&mut self, code: &Code, newpos: isize) {
        if newpos <= self.codepos {
            self.ensure_storage();
        }
        self.set_operator(code.codes[newpos as usize]);
        self.codepos = newpos;
    }

    fn textto(&mut self, newpos: isize) {
        self.runtextpos = newpos;
    }

    fn trackto(&mut self, newpos: isize) {
        self.runtrackpos = self.runtrack.len() as isize - newpos;
    }

    fn textstart(&self) -> isize {
        self.runtextstart
    }

    fn text_pos(&self) -> isize {
        self.runtextpos
    }

    fn trackpos(&self) -> isize {
        self.runtrack.len() as isize - self.runtrackpos
    }

    fn track_set(&mut self, v: isize) {
        self.runtrackpos -= 1;
        self.runtrack[self.runtrackpos as usize] = v;
    }

    fn track_push(&mut self) {
        self.track_set(self.codepos);
    }

    fn track_push1(&mut self, i1: isize) {
        self.track_set(i1);
        self.track_set(self.codepos);
    }

    fn track_push2(&mut self, i1: isize, i2: isize) {
        self.track_set(i1);
        self.track_set(i2);
        self.track_set(self.codepos);
    }

    fn track_push3(&mut self, i1: isize, i2: isize, i3: isize) {
        self.track_set(i1);
        self.track_set(i2);
        self.track_set(i3);
        self.track_set(self.codepos);
    }

    fn track_push_neg1(&mut self, i1: isize) {
        self.track_set(i1);
        self.track_set(-self.codepos);
    }

    fn track_push_neg2(&mut self, i1: isize, i2: isize) {
        self.track_set(i1);
        self.track_set(i2);
        self.track_set(-self.codepos);
    }

    fn backtrack(&mut self, code: &Code, debug: bool) {
        let mut newpos = self.runtrack[self.runtrackpos as usize];
        self.runtrackpos += 1;
        if debug {
            if newpos < 0 {
                write_stdout(&format!(
                    "       Backtracking (back2) to code position {}\n",
                    -newpos
                ));
            } else {
                write_stdout(&format!("       Backtracking to code position {newpos}\n"));
            }
        }
        if newpos < 0 {
            newpos = -newpos;
            self.set_operator(code.codes[newpos as usize] | BACK2);
        } else {
            self.set_operator(code.codes[newpos as usize] | BACK);
        }
        if newpos < self.codepos {
            self.ensure_storage();
        }
        self.codepos = newpos;
    }

    fn set_operator(&mut self, op: InstOp) {
        self.case_insensitive = op & CI != 0;
        self.right_to_left = op & RTL != 0;
        self.operator = op & !(RTL | CI);
    }

    fn track_pop(&mut self) {
        self.runtrackpos += 1;
    }

    fn track_pop_n(&mut self, framesize: isize) {
        self.runtrackpos += framesize;
    }

    fn track_peek(&self) -> isize {
        self.runtrack[(self.runtrackpos - 1) as usize]
    }

    fn track_peek_n(&self, i: isize) -> isize {
        self.runtrack[(self.runtrackpos - i - 1) as usize]
    }

    fn stack_push(&mut self, i1: isize) {
        self.runstackpos -= 1;
        self.runstack[self.runstackpos as usize] = i1;
    }

    fn stack_push2(&mut self, i1: isize, i2: isize) {
        self.stack_push(i1);
        self.stack_push(i2);
    }

    fn stack_pop(&mut self) {
        self.runstackpos += 1;
    }

    fn stack_pop_n(&mut self, framesize: isize) {
        self.runstackpos += framesize;
    }

    fn stack_peek(&self) -> isize {
        self.runstack[(self.runstackpos - 1) as usize]
    }

    fn stack_peek_n(&self, i: isize) -> isize {
        self.runstack[(self.runstackpos - i - 1) as usize]
    }

    fn operand(&self, code: &Code, i: isize) -> isize {
        code.codes[(self.codepos + i + 1) as usize]
    }

    fn leftchars(&self) -> isize {
        self.runtextpos
    }

    fn rightchars(&self) -> isize {
        self.runtextend - self.runtextpos
    }

    fn bump(&self) -> isize {
        if self.right_to_left {
            return -1;
        }
        1
    }

    fn forwardchars(&self) -> isize {
        if self.right_to_left {
            return self.runtextpos;
        }
        self.runtextend - self.runtextpos
    }

    fn forwardcharnext(&mut self) -> Rune {
        let ch = if self.right_to_left {
            self.runtextpos -= 1;
            self.text()[self.runtextpos as usize]
        } else {
            let ch = self.text()[self.runtextpos as usize];
            self.runtextpos += 1;
            ch
        };
        if self.case_insensitive {
            return unicode::to_lower(ch);
        }
        ch
    }

    fn runematch(&mut self, str: &[Rune]) -> bool {
        let mut c = str.len() as isize;
        let mut pos: isize;
        if !self.right_to_left {
            if self.runtextend - self.runtextpos < c {
                return false;
            }
            pos = self.runtextpos + c;
        } else {
            if self.runtextpos < c {
                return false;
            }
            pos = self.runtextpos;
        }
        let text = self.text();
        if !self.case_insensitive {
            while c != 0 {
                c -= 1;
                pos -= 1;
                if str[c as usize] != text[pos as usize] {
                    return false;
                }
            }
        } else {
            while c != 0 {
                c -= 1;
                pos -= 1;
                if str[c as usize] != unicode::to_lower(text[pos as usize]) {
                    return false;
                }
            }
        }
        if !self.right_to_left {
            pos += str.len() as isize;
        }
        self.runtextpos = pos;
        true
    }

    fn refmatch(&mut self, index: isize, len: isize) -> bool {
        let mut pos: isize;
        if !self.right_to_left {
            if self.runtextend - self.runtextpos < len {
                return false;
            }
            pos = self.runtextpos + len;
        } else {
            if self.runtextpos < len {
                return false;
            }
            pos = self.runtextpos;
        }
        let mut cmpos = index + len;
        let mut c = len;
        let text = self.text();
        if !self.case_insensitive {
            while c != 0 {
                c -= 1;
                cmpos -= 1;
                pos -= 1;
                if text[cmpos as usize] != text[pos as usize] {
                    return false;
                }
            }
        } else {
            while c != 0 {
                c -= 1;
                cmpos -= 1;
                pos -= 1;
                if unicode::to_lower(text[cmpos as usize]) != unicode::to_lower(text[pos as usize])
                {
                    return false;
                }
            }
        }
        if !self.right_to_left {
            pos += len;
        }
        self.runtextpos = pos;
        true
    }

    fn backwardnext(&mut self) {
        if self.right_to_left {
            self.runtextpos += 1;
        } else {
            self.runtextpos -= 1;
        }
    }

    fn char_at(&self, j: isize) -> Rune {
        self.text()[j as usize]
    }

    fn find_first_char(&mut self, code: &Code) -> bool {
        let anchors = code.anchors;
        if 0 != anchors & (ANCHOR_BEGINNING | ANCHOR_START | ANCHOR_END_Z | ANCHOR_END) {
            if !code.right_to_left {
                if (0 != anchors & ANCHOR_BEGINNING && self.runtextpos > 0)
                    || (0 != anchors & ANCHOR_START && self.runtextpos > self.runtextstart)
                {
                    self.runtextpos = self.runtextend;
                    return false;
                }
                if 0 != anchors & ANCHOR_END_Z && self.runtextpos < self.runtextend - 1 {
                    self.runtextpos = self.runtextend - 1;
                } else if 0 != anchors & ANCHOR_END && self.runtextpos < self.runtextend {
                    self.runtextpos = self.runtextend;
                }
            } else {
                if (0 != anchors & ANCHOR_END && self.runtextpos < self.runtextend)
                    || (0 != anchors & ANCHOR_END_Z
                        && (self.runtextpos < self.runtextend - 1
                            || (self.runtextpos == self.runtextend - 1
                                && self.char_at(self.runtextpos) != '\n' as Rune)))
                    || (0 != anchors & ANCHOR_START && self.runtextpos < self.runtextstart)
                {
                    self.runtextpos = 0;
                    return false;
                }
                if 0 != anchors & ANCHOR_BEGINNING && self.runtextpos > 0 {
                    self.runtextpos = 0;
                }
            }
            if let Some(bm) = &code.bm_prefix {
                return bm.is_match(self.text(), self.runtextpos, 0, self.runtextend);
            }
            return true;
        } else if let Some(bm) = &code.bm_prefix {
            self.runtextpos = bm.scan(self.text(), self.runtextpos, 0, self.runtextend);
            if self.runtextpos == -1 {
                if code.right_to_left {
                    self.runtextpos = 0;
                } else {
                    self.runtextpos = self.runtextend;
                }
                return false;
            }
            return true;
        }
        let Some(fc) = &code.fc_prefix else {
            return true;
        };
        self.right_to_left = code.right_to_left;
        self.case_insensitive = fc.case_insensitive;
        let set = &fc.prefix_set;
        if set.is_singleton() {
            let ch = set.singleton_char();
            let mut i = self.forwardchars();
            while i > 0 {
                if ch == self.forwardcharnext() {
                    self.backwardnext();
                    return true;
                }
                i -= 1;
            }
        } else {
            let mut i = self.forwardchars();
            while i > 0 {
                let n = self.forwardcharnext();
                if set.char_in(n) {
                    self.backwardnext();
                    return true;
                }
                i -= 1;
            }
        }
        false
    }

    fn init_match(&mut self, re: &Regexp) {
        match &mut self.runmatch {
            None => self.runmatch = Some(MatchState::new(re.capsize())),
            Some(m) => m.reset(),
        }
        if !self.runcrawl.is_empty() {
            self.runtrackpos = self.runtrack.len() as isize;
            self.runstackpos = self.runstack.len() as isize;
            self.runcrawlpos = self.runcrawl.len() as isize;
            return;
        }
        self.init_track_count(re.code());
        let mut tracksize = self.runtrackcount * 8;
        let mut stacksize = self.runtrackcount * 8;
        if tracksize < 32 {
            tracksize = 32;
        }
        if stacksize < 16 {
            stacksize = 16;
        }
        self.runtrack = vec![0; tracksize as usize];
        self.runtrackpos = tracksize;
        self.runstack = vec![0; stacksize as usize];
        self.runstackpos = stacksize;
        self.runcrawl = vec![0; 32];
        self.runcrawlpos = 32;
    }

    fn tidy_match(&mut self, quick: bool) -> Scanned {
        if !quick {
            let state = self.runmatch.take().expect("an initialized match");
            return Scanned::Found(state, self.runtextpos);
        }
        Scanned::Quick
    }

    fn capture(&mut self, capnum: isize, start: isize, end: isize) {
        let (start, end) = if end < start {
            (end, start)
        } else {
            (start, end)
        };
        self.crawl(capnum);
        self.runmatch_mut().add_match(capnum, start, end - start);
    }

    fn transfer_capture(&mut self, capnum: isize, uncapnum: isize, start: isize, end: isize) {
        let (mut start, mut end) = if end < start {
            (end, start)
        } else {
            (start, end)
        };
        let start2 = self.runmatch().match_index(uncapnum);
        let end2 = start2 + self.runmatch().match_length(uncapnum);
        if start >= end2 {
            end = start;
            start = end2;
        } else if end <= start2 {
            start = start2;
        } else {
            if end > end2 {
                end = end2;
            }
            if start2 > start {
                start = start2;
            }
        }
        self.crawl(uncapnum);
        self.runmatch_mut().balance_match(uncapnum);
        if capnum != -1 {
            self.crawl(capnum);
            self.runmatch_mut().add_match(capnum, start, end - start);
        }
    }

    fn uncapture(&mut self) {
        let capnum = self.popcrawl();
        self.runmatch_mut().remove_match(capnum);
    }

    fn dump_state(&self, code: &Code) {
        let mut back = String::new();
        if self.operator & BACK != 0 {
            back.push_str(" Back");
        }
        if self.operator & BACK2 != 0 {
            back.push_str(" Back2");
        }
        let mut out = b"Text:  ".to_vec();
        out.extend_from_slice(&self.textpos_description());
        out.extend_from_slice(b"\nTrack: ");
        out.extend_from_slice(stack_description(&self.runtrack, self.runtrackpos).as_bytes());
        out.extend_from_slice(b"\nStack: ");
        out.extend_from_slice(stack_description(&self.runstack, self.runstackpos).as_bytes());
        out.extend_from_slice(b"\n       ");
        out.extend_from_slice(code.opcode_description(self.codepos as usize).as_bytes());
        out.extend_from_slice(back.as_bytes());
        out.extend_from_slice(b"\n\n");
        use std::io::Write;
        let _ = std::io::stdout().write_all(&out);
    }

    fn textpos_description(&self) -> Vec<u8> {
        let mut buf = self.runtextpos.to_string().into_bytes();
        if buf.len() < 8 {
            buf.resize(8, b' ');
        }
        if self.runtextpos > 0 {
            buf.extend_from_slice(
                char_description(self.text()[(self.runtextpos - 1) as usize]).as_bytes(),
            );
        } else {
            buf.push(b'^');
        }
        buf.push(b'>');
        for i in self.runtextpos..self.runtextend {
            buf.extend_from_slice(char_description(self.text()[i as usize]).as_bytes());
        }
        if buf.len() >= 64 {
            buf.truncate(61);
            buf.extend_from_slice(b"...");
        } else {
            buf.push(b'$');
        }
        buf
    }

    fn is_boundary(&self, index: isize, startpos: isize, endpos: isize) -> bool {
        (index > startpos && is_word_char(self.text()[(index - 1) as usize]))
            != (index < endpos && is_word_char(self.text()[index as usize]))
    }

    fn is_ecma_boundary(&self, index: isize, startpos: isize, endpos: isize) -> bool {
        (index > startpos && is_ecma_word_char(self.text()[(index - 1) as usize]))
            != (index < endpos && is_ecma_word_char(self.text()[index as usize]))
    }

    fn init_track_count(&mut self, code: &Code) {
        self.runtrackcount = code.track_count;
    }
}

fn double_int_slice(s: &mut Vec<isize>, pos: &mut isize) {
    let old_len = s.len();
    let mut new_s = vec![0; old_len * 2];
    new_s[old_len..].copy_from_slice(s);
    *pos += old_len as isize;
    *s = new_s;
}

fn stack_description(a: &[isize], index: isize) -> String {
    let mut buf = format!("{}/{}", a.len() as isize - index, a.len());
    if buf.len() < 8 {
        let pad = 8 - buf.len();
        buf.push_str(&" ".repeat(pad));
    }
    buf.push('(');
    let mut i = index;
    while i < a.len() as isize {
        if i > index {
            buf.push(' ');
        }
        buf.push_str(&a[i as usize].to_string());
        i += 1;
    }
    buf.push(')');
    buf
}
