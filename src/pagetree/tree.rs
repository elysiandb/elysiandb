use std::fmt;
use std::ops::Deref;
use std::panic::panic_any;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use arc_swap::Guard;

use super::page::{MAX_BYTES, MAX_ENTRIES, MIN_ENTRIES, Page, Ref, Token};
use crate::gocompat::errors::{self, BoxError};

pub trait Loader: Send + Sync {
    fn load(&self, r: &Arc<Ref>) -> Result<Arc<Page>, BoxError>;

    fn peek(&self, _r: &Arc<Ref>) -> Option<Result<Arc<Page>, BoxError>> {
        None
    }
}

pub struct PageRef(Guard<Option<Arc<Page>>>);

impl PageRef {
    fn resident(g: Guard<Option<Arc<Page>>>) -> Option<PageRef> {
        if g.is_some() { Some(PageRef(g)) } else { None }
    }

    fn owned(p: Arc<Page>) -> PageRef {
        PageRef(Guard::from_inner(Some(p)))
    }

    pub fn into_arc(self) -> Arc<Page> {
        match Guard::into_inner(self.0) {
            Some(p) => p,
            None => unreachable!(),
        }
    }
}

impl Deref for PageRef {
    type Target = Page;

    fn deref(&self) -> &Page {
        match &*self.0 {
            Some(p) => p,
            None => unreachable!(),
        }
    }
}

#[derive(Clone)]
pub struct Slice {
    page: Arc<Page>,
    val: bool,
    start: u32,
    end: u32,
}

impl Slice {
    fn key(page: &Arc<Page>, i: usize) -> Slice {
        let (start, end) = page.key_range(i);
        Slice {
            page: page.clone(),
            val: false,
            start,
            end,
        }
    }

    fn val(page: &Arc<Page>, i: usize) -> Slice {
        let (start, end) = page.val_range(i);
        Slice {
            page: page.clone(),
            val: true,
            start,
            end,
        }
    }
}

impl Deref for Slice {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        let b = if self.val {
            &self.page.vals
        } else {
            &self.page.keys
        };
        &b[self.start as usize..self.end as usize]
    }
}

impl AsRef<[u8]> for Slice {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl fmt::Debug for Slice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

pub enum UpsertOut {
    InPlace,
    Replace(Vec<u8>),
}

struct Owned {
    page: Option<Arc<Page>>,
    r: Arc<Ref>,
}

impl Owned {
    fn page(&mut self) -> &mut Page {
        match self.page.as_mut().and_then(Arc::get_mut) {
            Some(p) => p,
            None => panic_any(String::from(
                "pagetree: a page owned by the batch is shared",
            )),
        }
    }

    fn finish(mut self) -> Arc<Ref> {
        if let Some(p) = self.page.take() {
            self.r.mem.store(Some(p));
        }
        self.r.clone()
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if let Some(p) = self.page.take() {
            self.r.mem.store(Some(p));
        }
    }
}

type Visit<'a> = dyn FnMut(&[u8], &[u8]) -> bool + 'a;

type DirtyVisit<'a> = dyn FnMut(&Arc<Ref>, &Arc<Page>) -> Result<(), BoxError> + 'a;

#[derive(Clone, Default)]
pub struct Tree {
    root: Option<Arc<Ref>>,
    ld: Option<Arc<dyn Loader>>,
    tok: Option<Token>,
}

impl Tree {
    pub fn new(ld: Option<Arc<dyn Loader>>) -> Tree {
        Tree {
            root: None,
            ld,
            tok: None,
        }
    }

    pub fn open(ld: Option<Arc<dyn Loader>>, root: Option<Arc<Ref>>) -> Tree {
        Tree {
            root,
            ld,
            tok: None,
        }
    }

    pub fn with_token(&self, tok: Option<&Token>) -> Tree {
        Tree {
            root: self.root.clone(),
            ld: self.ld.clone(),
            tok: tok.cloned(),
        }
    }

    pub fn root(&self) -> Option<&Arc<Ref>> {
        self.root.as_ref()
    }

