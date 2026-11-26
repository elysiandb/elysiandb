fn usage() -> Option<libc::rusage> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }

    Some(unsafe { usage.assume_init() })
}

pub(super) fn process_cpu_seconds() -> f64 {
    let Some(usage) = usage() else {
        return -1.0;
    };

    timeval_seconds(usage.ru_utime) + timeval_seconds(usage.ru_stime)
}

fn timeval_seconds(tv: libc::timeval) -> f64 {
    tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6
}

#[cfg(target_os = "linux")]
const MAXRSS_UNIT: f64 = 1024.0;

#[cfg(not(target_os = "linux"))]
const MAXRSS_UNIT: f64 = 1.0;

pub(super) fn peak_resident_bytes() -> f64 {
    let Some(usage) = usage() else {
        return f64::NAN;
    };

    usage.ru_maxrss as f64 * MAXRSS_UNIT
}
