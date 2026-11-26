#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::sync::Arc;

use elysiandb::durable::{DirEntry, File, Fs, FsError, OpenFlags, filepath};
use elysiandb::sync::{Mutex, MutexGuard, Stop};

pub fn err_crashed() -> FsError {
    FsError::other("memfs: the machine is down")
}

#[derive(Clone, Debug, Default)]
pub struct MemNode {
    pub data: Vec<u8>,
    pub synced: Vec<u8>,
    pub shared: Option<usize>,
}

impl MemNode {
    pub fn alias_synced(&mut self) {
        self.synced = self.data.clone();
        self.shared = Some(self.data.capacity().max(self.data.len()));
    }
}

pub struct MemState {
    nodes: Vec<MemNode>,
    names: HashMap<String, usize>,
    durable: HashMap<String, usize>,
    dirs: HashSet<String>,
    pub ops: i64,
    pub crash_at: i64,
    crashed: bool,
    quota: i64,
    used: i64,
}

pub struct MemShared {
    st: Mutex<MemState>,
    down: Stop,
}

#[derive(Clone)]
pub struct MemFs(Arc<MemShared>);

fn key(path: &Path) -> String {
    path.to_str()
        .unwrap_or_else(|| panic!("memfs paths are text: {}", path.display()))
        .to_string()
}

fn dir_of(path: &str) -> String {
    key(&filepath::dir(Path::new(path)))
}

fn base_of(path: &str) -> String {
    filepath::base(Path::new(path))
        .into_string()
        .unwrap_or_else(|p| panic!("memfs paths are text: {p:?}"))
}

fn not_exist(op: &'static str, path: &str) -> FsError {
    FsError::new(op, path, io::Error::from(io::ErrorKind::NotFound))
}

impl MemState {
    fn step(&mut self, down: &Stop) -> Result<(), FsError> {
        if self.crashed {
            return Err(err_crashed());
        }

        self.ops += 1;
        if self.crash_at > 0 && self.ops >= self.crash_at {
            self.crashed = true;
            down.stop();
            return Err(err_crashed());
        }

        Ok(())
    }

    fn add(&mut self, node: MemNode) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }
}

pub fn new_mem_fs() -> MemFs {
    MemFs(Arc::new(MemShared {
        st: Mutex::new(MemState {
            nodes: Vec::new(),
            names: HashMap::new(),
            durable: HashMap::new(),
            dirs: HashSet::new(),
            ops: 0,
            crash_at: 0,
            crashed: false,
            quota: -1,
            used: 0,
        }),
        down: Stop::new(),
    }))
}

impl MemFs {
    pub fn lock(&self) -> MutexGuard<'_, MemState> {
        self.0.st.lock()
    }

    pub fn down(&self) -> &Stop {
        &self.0.down
    }

    pub fn ops(&self) -> i64 {
        self.lock().ops
    }

    pub fn set_ops(&self, ops: i64) {
        self.lock().ops = ops;
    }

    pub fn set_crash_at(&self, at: i64) {
        self.lock().crash_at = at;
    }

    pub fn set_quota(&self, n: i64) {
        self.lock().quota = n;
    }

    pub fn usage(&self) -> i64 {
        self.lock().used
    }

    pub fn after_power_loss(&self) -> MemFs {
        let m = self.lock();
        let out = new_mem_fs();
        {
            let mut o = out.lock();
            for (name, &idx) in &m.durable {
                let n = &m.nodes[idx];
                let c = o.add(MemNode {
                    data: n.synced.clone(),
                    synced: n.synced.clone(),
                    shared: None,
                });
                o.names.insert(name.clone(), c);
                o.durable.insert(name.clone(), c);
            }
            for d in &m.dirs {
                o.dirs.insert(d.clone());
            }
        }
        out
    }

    pub fn after_process_crash(&self) -> MemFs {
        let m = self.lock();
        let out = new_mem_fs();
        {
            let mut o = out.lock();
            for (name, &idx) in &m.names {
                let n = &m.nodes[idx];
                let c = o.add(MemNode {
                    data: n.data.clone(),
                    synced: n.data.clone(),
                    shared: None,
                });
                o.names.insert(name.clone(), c);
                o.durable.insert(name.clone(), c);
            }
            for d in &m.dirs {
                o.dirs.insert(d.clone());
            }
        }
        out
    }

    pub fn mutate(&self, name: &Path, f: impl FnOnce(&mut MemNode)) {
        let mut m = self.lock();
        if let Some(&idx) = m.names.get(&key(name)) {
            f(&mut m.nodes[idx]);
        }
    }

    pub fn list(&self, prefix: &Path) -> Vec<String> {
        let m = self.lock();
        let prefix = key(prefix);
        let mut out: Vec<String> = m
            .names
            .keys()
            .filter(|name| name.starts_with(&prefix))
            .cloned()
            .collect();
        out.sort();
        out
    }
}

