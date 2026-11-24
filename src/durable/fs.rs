use std::ffi::{CString, OsStr, OsString};
use std::fmt;
use std::fs as stdfs;
use std::io;
use std::ops::BitOr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::disk::{IO_READ_BYTES, IO_WRITE_BYTES, note_sync};
use crate::sync::RwLock;

#[derive(Clone, Debug)]
pub struct FsError {
    op: Option<&'static str>,
    path: Option<PathBuf>,
    to: Option<PathBuf>,
    err: Arc<io::Error>,
}

impl FsError {
    pub fn new(op: &'static str, path: impl Into<PathBuf>, err: io::Error) -> FsError {
        FsError {
            op: Some(op),
            path: Some(path.into()),
            to: None,
            err: Arc::new(err),
        }
    }

    pub fn link(
        op: &'static str,
        from: impl Into<PathBuf>,
        to: impl Into<PathBuf>,
        err: io::Error,
    ) -> FsError {
        FsError {
            op: Some(op),
            path: Some(from.into()),
            to: Some(to.into()),
            err: Arc::new(err),
        }
    }

    pub fn bare(err: io::Error) -> FsError {
        FsError {
            op: None,
            path: None,
            to: None,
            err: Arc::new(err),
        }
    }

    pub fn eof() -> FsError {
        FsError::bare(io::Error::from(io::ErrorKind::UnexpectedEof))
    }

    pub fn errno(errno: i32) -> FsError {
        FsError::bare(io::Error::from_raw_os_error(errno))
    }

    pub fn other(msg: impl Into<String>) -> FsError {
        FsError::bare(io::Error::other(msg.into()))
    }

    pub fn op(&self) -> Option<&'static str> {
        self.op
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn to(&self) -> Option<&Path> {
        self.to.as_deref()
    }

    pub fn io_error(&self) -> &io::Error {
        &self.err
    }

    pub fn kind(&self) -> io::ErrorKind {
        self.err.kind()
    }

    pub fn raw_os_error(&self) -> Option<i32> {
        self.err.raw_os_error()
    }

    pub fn is_not_exist(&self) -> bool {
        self.err.kind() == io::ErrorKind::NotFound
    }

    pub fn is_exist(&self) -> bool {
        self.err.kind() == io::ErrorKind::AlreadyExists
    }

    pub fn is_eof(&self) -> bool {
        self.op.is_none() && self.err.kind() == io::ErrorKind::UnexpectedEof
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.op, &self.path, &self.to) {
            (Some(op), Some(path), Some(to)) => {
                write!(f, "{op} {} {}: {}", path.display(), to.display(), self.err)
            }
            (Some(op), Some(path), None) => write!(f, "{op} {}: {}", path.display(), self.err),
            _ => write!(f, "{}", self.err),
        }
    }
}

impl std::error::Error for FsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.err)
    }
}

impl From<io::Error> for FsError {
    fn from(err: io::Error) -> FsError {
        FsError::bare(err)
    }
}

