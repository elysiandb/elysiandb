use super::parse::{Flags, NON_GREEDY};
use super::regexp::{Op, Regexp};
use std::sync::Arc;

impl Regexp {
    pub fn simplify(self: &Arc<Regexp>) -> Arc<Regexp> {
        match self.op {
            Op::Capture | Op::Concat | Op::Alternate => {
                let mut nsubs: Option<Vec<Arc<Regexp>>> = None;
                for (i, sub) in self.sub.iter().enumerate() {
                    let nsub = sub.simplify();
                    if nsubs.is_none() && !Arc::ptr_eq(&nsub, sub) {
                        nsubs = Some(self.sub[..i].to_vec());
                    }
                    if let Some(list) = &mut nsubs {
                        list.push(nsub);
                    }
                }
                match nsubs {
                    None => self.clone(),
                    Some(list) => Arc::new(Regexp {
                        op: self.op,
                        flags: self.flags,
                        sub: list,
                        rune: Arc::from(Vec::new()),
                        min: self.min,
                        max: self.max,
                        cap: self.cap,
                        name: self.name.clone(),
                    }),
                }
            }
            Op::Star | Op::Plus | Op::Quest => {
                let sub = self.sub[0].simplify();
                simplify1(self.op, self.flags, sub, Some(self))
            }
            Op::Repeat => {
                if self.min == 0 && self.max == 0 {
                    return Arc::new(Regexp::new(Op::EmptyMatch));
                }
                let sub = self.sub[0].simplify();
                if self.max == -1 {
                    if self.min == 0 {
                        return simplify1(Op::Star, self.flags, sub, None);
                    }
                    if self.min == 1 {
                        return simplify1(Op::Plus, self.flags, sub, None);
                    }
                    let mut nre = Regexp::new(Op::Concat);
                    for _ in 0..self.min - 1 {
                        nre.sub.push(sub.clone());
                    }
                    nre.sub.push(simplify1(Op::Plus, self.flags, sub, None));
                    return Arc::new(nre);
                }
                if self.min == 1 && self.max == 1 {
                    return sub;
                }
                let mut prefix: Option<Regexp> = None;
                if self.min > 0 {
                    let mut p = Regexp::new(Op::Concat);
                    for _ in 0..self.min {
                        p.sub.push(sub.clone());
                    }
                    prefix = Some(p);
                }
                if self.max > self.min {
                    let mut suffix = simplify1(Op::Quest, self.flags, sub.clone(), None);
                    for _ in self.min + 1..self.max {
                        let mut nre2 = Regexp::new(Op::Concat);
                        nre2.sub.push(sub.clone());
                        nre2.sub.push(suffix);
                        suffix = simplify1(Op::Quest, self.flags, Arc::new(nre2), None);
                    }
                    match &mut prefix {
                        None => return suffix,
                        Some(p) => p.sub.push(suffix),
                    }
                }
                if let Some(p) = prefix {
                    return Arc::new(p);
                }
                Arc::new(Regexp::new(Op::NoMatch))
            }
            _ => self.clone(),
        }
    }
}

fn simplify1(op: Op, flags: Flags, sub: Arc<Regexp>, re: Option<&Arc<Regexp>>) -> Arc<Regexp> {
    if sub.op == Op::EmptyMatch {
        return sub;
    }
    if op == sub.op && flags & NON_GREEDY == sub.flags & NON_GREEDY {
        return sub;
    }
    if let Some(re) = re
        && re.op == op
        && re.flags & NON_GREEDY == flags & NON_GREEDY
        && Arc::ptr_eq(&sub, &re.sub[0])
    {
        return re.clone();
    }
    let mut nre = Regexp::new(op);
    nre.flags = flags;
    nre.sub.push(sub);
    Arc::new(nre)
}