    pub fn loader(&self) -> Option<&Arc<dyn Loader>> {
        self.ld.as_ref()
    }

    fn fetch(&self, r: &Arc<Ref>) -> PageRef {
        if !r.hot.load(Ordering::Relaxed) {
            r.hot.store(true, Ordering::Relaxed);
        }

        if let Some(p) = PageRef::resident(r.load()) {
            return p;
        }

        let Some(ld) = &self.ld else {
            panic_any(format!(
                "pagetree: page {} is not in memory and the tree has no loader",
                r.id()
            ));
        };

        match ld.load(r) {
            Ok(p) => PageRef::owned(p),
            Err(err) => panic_any(format!("pagetree: cannot load page {}: {}", r.id(), err)),
        }
    }

    pub fn page(&self, r: &Arc<Ref>) -> PageRef {
        self.fetch(r)
    }

    fn owner(&self) -> u64 {
        self.tok.as_ref().map_or(0, Token::id)
    }

    fn owns(&self, p: &Page) -> bool {
        match &self.tok {
            Some(tok) => tok.active() && p.owner == tok.id(),
            None => false,
        }
    }

    fn own(&self, r: &Arc<Ref>, p: PageRef) -> Owned {
        if self.owns(&p) {
            drop(p);
            let page = r.mem.swap(None);
            return Owned { page, r: r.clone() };
        }

        let c = p.clone_owned(self.owner());
        drop(p);

        Owned {
            page: Some(Arc::new(c)),
            r: Ref::empty(),
        }
    }

    fn fresh(&self, mut p: Page) -> Arc<Ref> {
        p.owner = self.owner();
        Ref::new(Arc::new(p))
    }