pub fn find_errno(err: &(dyn std::error::Error + 'static), errno: i32) -> bool {
    crate::gocompat::errors::any(err, &mut |e| {
        if let Some(fe) = e.downcast_ref::<FsError>() {
            return fe.raw_os_error() == Some(errno);
        }
        if let Some(ie) = e.downcast_ref::<io::Error>() {
            return ie.raw_os_error() == Some(errno);
        }
        false
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenFlags(u32);

impl OpenFlags {
    pub const RDONLY: OpenFlags = OpenFlags(0);
    pub const WRONLY: OpenFlags = OpenFlags(1);
    pub const RDWR: OpenFlags = OpenFlags(2);
    pub const CREATE: OpenFlags = OpenFlags(0x40);
    pub const EXCL: OpenFlags = OpenFlags(0x80);
    pub const TRUNC: OpenFlags = OpenFlags(0x200);

    const ACCESS: u32 = 3;

    pub fn contains(self, other: OpenFlags) -> bool {
        if other.0 == 0 {
            return self.0 & OpenFlags::ACCESS == 0;
        }
        self.0 & other.0 == other.0
    }

    pub fn access(self) -> OpenFlags {
        OpenFlags(self.0 & OpenFlags::ACCESS)
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    fn to_libc(self) -> libc::c_int {
        let mut flags = match self.0 & OpenFlags::ACCESS {
            1 => libc::O_WRONLY,
            2 => libc::O_RDWR,
            _ => libc::O_RDONLY,
        };
        if self.contains(OpenFlags::CREATE) {
            flags |= libc::O_CREAT;
        }
        if self.contains(OpenFlags::EXCL) {
            flags |= libc::O_EXCL;
        }
        if self.contains(OpenFlags::TRUNC) {
            flags |= libc::O_TRUNC;
        }
        flags | libc::O_CLOEXEC
    }
}

impl BitOr for OpenFlags {
    type Output = OpenFlags;

    fn bitor(self, rhs: OpenFlags) -> OpenFlags {
        OpenFlags(self.0 | rhs.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    name: OsString,
    is_dir: bool,
    size: Option<u64>,
}

impl DirEntry {
    pub fn new(name: impl Into<OsString>, is_dir: bool, size: Option<u64>) -> DirEntry {
        DirEntry {
            name: name.into(),
            is_dir,
            size,
        }
    }

    pub fn name(&self) -> &OsStr {
        &self.name
    }

    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    pub fn size(&self) -> Option<u64> {
        self.size
    }
}

pub trait File: Send + Sync {
    fn read_at(&self, buf: &mut [u8], off: u64) -> (usize, Option<FsError>);
    fn write_at(&self, buf: &[u8], off: u64) -> (usize, Option<FsError>);
    fn sync(&self) -> Result<(), FsError>;
    fn truncate(&self, size: u64) -> Result<(), FsError>;
    fn close(&self) -> Result<(), FsError>;
    fn size(&self) -> Result<u64, FsError>;

    fn os_file(&self) -> Option<&OsFile> {
        None
    }
}

pub trait Fs: Send + Sync {
    fn open_file(&self, path: &Path, flags: OpenFlags, mode: u32)
    -> Result<Arc<dyn File>, FsError>;
    fn remove(&self, path: &Path) -> Result<(), FsError>;
    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError>;
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FsError>;
    fn mkdir_all(&self, path: &Path, mode: u32) -> Result<(), FsError>;
    fn sync_dir(&self, path: &Path) -> Result<(), FsError>;
}

fn cpath(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))
}

fn retry<F: FnMut() -> libc::c_int>(mut f: F) -> io::Result<libc::c_int> {
    loop {
        let r = f();
        if r >= 0 {
            return Ok(r);
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EINTR) {
            return Err(err);
        }
    }
}

#[cfg(target_vendor = "apple")]
pub(crate) fn fsync_fd(fd: RawFd) -> io::Result<()> {
    retry(|| {
        let r = unsafe { libc::fcntl(fd, libc::F_FULLFSYNC, 0) };
        if r < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ENOTSUP) {
            return unsafe { libc::fsync(fd) };
        }
        r
    })
    .map(|_| ())
}

#[cfg(not(target_vendor = "apple"))]
pub(crate) fn fsync_fd(fd: RawFd) -> io::Result<()> {
    retry(|| unsafe { libc::fsync(fd) }).map(|_| ())
}

pub struct OsFile {
    name: PathBuf,
    file: RwLock<Option<stdfs::File>>,
}

impl OsFile {
    pub fn name(&self) -> &Path {
        &self.name
    }

    fn closed(&self, op: &'static str) -> FsError {
        FsError::new(op, &self.name, io::Error::from_raw_os_error(libc::EBADF))
    }

    pub fn with_fd<R>(&self, f: impl FnOnce(RawFd) -> R) -> Option<R> {
        let g = self.file.read();
        g.as_ref().map(|file| f(file.as_raw_fd()))
    }
}

impl File for OsFile {
    fn read_at(&self, buf: &mut [u8], off: u64) -> (usize, Option<FsError>) {
        let g = self.file.read();
        let Some(file) = g.as_ref() else {
            return (0, Some(self.closed("read")));
        };
        let mut n = 0;
        let mut err = None;
        while n < buf.len() {
            match file.read_at(&mut buf[n..], off + n as u64) {
                Ok(0) => {
                    err = Some(FsError::eof());
                    break;
                }
                Ok(m) => n += m,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    err = Some(FsError::new("read", &self.name, e));
                    break;
                }
            }
        }
        IO_READ_BYTES.fetch_add(n as i64, Ordering::Relaxed);
        (n, err)
    }

    fn write_at(&self, buf: &[u8], off: u64) -> (usize, Option<FsError>) {
        let g = self.file.read();
        let Some(file) = g.as_ref() else {
            return (0, Some(self.closed("write")));
        };
        let mut n = 0;
        let mut err = None;
        while n < buf.len() {
            match file.write_at(&buf[n..], off + n as u64) {
                Ok(0) => {
                    err = Some(FsError::new(
                        "write",
                        &self.name,
                        io::Error::from(io::ErrorKind::WriteZero),
                    ));
                    break;
                }
                Ok(m) => n += m,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    err = Some(FsError::new("write", &self.name, e));
                    break;
                }
            }
        }
        IO_WRITE_BYTES.fetch_add(n as i64, Ordering::Relaxed);
        (n, err)
    }

    fn sync(&self) -> Result<(), FsError> {
        let start = Instant::now();
        let res = {
            let g = self.file.read();
            match g.as_ref() {
                Some(file) => {
                    fsync_fd(file.as_raw_fd()).map_err(|e| FsError::new("sync", &self.name, e))
                }
                None => Err(self.closed("sync")),
            }
        };
        note_sync(start);
        res
    }

    fn truncate(&self, size: u64) -> Result<(), FsError> {
        let g = self.file.read();
        let Some(file) = g.as_ref() else {
            return Err(self.closed("truncate"));
        };
        let Ok(len) = libc::off_t::try_from(size) else {
            return Err(FsError::new(
                "truncate",
                &self.name,
                io::Error::from_raw_os_error(libc::EINVAL),
            ));
        };
        let fd = file.as_raw_fd();
        retry(|| unsafe { libc::ftruncate(fd, len) })
            .map(|_| ())
            .map_err(|e| FsError::new("truncate", &self.name, e))
    }

    fn close(&self) -> Result<(), FsError> {
        let taken = self.file.write().take();
        let Some(file) = taken else {
            return Err(self.closed("close"));
        };
        let fd = file.into_raw_fd();
        if unsafe { libc::close(fd) } != 0 {
            return Err(FsError::new(
                "close",
                &self.name,
                io::Error::last_os_error(),
            ));
        }
        Ok(())
    }

    fn size(&self) -> Result<u64, FsError> {
        let g = self.file.read();
        let Some(file) = g.as_ref() else {
            return Err(self.closed("stat"));
        };
        file.metadata()
            .map(|m| m.len())
            .map_err(|e| FsError::new("stat", &self.name, e))
    }

    fn os_file(&self) -> Option<&OsFile> {
        Some(self)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OsFs;

pub static OS: OsFs = OsFs;

pub fn or(fs: Option<Arc<dyn Fs>>) -> Arc<dyn Fs> {
    fs.unwrap_or_else(|| Arc::new(OsFs))
}

fn mkdir_all_bytes(path: &[u8], mode: u32) -> Result<(), FsError> {
    let p = Path::new(OsStr::from_bytes(path));
    if let Ok(m) = stdfs::metadata(p) {
        if m.is_dir() {
            return Ok(());
        }
        return Err(FsError::new(
            "mkdir",
            p,
            io::Error::from_raw_os_error(libc::ENOTDIR),
        ));
    }

    let mut i = path.len() as isize - 1;
    while i >= 0 && path[i as usize] == b'/' {
        i -= 1;
    }
    while i >= 0 && path[i as usize] != b'/' {
        i -= 1;
    }
    if i < 0 {
        i = 0;
    }
    let parent = &path[..i as usize];
    if !parent.is_empty() {
        mkdir_all_bytes(parent, mode)?;
    }

    let res = cpath(p).and_then(|c| {
        retry(|| unsafe { libc::mkdir(c.as_ptr(), mode as libc::mode_t) }).map(|_| ())
    });
    match res {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Ok(m) = stdfs::symlink_metadata(p)
                && m.is_dir()
            {
                return Ok(());
            }
            Err(FsError::new("mkdir", p, e))
        }
    }
}

impl Fs for OsFs {
    fn open_file(
        &self,
        path: &Path,
        flags: OpenFlags,
        mode: u32,
    ) -> Result<Arc<dyn File>, FsError> {
        let c = cpath(path).map_err(|e| FsError::new("open", path, e))?;
        let oflags = flags.to_libc();
        let fd = retry(|| unsafe { libc::open(c.as_ptr(), oflags, mode as libc::c_uint) })
            .map_err(|e| FsError::new("open", path, e))?;
        let file = unsafe { stdfs::File::from_raw_fd(fd) };
        Ok(Arc::new(OsFile {
            name: path.to_path_buf(),
            file: RwLock::new(Some(file)),
        }))
    }

    fn remove(&self, path: &Path) -> Result<(), FsError> {
        let c = cpath(path).map_err(|e| FsError::new("remove", path, e))?;
        let e = match retry(|| unsafe { libc::unlink(c.as_ptr()) }) {
            Ok(_) => return Ok(()),
            Err(e) => e,
        };
        let e1 = match retry(|| unsafe { libc::rmdir(c.as_ptr()) }) {
            Ok(_) => return Ok(()),
            Err(e1) => e1,
        };
        let chosen = if e1.raw_os_error() != Some(libc::ENOTDIR) {
            e1
        } else {
            e
        };
        Err(FsError::new("remove", path, chosen))
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        if let Ok(fi) = stdfs::symlink_metadata(to)
            && fi.is_dir()
        {
            match stdfs::symlink_metadata(from) {
                Err(e) => return Err(FsError::link("rename", from, to, e)),
                Ok(ofi) => {
                    if to.as_os_str() == from.as_os_str()
                        || fi.dev() != ofi.dev()
                        || fi.ino() != ofi.ino()
                    {
                        return Err(FsError::link(
                            "rename",
                            from,
                            to,
                            io::Error::from_raw_os_error(libc::EEXIST),
                        ));
                    }
                }
            }
        }
        let cf = cpath(from).map_err(|e| FsError::link("rename", from, to, e))?;
        let ct = cpath(to).map_err(|e| FsError::link("rename", from, to, e))?;
        retry(|| unsafe { libc::rename(cf.as_ptr(), ct.as_ptr()) })
            .map(|_| ())
            .map_err(|e| FsError::link("rename", from, to, e))
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FsError> {
        let rd = stdfs::read_dir(path).map_err(|e| FsError::new("open", path, e))?;
        let mut out = Vec::new();
        for ent in rd {
            let ent = ent.map_err(|e| FsError::new("read", path, e))?;
            let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let size = ent.metadata().ok().map(|m| m.len());
            out.push(DirEntry::new(ent.file_name(), is_dir, size));
        }
        out.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        Ok(out)
    }

    fn mkdir_all(&self, path: &Path, mode: u32) -> Result<(), FsError> {
        mkdir_all_bytes(path.as_os_str().as_bytes(), mode)
    }

    fn sync_dir(&self, path: &Path) -> Result<(), FsError> {
        let d = stdfs::File::open(path).map_err(|e| FsError::new("open", path, e))?;
        let start = Instant::now();
        let _ = fsync_fd(d.as_raw_fd());
        note_sync(start);
        Ok(())
    }
}

pub struct BufferedWriter<'a> {
    file: &'a dyn File,
    off: u64,
    size: usize,
    buf: Vec<u8>,
    err: Option<FsError>,
}

impl<'a> BufferedWriter<'a> {
    pub fn new(file: &'a dyn File, size: usize) -> BufferedWriter<'a> {
        BufferedWriter {
            file,
            off: 0,
            size,
            buf: Vec::new(),
            err: None,
        }
    }

    pub fn available(&self) -> usize {
        self.size - self.buf.len()
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    fn put(&mut self, p: &[u8]) {
        if self.buf.capacity() == 0 {
            self.buf.reserve_exact(self.size);
        }
        self.buf.extend_from_slice(p);
    }

    pub fn write(&mut self, mut p: &[u8]) -> Result<(), FsError> {
        while p.len() > self.available() && self.err.is_none() {
            let n;
            if self.buffered() == 0 {
                let (m, err) = self.file.write_at(p, self.off);
                self.off += m as u64;
                self.err = err;
                n = m;
            } else {
                n = self.available();
                self.put(&p[..n]);
                let _ = self.flush();
            }
            p = &p[n..];
        }
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        self.put(p);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), FsError> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.buf.is_empty() {
            return Ok(());
        }
        let (n, mut err) = self.file.write_at(&self.buf, self.off);
        self.off += n as u64;
        if n < self.buf.len() && err.is_none() {
            err = Some(FsError::bare(io::Error::new(
                io::ErrorKind::WriteZero,
                "short write",
            )));
        }
        if let Some(e) = err {
            self.buf.drain(..n.min(self.buf.len()));
            self.err = Some(e.clone());
            return Err(e);
        }
        self.buf.clear();
        Ok(())
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

pub fn write_file_atomic<F>(fs: &dyn Fs, path: &Path, write: F) -> Result<(), FsError>
where
    F: FnOnce(&mut BufferedWriter<'_>) -> Result<(), FsError>,
{
    let tmp = with_suffix(path, ".tmp");
    let f = fs.open_file(
        &tmp,
        OpenFlags::CREATE | OpenFlags::TRUNC | OpenFlags::WRONLY,
        0o600,
    )?;

    let mut err = {
        let mut bw = BufferedWriter::new(&*f, 1 << 20);
        let mut err = write(&mut bw).err();
        if err.is_none() {
            err = bw.flush().err();
        }
        err
    };
    if err.is_none() {
        err = f.sync().err();
    }
    let close_err = f.close().err();
    if err.is_none() {
        err = close_err;
    }

    if let Some(e) = err {
        let _ = fs.remove(&tmp);
        return Err(e);
    }

    if let Err(e) = fs.rename(&tmp, path) {
        let _ = fs.remove(&tmp);
        return Err(e);
    }

    fs.sync_dir(&filepath::dir(path))
}

pub fn read_file(fs: &dyn Fs, path: &Path) -> Result<Vec<u8>, FsError> {
    let f = fs.open_file(path, OpenFlags::RDONLY, 0)?;
    let res = read_all(&*f);
    let _ = f.close();
    res
}

fn read_all(f: &dyn File) -> Result<Vec<u8>, FsError> {
    let size = f.size()?;
    let mut data = vec![0u8; size as usize];
    let (_, err) = f.read_at(&mut data, 0);
    if let Some(e) = err
        && !e.is_eof()
    {
        return Err(e);
    }
    Ok(data)
}

pub mod filepath {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    fn from_bytes(b: Vec<u8>) -> PathBuf {
        PathBuf::from(OsString::from_vec(b))
    }

    fn clean_bytes(path: &[u8]) -> Vec<u8> {
        if path.is_empty() {
            return b".".to_vec();
        }
        let rooted = path[0] == b'/';
        let n = path.len();
        let mut out: Vec<u8> = Vec::with_capacity(n);
        let (mut r, mut dotdot) = (0, 0);
        if rooted {
            out.push(b'/');
            r = 1;
            dotdot = 1;
        }
        while r < n {
            if path[r] == b'/' || (path[r] == b'.' && (r + 1 == n || path[r + 1] == b'/')) {
                r += 1;
            } else if path[r] == b'.' && path[r + 1] == b'.' && (r + 2 == n || path[r + 2] == b'/')
            {
                r += 2;
                if out.len() > dotdot {
                    let mut w = out.len() - 1;
                    while w > dotdot && out[w] != b'/' {
                        w -= 1;
                    }
                    out.truncate(w);
                } else if !rooted {
                    if !out.is_empty() {
                        out.push(b'/');
                    }
                    out.push(b'.');
                    out.push(b'.');
                    dotdot = out.len();
                }
            } else {
                if (rooted && out.len() != 1) || (!rooted && !out.is_empty()) {
                    out.push(b'/');
                }
                while r < n && path[r] != b'/' {
                    out.push(path[r]);
                    r += 1;
                }
            }
        }
        if out.is_empty() {
            out.push(b'.');
        }
        out
    }

    pub fn clean(path: &Path) -> PathBuf {
        from_bytes(clean_bytes(path.as_os_str().as_bytes()))
    }

    pub fn join(dir: &Path, name: impl AsRef<OsStr>) -> PathBuf {
        let a = dir.as_os_str().as_bytes();
        let b = name.as_ref().as_bytes();
        if a.is_empty() {
            if b.is_empty() {
                return PathBuf::new();
            }
            return from_bytes(clean_bytes(b));
        }
        let mut s = Vec::with_capacity(a.len() + 1 + b.len());
        s.extend_from_slice(a);
        s.push(b'/');
        s.extend_from_slice(b);
        from_bytes(clean_bytes(&s))
    }

    pub fn dir(path: &Path) -> PathBuf {
        let p = path.as_os_str().as_bytes();
        let mut i = p.len();
        while i > 0 && p[i - 1] != b'/' {
            i -= 1;
        }
        from_bytes(clean_bytes(&p[..i]))
    }

    pub fn base(path: &Path) -> OsString {
        let mut p = path.as_os_str().as_bytes();
        if p.is_empty() {
            return OsString::from(".");
        }
        while !p.is_empty() && p[p.len() - 1] == b'/' {
            p = &p[..p.len() - 1];
        }
        let mut i = p.len();
        while i > 0 && p[i - 1] != b'/' {
            i -= 1;
        }
        let p = &p[i..];
        if p.is_empty() {
            return OsString::from("/");
        }
        OsString::from_vec(p.to_vec())
    }
}