impl Fs for MemFs {
    fn open_file(
        &self,
        path: &Path,
        flags: OpenFlags,
        _mode: u32,
    ) -> Result<Arc<dyn File>, FsError> {
        let name = key(path);
        let mut m = self.lock();
        let existing = m.names.get(&name).copied();
        if flags.contains(OpenFlags::CREATE) || flags.contains(OpenFlags::TRUNC) {
            m.step(&self.0.down)?;
        }

        let idx = match existing {
            Some(_) if flags.contains(OpenFlags::CREATE) && flags.contains(OpenFlags::EXCL) => {
                return Err(FsError::bare(io::Error::from(io::ErrorKind::AlreadyExists)));
            }
            None if !flags.contains(OpenFlags::CREATE) => return Err(not_exist("open", &name)),
            None => {
                let idx = m.add(MemNode::default());
                m.names.insert(name.clone(), idx);
                idx
            }
            Some(idx) => idx,
        };

        if flags.contains(OpenFlags::TRUNC) {
            let len = m.nodes[idx].data.len() as i64;
            m.used -= len;
            m.nodes[idx].data = Vec::new();
            m.nodes[idx].shared = None;
        }

        Ok(Arc::new(MemFile {
            fs: self.clone(),
            name,
            node: idx,
        }))
    }