    pub fn len(&self) -> usize {
        match &self.root {
            None => 0,
            Some(r) => self.fetch(r).total as usize,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, key: &[u8]) -> Option<Slice> {
        let root = self.root.as_ref()?;

        let mut p = self.fetch(root);
        while !p.leaf {
            let next = self.fetch(&p.kids[p.child_index(key)]);
            p = next;
        }

        let (i, found) = p.search(key);
        if !found {
            return None;
        }

        Some(Slice::val(&p.into_arc(), i))
    }

    fn grow(&self, r: Arc<Ref>, split: Option<Arc<Ref>>) -> Arc<Ref> {
        let Some(split) = split else {
            return r;
        };

        let left = self.fetch(&r);
        let right = self.fetch(&split);
        let mut root = Page::internal(self.owner());
        root.insert_key(0, left.min_key());
        root.insert_key(1, right.min_key());
        root.sizes = vec![left.total, right.total];
        root.total = left.total + right.total;
        drop(left);
        drop(right);
        root.kids = vec![r, split];

        self.fresh(root)
    }

    fn link(&self, c: &mut Page, i: usize, split: Option<&Arc<Ref>>) {
        if let Some(split) = split {
            let sp = self.fetch(split);
            c.insert_key(i + 1, sp.min_key());
            c.kids.insert(i + 1, split.clone());
            c.sizes.insert(i + 1, sp.total);
        }
    }

    fn split_if_full(&self, mut c: Owned, replaced: bool) -> (Arc<Ref>, Option<Arc<Ref>>, bool) {
        let cp = c.page();
        if !cp.full() {
            return (c.finish(), None, replaced);
        }

        let right = cp.split(self.owner());

        (c.finish(), Some(self.fresh(right)), replaced)
    }

    pub fn put(&self, key: &[u8], val: &[u8]) -> (Tree, bool) {
        let mut t = self.clone();
        let Some(root) = &self.root else {
            let mut p = Page::empty_leaf(self.owner());
            p.insert_key(0, key);
            p.insert_val(0, val);
            p.total = 1;
            t.root = Some(self.fresh(p));
            return (t, false);
        };

        let (r, split, replaced) = self.put_at(root, key, val);
        t.root = Some(self.grow(r, split));

        (t, replaced)
    }

    fn put_at(&self, r: &Arc<Ref>, key: &[u8], val: &[u8]) -> (Arc<Ref>, Option<Arc<Ref>>, bool) {
        let p = self.fetch(r);
        if p.leaf {
            let (i, found) = p.search(key);
            let mut c = self.own(r, p);
            let cp = c.page();
            if found {
                cp.set_val(i, val);
                return (c.finish(), None, true);
            }

            cp.insert_key(i, key);
            cp.insert_val(i, val);
            cp.total += 1;

            return self.split_if_full(c, false);
        }

        let i = p.child_index(key);
        let child = p.kids[i].clone();
        let (kid, split, replaced) = self.put_at(&child, key, val);

        let mut c = self.own(r, p);
        let cp = c.page();
        {
            let kp = self.fetch(&kid);
            cp.sizes[i] = kp.total;
            cp.set_key(i, kp.min_key());
        }
        cp.kids[i] = kid.clone();
        if !replaced {
            cp.total += 1;
        }
        self.link(cp, i, split.as_ref());

        self.split_if_full(c, replaced)
    }

    pub fn upsert<F>(&self, key: &[u8], f: F) -> Tree
    where
        F: FnOnce(Option<&mut [u8]>) -> UpsertOut,
    {
        let mut t = self.clone();
        let Some(root) = &self.root else {
            let v = match f(None) {
                UpsertOut::Replace(v) => v,
                UpsertOut::InPlace => Vec::new(),
            };
            let mut p = Page::empty_leaf(self.owner());
            p.insert_key(0, key);
            p.insert_val(0, &v);
            p.total = 1;
            t.root = Some(self.fresh(p));
            return t;
        };

        let (r, split, _) = self.upsert_at(root, key, f);
        t.root = Some(self.grow(r, split));

        t
    }

    fn upsert_at<F>(&self, r: &Arc<Ref>, key: &[u8], f: F) -> (Arc<Ref>, Option<Arc<Ref>>, bool)
    where
        F: FnOnce(Option<&mut [u8]>) -> UpsertOut,
    {
        let p = self.fetch(r);
        if p.leaf {
            let (i, found) = p.search(key);
            let mut c = self.own(r, p);
            let cp = c.page();
            if found {
                if let UpsertOut::Replace(next) = f(Some(cp.val_mut(i))) {
                    cp.set_val(i, &next);
                }
            } else {
                let v = match f(None) {
                    UpsertOut::Replace(v) => v,
                    UpsertOut::InPlace => Vec::new(),
                };
                cp.insert_key(i, key);
                cp.insert_val(i, &v);
                cp.total += 1;
            }

            return self.split_if_full(c, found);
        }

        let i = p.child_index(key);
        let child = p.kids[i].clone();
        let (kid, split, found) = self.upsert_at(&child, key, f);

        if Arc::ptr_eq(&kid, &child) && split.is_none() && found && self.owns(&p) {
            return (r.clone(), None, true);
        }

        let mut c = self.own(r, p);
        let cp = c.page();
        {
            let kp = self.fetch(&kid);
            cp.sizes[i] = kp.total;
            cp.set_key(i, kp.min_key());
        }
        cp.kids[i] = kid.clone();
        if !found {
            cp.total += 1;
        }
        self.link(cp, i, split.as_ref());

        self.split_if_full(c, found)
    }

    pub fn update<F>(&self, key: &[u8], f: F) -> (Tree, bool)
    where
        F: FnOnce(&mut [u8]) -> UpsertOut,
    {
        let Some(root) = &self.root else {
            return (self.clone(), false);
        };

        let (r, split, found) = self.update_at(root, key, f);
        if !found {
            return (self.clone(), false);
        }

        let mut t = self.clone();
        t.root = Some(self.grow(r, split));

        (t, true)
    }

    fn update_at<F>(&self, r: &Arc<Ref>, key: &[u8], f: F) -> (Arc<Ref>, Option<Arc<Ref>>, bool)
    where
        F: FnOnce(&mut [u8]) -> UpsertOut,
    {
        let p = self.fetch(r);
        if p.leaf {
            let (i, found) = p.search(key);
            if !found {
                return (r.clone(), None, false);
            }

            let mut c = self.own(r, p);
            let cp = c.page();
            if let UpsertOut::Replace(next) = f(cp.val_mut(i)) {
                cp.set_val(i, &next);
            }

            return self.split_if_full(c, true);
        }

        let i = p.child_index(key);
        let child = p.kids[i].clone();
        let (kid, split, found) = self.update_at(&child, key, f);
        if !found {
            return (r.clone(), None, false);
        }

        if Arc::ptr_eq(&kid, &child) && split.is_none() && self.owns(&p) {
            return (r.clone(), None, true);
        }

        let mut c = self.own(r, p);
        let cp = c.page();
        cp.sizes[i] = self.fetch(&kid).total;
        cp.kids[i] = kid.clone();
        self.link(cp, i, split.as_ref());

        self.split_if_full(c, true)
    }

    pub fn delete(&self, key: &[u8]) -> (Tree, bool) {
        let Some(root) = &self.root else {
            return (self.clone(), false);
        };

        let (r, removed) = self.del_at(root, key);
        if !removed {
            return (self.clone(), false);
        }

        let mut t = self.clone();
        let Some(mut r) = r else {
            t.root = None;
            return (t, true);
        };

        let mut p = self.fetch(&r);
        if p.total == 0 {
            t.root = None;
            return (t, true);
        }

        while !p.leaf && p.kids.len() == 1 {
            let next = p.kids[0].clone();
            p = self.fetch(&next);
            r = next;
        }
        drop(p);

        t.root = Some(r);

        (t, true)
    }

    fn del_at(&self, r: &Arc<Ref>, key: &[u8]) -> (Option<Arc<Ref>>, bool) {
        let p = self.fetch(r);
        if p.leaf {
            let (i, found) = p.search(key);
            if !found {
                return (Some(r.clone()), false);
            }

            if p.len() == 1 {
                return (None, true);
            }

            let mut c = self.own(r, p);
            let cp = c.page();
            cp.remove_key(i);
            cp.remove_val(i);
            cp.total -= 1;

            return (Some(c.finish()), true);
        }

        let i = p.child_index(key);
        let child = p.kids[i].clone();
        let (kid, removed) = self.del_at(&child, key);
        if !removed {
            return (Some(r.clone()), false);
        }

        let mut c = self.own(r, p);
        let cp = c.page();
        cp.total = cp.total.wrapping_sub(1);

        let Some(kid) = kid else {
            if cp.kids.len() == 1 {
                return (None, true);
            }

            cp.remove_key(i);
            cp.kids.remove(i);
            cp.sizes.remove(i);

            return (Some(c.finish()), true);
        };

        let small = {
            let kp = self.fetch(&kid);
            cp.sizes[i] = kp.total;
            cp.set_key(i, kp.min_key());
            kp.len() < MIN_ENTRIES
        };
        cp.kids[i] = kid;

        if small {
            self.merge(cp, i);
        }

        (Some(c.finish()), true)
    }

    fn merge(&self, c: &mut Page, i: usize) {
        let mut j = i + 1;
        if j >= c.kids.len() {
            if i == 0 {
                return;
            }
            j = i - 1;
        }

        let (a, b) = if i < j { (i, j) } else { (j, i) };

        let left = self.fetch(&c.kids[a]);
        let right = self.fetch(&c.kids[b]);
        if left.len() + right.len() > MAX_ENTRIES
            || left.keys.len() + left.vals.len() + right.keys.len() + right.vals.len() > MAX_BYTES
        {
            return;
        }

        let mut merged = left.clone_owned(self.owner());
        let base = merged.keys.len() as u32;
        merged.keys.extend_from_slice(&right.keys);
        for &o in &right.koff[1..] {
            merged.koff.push(o + base);
        }

        if left.leaf {
            let vbase = merged.vals.len() as u32;
            merged.vals.extend_from_slice(&right.vals);
            for &o in &right.voff[1..] {
                merged.voff.push(o + vbase);
            }
        } else {
            merged.kids.extend(right.kids.iter().cloned());
            merged.sizes.extend_from_slice(&right.sizes);
        }
        merged.total = left.total + right.total;
        drop(left);
        drop(right);

        c.sizes[a] = merged.total;
        c.set_key(a, merged.min_key());
        c.kids[a] = self.fresh(merged);
        c.remove_key(b);
        c.kids.remove(b);
        c.sizes.remove(b);
    }

    pub fn rank(&self, key: &[u8]) -> usize {
        let Some(root) = &self.root else {
            return 0;
        };

        let mut rank = 0u64;
        let mut p = self.fetch(root);
        while !p.leaf {
            let i = p.child_index(key);
            for &s in &p.sizes[..i] {
                rank = rank.wrapping_add(s);
            }
            let next = self.fetch(&p.kids[i]);
            p = next;
        }

        let (i, _) = p.search(key);

        rank as usize + i
    }

    pub fn at(&self, index: i64) -> Option<(Slice, Slice)> {
        let root = self.root.as_ref()?;
        if index < 0 {
            return None;
        }

        let mut p = self.fetch(root);
        if index as u64 >= p.total {
            return None;
        }

        let mut idx = index as u64;
        while !p.leaf {
            let mut i = 0;
            while i + 1 < p.sizes.len() && idx >= p.sizes[i] {
                idx -= p.sizes[i];
                i += 1;
            }
            let next = self.fetch(&p.kids[i]);
            p = next;
        }

        let page = p.into_arc();
        let idx = idx as usize;

        Some((Slice::key(&page, idx), Slice::val(&page, idx)))
    }

    pub fn ascend_at(&self, index: i64, mut f: impl FnMut(&[u8], &[u8]) -> bool) {
        let Some(root) = &self.root else {
            return;
        };
        if index >= self.len() as i64 {
            return;
        }

        self.ascend_at_rec(root, index.max(0) as u64, &mut f);
    }

    fn ascend_at_rec(&self, r: &Arc<Ref>, mut index: u64, f: &mut Visit<'_>) -> bool {
        let p = self.fetch(r);
        if p.leaf {
            for i in index as usize..p.len() {
                if !f(p.key(i), p.val(i)) {
                    return false;
                }
            }
            return true;
        }

        for (kid, &size) in p.kids.iter().zip(&p.sizes) {
            if index >= size {
                index -= size;
                continue;
            }

            if !self.ascend_at_rec(kid, index, f) {
                return false;
            }
            index = 0;
        }

        true
    }

    pub fn ascend_from(&self, key: &[u8], mut f: impl FnMut(&[u8], &[u8]) -> bool) {
        let Some(root) = &self.root else {
            return;
        };

        self.ascend_from_rec(root, key, &mut f);
    }

    fn ascend_from_rec(&self, r: &Arc<Ref>, key: &[u8], f: &mut Visit<'_>) -> bool {
        let p = self.fetch(r);
        if p.leaf {
            let (start, _) = p.search(key);
            for i in start..p.len() {
                if !f(p.key(i), p.val(i)) {
                    return false;
                }
            }
            return true;
        }

        for kid in &p.kids[p.child_index(key)..] {
            if !self.ascend_from_rec(kid, key, f) {
                return false;
            }
        }

        true
    }

    pub fn descend_before(&self, key: &[u8], mut f: impl FnMut(&[u8], &[u8]) -> bool) {
        let Some(root) = &self.root else {
            return;
        };

        self.descend_before_rec(root, key, &mut f);
    }

    fn descend_before_rec(&self, r: &Arc<Ref>, key: &[u8], f: &mut Visit<'_>) -> bool {
        let p = self.fetch(r);
        if p.leaf {
            let (end, _) = p.search(key);
            for i in (0..end).rev() {
                if !f(p.key(i), p.val(i)) {
                    return false;
                }
            }
            return true;
        }

        for kid in p.kids[..=p.child_index(key)].iter().rev() {
            if !self.descend_before_rec(kid, key, f) {
                return false;
            }
        }

        true
    }

    pub fn descend_at(&self, index: i64, mut f: impl FnMut(&[u8], &[u8]) -> bool) {
        let Some(root) = &self.root else {
            return;
        };
        if index < 0 {
            return;
        }

        let total = self.fetch(root).total;
        let index = if index as u64 >= total {
            total.wrapping_sub(1)
        } else {
            index as u64
        };

        self.descend_at_rec(root, index, &mut f);
    }

    fn descend_at_rec(&self, r: &Arc<Ref>, mut index: u64, f: &mut Visit<'_>) -> bool {
        let p = self.fetch(r);
        if p.leaf {
            let mut i = index as i64;
            while i >= 0 {
                if !f(p.key(i as usize), p.val(i as usize)) {
                    return false;
                }
                i -= 1;
            }
            return true;
        }

        let mut i = 0;
        while i + 1 < p.sizes.len() && index >= p.sizes[i] {
            index -= p.sizes[i];
            i += 1;
        }

        loop {
            if !self.descend_at_rec(&p.kids[i], index, f) {
                return false;
            }
            if i == 0 {
                break;
            }
            index = p.sizes[i - 1].wrapping_sub(1);
            i -= 1;
        }

        true
    }

    pub fn from_sorted<K, V>(ld: Option<Arc<dyn Loader>>, keys: &[K], vals: Option<&[V]>) -> Tree
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut t = Tree::new(ld);
        if keys.is_empty() {
            return t;
        }

        let per = MAX_ENTRIES * 3 / 4;
        let mut level: Vec<Arc<Ref>> = Vec::new();
        let mut start = 0;
        while start < keys.len() {
            let mut p = Page::empty_leaf(0);
            let mut i = start;
            while i < keys.len() && p.len() < per && p.keys.len() + p.vals.len() < MAX_BYTES / 2 {
                p.keys.extend_from_slice(keys[i].as_ref());
                p.koff.push(p.keys.len() as u32);
                if let Some(vals) = vals {
                    p.vals.extend_from_slice(vals[i].as_ref());
                }
                p.voff.push(p.vals.len() as u32);
                i += 1;
            }
            p.total = p.len() as u64;
            start += p.len();
            level.push(Ref::new(Arc::new(p)));
        }

        while level.len() > 1 {
            let mut next = Vec::new();
            for group in level.chunks(per) {
                let mut p = Page::internal(0);
                for kid in group {
                    let Some(kp) = kid.resident() else {
                        unreachable!()
                    };
                    p.keys.extend_from_slice(kp.min_key());
                    p.koff.push(p.keys.len() as u32);
                    p.kids.push(kid.clone());
                    p.sizes.push(kp.total);
                    p.total += kp.total;
                }
                next.push(Ref::new(Arc::new(p)));
            }
            level = next;
        }

        t.root = level.pop();

        t
    }

