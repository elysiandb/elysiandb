use std::io;
use std::os::fd::RawFd;
use std::ptr;
use std::time::Duration;

pub(crate) const TCP_KEEPIDLE_OPTION: libc::c_int = libc::TCP_KEEPALIVE;

pub(crate) const MAX_WORKERS: usize = 4096;

pub(crate) fn somaxconn() -> libc::c_int {
    let mut value: libc::c_int = 0;
    let mut len: libc::size_t = std::mem::size_of::<libc::c_int>();
    let name = c"kern.ipc.somaxconn";
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut value).cast::<libc::c_void>(),
            &raw mut len,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || value <= 0 {
        return libc::SOMAXCONN;
    }
    value.min(65535)
}

pub(crate) struct Selector {
    kq: RawFd,
}

impl Selector {
    pub(crate) fn new() -> io::Result<Selector> {
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe {
            libc::fcntl(kq, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        Ok(Selector { kq })
    }

    fn change(&self, fd: RawFd, flags: u16, token: u64) -> io::Result<()> {
        let ev = libc::kevent {
            ident: fd as libc::uintptr_t,
            filter: libc::EVFILT_READ,
            flags,
            fflags: 0,
            data: 0,
            udata: token as usize as *mut libc::c_void,
        };
        let rc = unsafe { libc::kevent(self.kq, &ev, 1, ptr::null_mut(), 0, ptr::null()) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(crate) fn add(&self, fd: RawFd, token: u64, oneshot: bool) -> io::Result<()> {
        let mut flags = libc::EV_ADD;
        if oneshot {
            flags |= libc::EV_ONESHOT;
        }
        self.change(fd, flags, token)
    }

    pub(crate) fn delete(&self, fd: RawFd) -> io::Result<()> {
        self.change(fd, libc::EV_DELETE, 0)
    }

    pub(crate) fn wait(&self, out: &mut Vec<u64>, timeout: Option<Duration>) -> io::Result<()> {
        out.clear();
        let mut events: Vec<libc::kevent> = Vec::with_capacity(256);
        let ts;
        let ts_ptr = match timeout {
            None => ptr::null(),
            Some(d) => {
                ts = libc::timespec {
                    tv_sec: libc::time_t::try_from(d.as_secs()).unwrap_or(libc::time_t::MAX),
                    tv_nsec: libc::c_long::from(d.subsec_nanos() as i32),
                };
                &raw const ts
            }
        };
        let n = unsafe { libc::kevent(self.kq, ptr::null(), 0, events.as_mut_ptr(), 256, ts_ptr) };
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
            let udata = ev.udata;
            out.push(udata as usize as u64);
        }
        Ok(())
    }
}

impl Drop for Selector {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.kq);
        }
    }
}
