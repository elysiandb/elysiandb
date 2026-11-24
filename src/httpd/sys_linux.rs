use std::io;
use std::os::fd::RawFd;
use std::ptr;
use std::time::Duration;

pub(crate) const TCP_KEEPIDLE_OPTION: libc::c_int = libc::TCP_KEEPIDLE;

pub(crate) const MAX_WORKERS: usize = 16384;

pub(crate) fn somaxconn() -> libc::c_int {
    let Ok(text) = std::fs::read_to_string("/proc/sys/net/core/somaxconn") else {
        return libc::SOMAXCONN;
    };
    let field = text.split_whitespace().next().unwrap_or("");
    match field.parse::<i64>() {
        Ok(n) if n > 0 => libc::c_int::try_from(n).unwrap_or(libc::c_int::MAX),
        _ => libc::SOMAXCONN,
    }
}

pub(crate) struct Selector {
    epfd: RawFd,
}

impl Selector {
    pub(crate) fn new() -> io::Result<Selector> {
        let epfd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epfd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Selector { epfd })
    }

    pub(crate) fn add(&self, fd: RawFd, token: u64, oneshot: bool) -> io::Result<()> {
        let mut flags = libc::EPOLLIN | libc::EPOLLRDHUP;
        if oneshot {
            flags |= libc::EPOLLONESHOT;
        }
        let mut ev = libc::epoll_event {
            events: flags as u32,
            u64: token,
        };
        let rc = unsafe { libc::epoll_ctl(self.epfd, libc::EPOLL_CTL_ADD, fd, &raw mut ev) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(crate) fn delete(&self, fd: RawFd) -> io::Result<()> {
        let rc = unsafe { libc::epoll_ctl(self.epfd, libc::EPOLL_CTL_DEL, fd, ptr::null_mut()) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(crate) fn wait(&self, out: &mut Vec<u64>, timeout: Option<Duration>) -> io::Result<()> {
        out.clear();
        let timeout_ms: libc::c_int = match timeout {
            None => -1,
            Some(d) => {
                let ms = d.as_nanos().div_ceil(1_000_000);
                libc::c_int::try_from(ms).unwrap_or(libc::c_int::MAX)
            }
        };
        let mut events: Vec<libc::epoll_event> = Vec::with_capacity(256);
        let n = unsafe { libc::epoll_wait(self.epfd, events.as_mut_ptr(), 256, timeout_ms) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                return Ok(());
            }
            return Err(e);
        }
        unsafe {
            events.set_len(n as usize);
        }
        for ev in &events {
            let token: u64 = ev.u64;
            out.push(token);
        }
        Ok(())
    }
}

impl Drop for Selector {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.epfd);
        }
    }
}