    pub fn equal(&self, o: &Tree) -> bool {
        match (&self.root, &o.root) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    pub fn for_each_dirty(
        &self,
        mut f: impl FnMut(&Arc<Ref>, &Arc<Page>) -> Result<(), BoxError>,
    ) -> Result<(), BoxError> {
        match &self.root {
            None => Ok(()),
            Some(r) => for_each_dirty(r, &mut f),
        }
    }

    fn height(&self, root: &Arc<Ref>) -> usize {
        let mut h = 1;
        let mut p = self.fetch(root);
        while !p.leaf {
            let next = self.fetch(&p.kids[0]);
            p = next;
            h += 1;
        }

        h
    }

    pub fn walk_stored(&self, mut f: impl FnMut(u64, u32)) {
        let Some(root) = &self.root else {
            return;
        };

        self.walk_stored_rec(root, self.height(root), &mut f);
    }

    fn walk_stored_rec(&self, r: &Arc<Ref>, level: usize, f: &mut dyn FnMut(u64, u32)) {
        let id = r.id();
        if id != 0 {
            f(id, r.size());
        }

        if level == 1 {
            return;
        }

        let p = self.fetch(r);
        for kid in &p.kids {
            self.walk_stored_rec(kid, level - 1, f);
        }
    }

    pub fn scan(&self, mut f: impl FnMut(&[u8], &[u8]) -> bool) {
        let Some(root) = &self.root else {
            return;
        };

        self.scan_rec(root, self.height(root), &mut f);
    }

    fn scan_rec(&self, r: &Arc<Ref>, level: usize, f: &mut Visit<'_>) -> bool {
        if level > 1 {
            let p = self.fetch(r);
            for kid in &p.kids {
                if !self.scan_rec(kid, level - 1, f) {
                    return false;
                }
            }
            return true;
        }

        let p = match PageRef::resident(r.load()) {
            Some(p) => p,
            None => match self.ld.as_ref().and_then(|ld| ld.peek(r)) {
                None => self.fetch(r),
                Some(Ok(p)) => PageRef::owned(p),
                Some(Err(err)) => {
                    panic_any(format!("pagetree: cannot read page {}: {}", r.id(), err))
                }
            },
        };

        for i in 0..p.len() {
            if !f(p.key(i), p.val(i)) {
                return false;
            }
        }

        true
    }

    pub fn preload(&self, mut victim: impl FnMut(u64) -> bool) {
        let Some(root) = &self.root else {
            return;
        };

        self.preload_rec(root, self.height(root), &mut victim);
    }

    fn preload_rec(&self, r: &Arc<Ref>, level: usize, victim: &mut dyn FnMut(u64) -> bool) {
        if level == 1 {
            let id = r.id();
            if id != 0 && victim(id) {
                drop(self.fetch(r));
            }
            return;
        }

        let p = self.fetch(r);
        for kid in &p.kids {
            self.preload_rec(kid, level - 1, victim);
        }
    }

    pub fn relocate(&self, mut victim: impl FnMut(u64) -> bool, limit: usize) -> (Tree, usize) {
        let Some(root) = &self.root else {
            return (self.clone(), 0);
        };

        let mut moved = 0;
        let mut t = self.clone();
        let height = self.height(root);
        t.root = Some(self.relocate_rec(root, height, &mut victim, &mut moved, limit));

        (t, moved)
    }

    fn relocate_rec(
        &self,
        r: &Arc<Ref>,
        level: usize,
        victim: &mut dyn FnMut(u64) -> bool,
        moved: &mut usize,
        limit: usize,
    ) -> Arc<Ref> {
        let is_victim = r.id() != 0 && victim(r.id()) && *moved < limit;
        if level == 1 && !is_victim {
            return r.clone();
        }

        let p = self.fetch(r);
        let kids = if level > 1 {
            p.kids.clone()
        } else {
            Vec::new()
        };
        let mut pending = Some(p);
        let mut c = None;
        if is_victim {
            c = pending.take().map(|p| self.own(r, p));
            *moved += 1;
        }

        for (i, kid) in kids.iter().enumerate() {
            let nk = self.relocate_rec(kid, level - 1, victim, moved, limit);
            if Arc::ptr_eq(&nk, kid) {
                continue;
            }
            if c.is_none() {
                c = pending.take().map(|p| self.own(r, p));
            }
            if let Some(c) = c.as_mut() {
                c.page().kids[i] = nk;
            }
        }
        drop(pending);

        match c {
            Some(c) => c.finish(),
            None => r.clone(),
        }
    }
}

fn for_each_dirty(r: &Arc<Ref>, f: &mut DirtyVisit<'_>) -> Result<(), BoxError> {
    if r.id() != 0 {
        return Ok(());
    }

    let Some(p) = r.resident() else {
        return Err(errors::new("pagetree: an unwritten page is not in memory"));
    };

    if !p.leaf {
        for kid in &p.kids {
            for_each_dirty(kid, f)?;
        }
    }

    f(r, &p)
}
