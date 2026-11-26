use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::time::{Duration, Instant};

pub(super) struct Pty {
    pub(super) master: File,
    pub(super) slave: Option<File>,
    pub(super) seen: Vec<u8>,
}

pub(super) fn open() -> Pty {
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    assert!(master >= 0, "posix_openpt");
    assert_eq!(unsafe { libc::grantpt(master) }, 0, "grantpt");
    assert_eq!(unsafe { libc::unlockpt(master) }, 0, "unlockpt");
    let name = unsafe { std::ffi::CStr::from_ptr(libc::ptsname(master)) }
        .to_string_lossy()
        .into_owned();
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&name)
        .expect("terminal");
    let fd = slave.as_raw_fd();
    let mut tio: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut tio) }, 0, "tcgetattr");
    tio.c_oflag &= !libc::OPOST;
    assert_eq!(
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &tio) },
        0,
        "tcsetattr"
    );

    Pty {
        master: unsafe { File::from_raw_fd(master) },
        slave: Some(slave),
        seen: Vec::new(),
    }
}

impl Pty {
    pub(super) fn slave(&mut self) -> File {
        self.slave.take().expect("slave side")
    }

    pub(super) fn slave_copy(&self) -> File {
        self.slave
            .as_ref()
            .expect("slave side")
            .try_clone()
            .expect("slave copy")
    }

    fn readable(&self, wait: Duration) -> bool {
        let mut fds = [libc::pollfd {
            fd: self.master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        let ms = libc::c_int::try_from(wait.as_millis()).unwrap_or(libc::c_int::MAX);
        unsafe { libc::poll(fds.as_mut_ptr(), 1, ms) > 0 }
    }

    pub(super) fn wait_for(&mut self, needle: &[u8]) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut buf = [0u8; 4096];
        while !self.seen.windows(needle.len()).any(|w| w == needle) {
            let now = Instant::now();
            if now >= deadline || !self.readable(deadline - now) {
                return false;
            }
            match self.master.read(&mut buf) {
                Ok(0) | Err(_) => return false,
                Ok(n) => self.seen.extend_from_slice(&buf[..n]),
            }
        }
        true
    }

    pub(super) fn drain(&mut self) {
        let mut buf = [0u8; 4096];
        while self.readable(Duration::from_millis(300)) {
            match self.master.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => self.seen.extend_from_slice(&buf[..n]),
            }
        }
    }

    pub(super) fn type_text(&mut self, text: &[u8]) {
        self.master.write_all(text).expect("type on the terminal");
    }
}
