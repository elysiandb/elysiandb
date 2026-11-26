use super::parse::{Error, FOLD_CASE, Flags, NON_GREEDY};
use super::prog::{
    EMPTY_BEGIN_LINE, EMPTY_BEGIN_TEXT, EMPTY_END_LINE, EMPTY_END_TEXT, EMPTY_NO_WORD_BOUNDARY,
    EMPTY_WORD_BOUNDARY, EmptyOp, Inst, InstOp, Prog,
};
use super::regexp::{Op, Regexp};
use gocompat::unicode;
use gocompat::utf8::Rune;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default)]
struct PatchList {
    head: u32,
    tail: u32,
}

fn make_patch_list(n: u32) -> PatchList {
    PatchList { head: n, tail: n }
}

impl PatchList {
    fn patch(self, p: &mut Prog, val: u32) {
        let mut head = self.head;
        while head != 0 {
            let i = &mut p.inst[(head >> 1) as usize];
            if head & 1 == 0 {
                head = i.out;
                i.out = val;
            } else {
                head = i.arg;
                i.arg = val;
            }
        }
    }

    fn append(self, p: &mut Prog, l2: PatchList) -> PatchList {
        if self.head == 0 {
            return l2;
        }
        if l2.head == 0 {
            return self;
        }
        let i = &mut p.inst[(self.tail >> 1) as usize];
        if self.tail & 1 == 0 {
            i.out = l2.head;
        } else {
            i.arg = l2.head;
        }
        PatchList {
            head: self.head,
            tail: l2.tail,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Frag {
    i: u32,
    out: PatchList,
    nullable: bool,
}

struct Compiler {
    p: Prog,
    any_rune_not_nl: Arc<[Rune]>,
    any_rune: Arc<[Rune]>,
}

pub fn compile(re: &Regexp) -> Result<Prog, Error> {
    let mut c = Compiler {
        p: Prog {
            inst: Vec::new(),
            start: 0,
            num_cap: 2,
        },
        any_rune_not_nl: Arc::from(&[0, '\n' as Rune - 1, '\n' as Rune + 1, unicode::MAX_RUNE][..]),
        any_rune: Arc::from(&[0, unicode::MAX_RUNE][..]),
    };
    c.inst(InstOp::Fail);
    let f = c.compile(re);
    let m = c.inst(InstOp::Match).i;
    f.out.patch(&mut c.p, m);
    c.p.start = f.i as usize;
    Ok(c.p)
}

impl Compiler {
    fn compile(&mut self, re: &Regexp) -> Frag {
        match re.op {
            Op::NoMatch => self.fail(),
            Op::EmptyMatch => self.nop(),
            Op::Literal => {
                if re.rune.is_empty() {
                    return self.nop();
                }
                let mut f = Frag::default();
                for j in 0..re.rune.len() {
                    let runes = if re.rune.len() == 1 {
                        re.rune.clone()
                    } else {
                        Arc::from(&re.rune[j..j + 1])
                    };
                    let f1 = self.rune(runes, re.flags);
                    if j == 0 {
                        f = f1;
                    } else {
                        f = self.cat(f, f1);
                    }
                }
                f
            }
            Op::CharClass => self.rune(re.rune.clone(), re.flags),
            Op::AnyCharNotNL => self.rune(self.any_rune_not_nl.clone(), 0),
            Op::AnyChar => self.rune(self.any_rune.clone(), 0),
            Op::BeginLine => self.empty(EMPTY_BEGIN_LINE),
            Op::EndLine => self.empty(EMPTY_END_LINE),
            Op::BeginText => self.empty(EMPTY_BEGIN_TEXT),
            Op::EndText => self.empty(EMPTY_END_TEXT),
            Op::WordBoundary => self.empty(EMPTY_WORD_BOUNDARY),
            Op::NoWordBoundary => self.empty(EMPTY_NO_WORD_BOUNDARY),
            Op::Capture => {
                let bra = self.cap((re.cap << 1) as u32);
                let sub = self.compile(&re.sub[0]);
                let ket = self.cap(((re.cap << 1) | 1) as u32);
                let f = self.cat(bra, sub);
                self.cat(f, ket)
            }
            Op::Star => {
                let f = self.compile(&re.sub[0]);
                self.star(f, re.flags & NON_GREEDY != 0)
            }
            Op::Plus => {
                let f = self.compile(&re.sub[0]);
                self.plus(f, re.flags & NON_GREEDY != 0)
            }
            Op::Quest => {
                let f = self.compile(&re.sub[0]);
                self.quest(f, re.flags & NON_GREEDY != 0)
            }
            Op::Concat => {
                if re.sub.is_empty() {
                    return self.nop();
                }
                let mut f = Frag::default();
                for (i, sub) in re.sub.iter().enumerate() {
                    if i == 0 {
                        f = self.compile(sub);
                    } else {
                        let f2 = self.compile(sub);
                        f = self.cat(f, f2);
                    }
                }
                f
            }
            Op::Alternate => {
                let mut f = Frag::default();
                for sub in &re.sub {
                    let f2 = self.compile(sub);
                    f = self.alt(f, f2);
                }
                f
            }
            _ => panic!("unhandled case in regular expression compilation"),
        }
    }

    fn inst(&mut self, op: InstOp) -> Frag {
        let f = Frag {
            i: self.p.inst.len() as u32,
            out: PatchList::default(),
            nullable: true,
        };
        self.p.inst.push(Inst::new(op));
        f
    }

    fn nop(&mut self) -> Frag {
        let mut f = self.inst(InstOp::Nop);
        f.out = make_patch_list(f.i << 1);
        f
    }

    fn fail(&mut self) -> Frag {
        Frag::default()
    }

    fn cap(&mut self, arg: u32) -> Frag {
        let mut f = self.inst(InstOp::Capture);
        f.out = make_patch_list(f.i << 1);
        self.p.inst[f.i as usize].arg = arg;
        if self.p.num_cap < arg as usize + 1 {
            self.p.num_cap = arg as usize + 1;
        }
        f
    }

    fn cat(&mut self, f1: Frag, f2: Frag) -> Frag {
        if f1.i == 0 || f2.i == 0 {
            return Frag::default();
        }
        f1.out.patch(&mut self.p, f2.i);
        Frag {
            i: f1.i,
            out: f2.out,
            nullable: f1.nullable && f2.nullable,
        }
    }

    fn alt(&mut self, f1: Frag, f2: Frag) -> Frag {
        if f1.i == 0 {
            return f2;
        }
        if f2.i == 0 {
            return f1;
        }
        let mut f = self.inst(InstOp::Alt);
        let i = &mut self.p.inst[f.i as usize];
        i.out = f1.i;
        i.arg = f2.i;
        f.out = f1.out.append(&mut self.p, f2.out);
        f.nullable = f1.nullable || f2.nullable;
        f
    }

    fn quest(&mut self, f1: Frag, nongreedy: bool) -> Frag {
        let mut f = self.inst(InstOp::Alt);
        let i = &mut self.p.inst[f.i as usize];
        if nongreedy {
            i.arg = f1.i;
            f.out = make_patch_list(f.i << 1);
        } else {
            i.out = f1.i;
            f.out = make_patch_list((f.i << 1) | 1);
        }
        f.out = f.out.append(&mut self.p, f1.out);
        f
    }

    fn loop_(&mut self, f1: Frag, nongreedy: bool) -> Frag {
        let mut f = self.inst(InstOp::Alt);
        let i = &mut self.p.inst[f.i as usize];
        if nongreedy {
            i.arg = f1.i;
            f.out = make_patch_list(f.i << 1);
        } else {
            i.out = f1.i;
            f.out = make_patch_list((f.i << 1) | 1);
        }
        f1.out.patch(&mut self.p, f.i);
        f
    }

    fn star(&mut self, f1: Frag, nongreedy: bool) -> Frag {
        if f1.nullable {
            let p = self.plus(f1, nongreedy);
            return self.quest(p, nongreedy);
        }
        self.loop_(f1, nongreedy)
    }

    fn plus(&mut self, f1: Frag, nongreedy: bool) -> Frag {
        let out = self.loop_(f1, nongreedy).out;
        Frag {
            i: f1.i,
            out,
            nullable: f1.nullable,
        }
    }

    fn empty(&mut self, op: EmptyOp) -> Frag {
        let mut f = self.inst(InstOp::EmptyWidth);
        self.p.inst[f.i as usize].arg = u32::from(op);
        f.out = make_patch_list(f.i << 1);
        f
    }

    fn rune(&mut self, r: Arc<[Rune]>, flags: Flags) -> Frag {
        let mut f = self.inst(InstOp::Rune);
        f.nullable = false;
        let mut flags = flags & FOLD_CASE;
        if r.len() != 1 || unicode::simple_fold(r[0]) == r[0] {
            flags &= !FOLD_CASE;
        }
        let op = if flags & FOLD_CASE == 0 && (r.len() == 1 || r.len() == 2 && r[0] == r[1]) {
            InstOp::Rune1
        } else if r.len() == 2 && r[0] == 0 && r[1] == unicode::MAX_RUNE {
            InstOp::RuneAny
        } else if r.len() == 4
            && r[0] == 0
            && r[1] == '\n' as Rune - 1
            && r[2] == '\n' as Rune + 1
            && r[3] == unicode::MAX_RUNE
        {
            InstOp::RuneAnyNotNL
        } else {
            InstOp::Rune
        };
        let i = &mut self.p.inst[f.i as usize];
        i.rune = r;
        i.arg = u32::from(flags);
        i.op = op;
        f.out = make_patch_list(f.i << 1);
        f
    }
}
