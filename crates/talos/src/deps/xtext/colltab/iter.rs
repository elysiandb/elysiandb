use super::collelem::Elem;
use super::weighter::Weighter;

pub struct Iter<'a, W: Weighter + ?Sized> {
    pub weighter: &'a W,
    pub elems: Vec<Elem>,
    pub n: usize,
    src: &'a [u8],
    p_end: usize,
    p_next: usize,
}

const MAX_COMBINING_CHARACTERS: usize = 30;

impl<'a, W: Weighter + ?Sized> Iter<'a, W> {
    pub fn new(weighter: &'a W, elems: Vec<Elem>) -> Self {
        Iter {
            weighter,
            elems,
            n: 0,
            src: &[],
            p_end: 0,
            p_next: 0,
        }
    }

    pub fn reset(&mut self, p: usize) {
        self.elems.clear();
        self.n = 0;
        self.p_end = p;
        self.p_next = p;
    }

    pub fn len(&self) -> usize {
        self.src.len()
    }

    pub fn is_empty(&self) -> bool {
        self.src.is_empty()
    }

    pub fn discard(&mut self) {
        self.elems.drain(..self.n);
        self.n = 0;
    }

    pub fn end(&self) -> usize {
        self.p_end
    }

    pub fn set_input(&mut self, s: &'a [u8]) {
        self.src = s;
        self.reset(0);
    }

    pub fn set_input_string(&mut self, s: &'a [u8]) {
        self.set_input(s);
    }

    fn done(&self) -> bool {
        self.p_next >= self.src.len()
    }

    fn append_next(&mut self) -> bool {
        if self.done() {
            return false;
        }
        let mut sz = self
            .weighter
            .append_next(&mut self.elems, &self.src[self.p_next..]);
        if sz == 0 {
            sz = 1;
        }
        self.p_next += sz;
        true
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> bool {
        if self.n == self.elems.len() && !self.append_next() {
            return false;
        }
        let mut prev_ccc = self.elems[self.elems.len() - 1].ccc();
        if prev_ccc == 0 {
            self.n = self.elems.len();
            self.p_end = self.p_next;
            return true;
        } else if self.elems[self.n].ccc() == 0 {
            self.n += 1;
            while self.n < self.elems.len() && self.elems[self.n].ccc() == 0 {
                self.n += 1;
            }
            self.p_end = self.p_next;
            return true;
        }
        loop {
            let p = self.elems.len();
            self.p_end = self.p_next;
            if !self.append_next() {
                break;
            }
            let ccc = self.elems[p].ccc();
            if ccc == 0 || self.elems.len() - self.n > MAX_COMBINING_CHARACTERS {
                self.n = p;
                return true;
            } else if ccc < prev_ccc {
                self.do_norm(p, ccc);
            } else {
                prev_ccc = ccc;
            }
        }
        let done = self.elems.len() != self.n;
        self.n = self.elems.len();
        done
    }

    pub fn do_norm(&mut self, mut p: usize, ccc: u8) {
        let n = self.elems.len();
        let k = p;
        p -= 1;
        while p > self.n && ccc < self.elems[p - 1].ccc() {
            p -= 1;
        }
        self.elems.extend_from_within(p..k);
        self.elems.copy_within(k.., p);
        self.elems.truncate(n);
    }
}
