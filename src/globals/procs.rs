use std::os::unix::ffi::OsStrExt;
use std::sync::OnceLock;

pub const PROCS_ENV: &str = "ELYSIANDB_PROCS";

static PROCS: OnceLock<usize> = OnceLock::new();

pub fn procs() -> usize {
    *PROCS.get_or_init(|| {
        std::env::var_os(PROCS_ENV)
            .and_then(|v| parse_procs(v.as_bytes()))
            .unwrap_or_else(cpu_count)
    })
}

pub fn parse_procs(s: &[u8]) -> Option<usize> {
    let n = atoi32(s)?;
    if n > 0 { usize::try_from(n).ok() } else { None }
}

fn atoi32(s: &[u8]) -> Option<i32> {
    if s.is_empty() {
        return None;
    }
    let (neg, digits) = match s.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, s),
    };
    let mut un: u64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        if un > u64::MAX / 10 {
            return None;
        }
        un *= 10;
        let next = un.wrapping_add(u64::from(c - b'0'));
        if next < un {
            return None;
        }
        un = next;
    }
    let max = i64::MAX as u64;
    if (!neg && un > max) || (neg && un > max + 1) {
        return None;
    }
    let n = if neg {
        (un as i64).wrapping_neg()
    } else {
        un as i64
    };
    i32::try_from(n).ok()
}

#[cfg(target_os = "linux")]
fn cpu_count() -> usize {
    unsafe extern "C" {
        fn sched_getaffinity(pid: libc::pid_t, size: libc::size_t, mask: *mut u64) -> libc::c_int;
    }

    let mut mask = [0u64; 1024];
    let r = unsafe { sched_getaffinity(0, std::mem::size_of_val(&mask), mask.as_mut_ptr()) };
    if r != 0 {
        return 1;
    }
    let n: u32 = mask.iter().map(|w| w.count_ones()).sum();
    if n == 0 { 1 } else { n as usize }
}

#[cfg(not(target_os = "linux"))]
fn cpu_count() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}
