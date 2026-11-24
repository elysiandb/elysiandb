use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, ToSocketAddrs};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::time::Duration;

use super::sys::{TCP_KEEPIDLE_OPTION, somaxconn};

#[derive(Debug)]
pub struct ListenError {
    addr: String,
    err: io::Error,
}

impl ListenError {
    pub fn io_error(&self) -> &io::Error {
        &self.err
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }
}

impl fmt::Display for ListenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.err.kind() == io::ErrorKind::AddrInUse {
            return write!(f, "cannot listen on {}: address already in use", self.addr);
        }
        write!(f, "cannot listen on {}: {}", self.addr, self.err)
    }
}

impl std::error::Error for ListenError {}

fn resolve_tcp4(addr: &str) -> io::Result<SocketAddrV4> {
    let Some(colon) = addr.rfind(':') else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing port in address",
        ));
    };
    let (host, port) = (&addr[..colon], &addr[colon + 1..]);
    let port: u16 = if port.is_empty() {
        0
    } else {
        port.parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid port"))?
    };
    if host.is_empty() {
        return Ok(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port));
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(SocketAddrV4::new(ip, port));
    }
    for a in (host, port).to_socket_addrs()? {
        if let SocketAddr::V4(v4) = a {
            return Ok(v4);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no IPv4 address found for host",
    ))
}

fn setsockopt_int(
    fd: RawFd,
    level: libc::c_int,
    name: libc::c_int,
    value: libc::c_int,
) -> io::Result<()> {
    let rc = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            (&raw const value).cast::<libc::c_void>(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn bind_tcp4(addr: &str) -> Result<TcpListener, ListenError> {
    let wrap = |err: io::Error| ListenError {
        addr: addr.to_string(),
        err,
    };
    let sa = resolve_tcp4(addr).map_err(wrap)?;
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(wrap(io::Error::last_os_error()));
    }
    let listener = unsafe { TcpListener::from_raw_fd(fd) };
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    setsockopt_int(fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, 1).map_err(wrap)?;

    let mut sin: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    sin.sin_family = libc::AF_INET as libc::sa_family_t;
    sin.sin_port = sa.port().to_be();
    sin.sin_addr = libc::in_addr {
        s_addr: u32::from_ne_bytes(sa.ip().octets()),
    };
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    {
        sin.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
    }
    let rc = unsafe {
        libc::bind(
            fd,
            (&raw const sin).cast::<libc::sockaddr>(),
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        return Err(wrap(io::Error::last_os_error()));
    }
    let rc = unsafe { libc::listen(fd, somaxconn()) };
    if rc < 0 {
        return Err(wrap(io::Error::last_os_error()));
    }
    Ok(listener)
}

pub(crate) fn configure_accepted(stream: &TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_nodelay(true);
    let fd = stream.as_raw_fd();
    let _ = setsockopt_int(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1);
    let _ = setsockopt_int(fd, libc::IPPROTO_TCP, TCP_KEEPIDLE_OPTION, 15);
    let _ = setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 15);
    let _ = setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 9);
}

pub(crate) struct WakePipe {
    read: RawFd,
    write: RawFd,
}

impl WakePipe {
    pub(crate) fn new() -> io::Result<WakePipe> {
        let mut fds = [0 as libc::c_int; 2];
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        for fd in fds {
            unsafe {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        Ok(WakePipe {
            read: fds[0],
            write: fds[1],
        })
    }

    pub(crate) fn read_fd(&self) -> RawFd {
        self.read
    }

    pub(crate) fn wake(&self) {
        let b = [1u8];
        unsafe {
            libc::write(self.write, b.as_ptr().cast::<libc::c_void>(), 1);
        }
    }

    pub(crate) fn drain(&self) {
        let mut buf = [0u8; 64];
        loop {
            let n = unsafe {
                libc::read(
                    self.read,
                    buf.as_mut_ptr().cast::<libc::c_void>(),
                    buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
        }
    }
}

impl Drop for WakePipe {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read);
            libc::close(self.write);
        }
    }
}

pub(crate) enum Wait {
    Ready,
    Woken,
}

pub(crate) fn wait_readable(
    fd: RawFd,
    wake: &WakePipe,
    timeout: Option<Duration>,
) -> io::Result<Wait> {
    let mut fds = [
        libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake.read_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let timeout_ms: libc::c_int = match timeout {
        None => -1,
        Some(d) => libc::c_int::try_from(d.as_millis()).unwrap_or(libc::c_int::MAX),
    };
    loop {
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };
        if rc < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if fds[1].revents != 0 {
            return Ok(Wait::Woken);
        }
        return Ok(Wait::Ready);
    }
}