    fn remove(&self, path: &Path) -> Result<(), FsError> {
        let name = key(path);
        let mut m = self.lock();
        m.step(&self.0.down)?;

        let Some(idx) = m.names.get(&name).copied() else {
            return Err(not_exist("remove", &name));
        };

        let len = m.nodes[idx].data.len() as i64;
        m.used -= len;
        m.names.remove(&name);

        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        let (from, to) = (key(from), key(to));
        let mut m = self.lock();
        m.step(&self.0.down)?;

        let Some(idx) = m.names.get(&from).copied() else {
            return Err(not_exist("rename", &from));
        };

        if let Some(old) = m.names.get(&to).copied() {
            let len = m.nodes[old].data.len() as i64;
            m.used -= len;
        }

        m.names.insert(to, idx);
        m.names.remove(&from);

        Ok(())
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FsError> {
        let name = key(path);
        let m = self.lock();

        if !m.dirs.contains(&name) {
            return Err(not_exist("readdir", &name));
        }

        let mut out = Vec::new();
        for (p, &idx) in &m.names {
            if dir_of(p) == name {
                out.push(DirEntry::new(
                    base_of(p),
                    false,
                    Some(m.nodes[idx].data.len() as u64),
                ));
            }
        }
        for d in &m.dirs {
            if dir_of(d) == name && *d != name {
                out.push(DirEntry::new(base_of(d), true, Some(0)));
            }
        }
        out.sort_by(|a, b| a.name().cmp(b.name()));

        Ok(out)
    }

    fn mkdir_all(&self, path: &Path, _mode: u32) -> Result<(), FsError> {
        let mut m = self.lock();

        if m.crashed {
            return Err(err_crashed());
        }

        let mut d = key(path);
        while d != "/" && d != "." && !m.dirs.contains(&d) {
            m.dirs.insert(d.clone());
            d = dir_of(&d);
        }

        Ok(())
    }

    fn sync_dir(&self, path: &Path) -> Result<(), FsError> {
        let name = key(path);
        let mut m = self.lock();
        m.step(&self.0.down)?;

        let gone: Vec<String> = m
            .durable
            .keys()
            .filter(|p| dir_of(p) == name && !m.names.contains_key(*p))
            .cloned()
            .collect();
        for p in gone {
            m.durable.remove(&p);
        }

        let present: Vec<(String, usize)> = m
            .names
            .iter()
            .filter(|(p, _)| dir_of(p) == name)
            .map(|(p, &idx)| (p.clone(), idx))
            .collect();
        for (p, idx) in present {
            m.durable.insert(p, idx);
        }

        Ok(())
    }
}

pub struct MemFile {
    fs: MemFs,
    name: String,
    node: usize,
}

impl File for MemFile {
    fn read_at(&self, buf: &mut [u8], off: u64) -> (usize, Option<FsError>) {
        let m = self.fs.lock();

        if m.crashed {
            return (0, Some(err_crashed()));
        }

        let data = &m.nodes[self.node].data;
        if off >= data.len() as u64 {
            return (0, Some(FsError::eof()));
        }

        let src = &data[off as usize..];
        let n = src.len().min(buf.len());
        buf[..n].copy_from_slice(&src[..n]);
        if n < buf.len() {
            return (n, Some(FsError::eof()));
        }

        (n, None)
    }

    fn write_at(&self, buf: &[u8], off: u64) -> (usize, Option<FsError>) {
        let mut m = self.fs.lock();

        if let Err(e) = m.step(&self.fs.0.down) {
            return (0, Some(e));
        }

        let end = off as i64 + buf.len() as i64;
        let grow = (end - m.nodes[self.node].data.len() as i64).max(0);
        if m.quota >= 0 && m.used + grow > m.quota {
            return (0, Some(FsError::errno(libc::ENOSPC)));
        }

        let node = &mut m.nodes[self.node];
        if grow > 0 {
            let len = node.data.len();
            if node.shared.is_some_and(|cap| len + grow as usize > cap) {
                node.shared = None;
            }
            node.data.resize(len + grow as usize, 0);
        }
        let off = off as usize;
        node.data[off..off + buf.len()].copy_from_slice(buf);
        if node.shared.is_some() && off < node.synced.len() {
            let end = (off + buf.len()).min(node.synced.len());
            node.synced[off..end].copy_from_slice(&buf[..end - off]);
        }
        m.used += grow;

        (buf.len(), None)
    }

    fn sync(&self) -> Result<(), FsError> {
        let mut m = self.fs.lock();
        m.step(&self.fs.0.down)?;

        let node = &mut m.nodes[self.node];
        if node.shared.is_some_and(|cap| node.data.len() > cap) {
            node.shared = None;
        }
        node.synced = node.data.clone();

        Ok(())
    }

    fn truncate(&self, size: u64) -> Result<(), FsError> {
        let mut m = self.fs.lock();
        m.step(&self.fs.0.down)?;

        let len = m.nodes[self.node].data.len() as u64;
        if size < len {
            m.used -= (len - size) as i64;
            m.nodes[self.node].data.truncate(size as usize);
        }

        Ok(())
    }

    fn close(&self) -> Result<(), FsError> {
        Ok(())
    }

    fn size(&self) -> Result<u64, FsError> {
        let m = self.fs.lock();
        Ok(m.nodes[self.node].data.len() as u64)
    }
}

impl MemFile {
    pub fn name(&self) -> &str {
        &self.name
    }
}
