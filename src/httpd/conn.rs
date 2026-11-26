use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::sync::Mutex;

pub(crate) struct ConnInner {
    pub(crate) stream: TcpStream,
    read_deadline: Mutex<Option<Instant>>,
    write_deadline: Mutex<Option<Instant>>,
    closed: AtomicBool,
    remote: SocketAddr,
    local: SocketAddr,
}

impl ConnInner {
    pub(crate) fn new(stream: TcpStream, remote: SocketAddr, local: SocketAddr) -> ConnInner {
        ConnInner {
            stream,
            read_deadline: Mutex::new(None),
            write_deadline: Mutex::new(None),
            closed: AtomicBool::new(false),
            remote,
            local,
        }
    }

    pub(crate) fn fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    pub(crate) fn set_read_deadline(&self, d: Option<Instant>) {
        *self.read_deadline.lock() = d;
    }

    pub(crate) fn set_write_deadline(&self, d: Option<Instant>) {
        *self.write_deadline.lock() = d;
    }

    pub(crate) fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub(crate) fn remote(&self) -> SocketAddr {
        self.remote
    }

    pub(crate) fn local(&self) -> SocketAddr {
        self.local
    }

    fn remaining(deadline: Option<Instant>) -> Result<Option<Duration>, io::Error> {
        match deadline {
            None => Ok(None),
            Some(d) => {
                let now = Instant::now();
                if d <= now {
                    return Err(timeout_error());
                }
                let left = d - now;
                if left < Duration::from_micros(1) {
                    return Err(timeout_error());
                }
                Ok(Some(left))
            }
        }
    }

    pub(crate) fn read_with_deadline(&self, p: &mut [u8]) -> io::Result<usize> {
        let deadline = *self.read_deadline.lock();
        let left = ConnInner::remaining(deadline)?;
        self.stream.set_read_timeout(left)?;
        loop {
            match (&self.stream).read(p) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(timeout_error());
                }
                r => return r,
            }
        }
    }

    pub(crate) fn write_with_deadline(&self, p: &[u8]) -> io::Result<usize> {
        let deadline = *self.write_deadline.lock();
        let left = ConnInner::remaining(deadline)?;
        self.stream.set_write_timeout(left)?;
        loop {
            match (&self.stream).write(p) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(timeout_error());
                }
                r => return r,
            }
        }
    }

    pub(crate) fn write_all_with_deadline(&self, mut p: &[u8]) -> io::Result<()> {
        while !p.is_empty() {
            match self.write_with_deadline(p) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(n) => p = &p[n..],
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

pub(crate) fn timeout_error() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "i/o timeout")
}

#[derive(Clone)]
pub struct ConnHandle {
    pub(crate) inner: Arc<ConnInner>,
}

impl std::fmt::Debug for ConnHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnHandle")
            .field("remote", &self.inner.remote)
            .field("local", &self.inner.local)
            .finish()
    }
}

impl ConnHandle {
    pub fn set_write_deadline(&self, deadline: Instant) {
        self.inner.set_write_deadline(Some(deadline));
    }

    pub fn set_read_deadline(&self, deadline: Instant) {
        self.inner.set_read_deadline(Some(deadline));
    }

    pub fn clear_deadlines(&self) {
        self.inner.set_read_deadline(None);
        self.inner.set_write_deadline(None);
    }

    pub fn close(&self) {
        self.inner.close();
    }

    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    pub fn raw_fd(&self) -> RawFd {
        self.inner.fd()
    }

    pub fn remote_addr(&self) -> SocketAddr {
        self.inner.remote()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.inner.local()
    }
}

pub(crate) struct SockReader {
    pub(crate) inner: Arc<ConnInner>,
}

impl Read for SockReader {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        self.inner.read_with_deadline(p)
    }
}

pub(crate) struct ConnWriter {
    inner: Arc<ConnInner>,
    buf: Vec<u8>,
    cap: usize,
}

impl ConnWriter {
    pub(crate) fn new(inner: Arc<ConnInner>, cap: usize) -> ConnWriter {
        ConnWriter {
            inner,
            buf: Vec::new(),
            cap: cap.max(512),
        }
    }
}

impl Write for ConnWriter {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        if self.buf.len() + p.len() > self.cap {
            self.flush()?;
        }
        if p.len() >= self.cap {
            self.inner.write_all_with_deadline(p)?;
        } else {
            self.buf.extend_from_slice(p);
        }
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let res = self.inner.write_all_with_deadline(&self.buf);
        self.buf.clear();
        res
    }
}

pub struct HijackConn {
    inner: Arc<ConnInner>,
    buffered: Vec<u8>,
    pos: usize,
}

impl HijackConn {
    pub(crate) fn new(inner: Arc<ConnInner>, buffered: Vec<u8>) -> HijackConn {
        HijackConn {
            inner,
            buffered,
            pos: 0,
        }
    }

    pub fn handle(&self) -> ConnHandle {
        ConnHandle {
            inner: self.inner.clone(),
        }
    }

    pub fn set_read_deadline(&self, deadline: Option<Instant>) {
        self.inner.set_read_deadline(deadline);
    }

    pub fn set_write_deadline(&self, deadline: Option<Instant>) {
        self.inner.set_write_deadline(deadline);
    }

    pub fn set_deadline(&self, deadline: Option<Instant>) {
        self.inner.set_read_deadline(deadline);
        self.inner.set_write_deadline(deadline);
    }

    pub fn remote_addr(&self) -> SocketAddr {
        self.inner.remote()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.inner.local()
    }

    pub fn raw_fd(&self) -> RawFd {
        self.inner.fd()
    }
}

impl Read for HijackConn {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        if self.pos < self.buffered.len() {
            let n = p.len().min(self.buffered.len() - self.pos);
            p[..n].copy_from_slice(&self.buffered[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }
        self.inner.read_with_deadline(p)
    }
}

impl Write for HijackConn {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.inner.write_with_deadline(p)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
