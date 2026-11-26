const IOPRIO_WHO_PROCESS: libc::c_long = 1;
const IOPRIO_CLASS_BE: libc::c_long = 2;
const IOPRIO_CLASS_SHIFT: libc::c_long = 13;
const IOPRIO_LOWEST: libc::c_long = 7;
const GENTLE_NICE: libc::c_int = 10;

pub fn run_gently<F: FnOnce() + Send>(f: F) {
    std::thread::scope(|s| {
        let handle = std::thread::Builder::new()
            .name("gentle".to_string())
            .spawn_scoped(s, move || {
                unsafe {
                    libc::syscall(
                        libc::SYS_ioprio_set,
                        IOPRIO_WHO_PROCESS,
                        0 as libc::c_long,
                        (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | IOPRIO_LOWEST,
                    );
                    libc::setpriority(libc::PRIO_PROCESS, 0, GENTLE_NICE);
                }
                crate::sync::run_fatal(f);
            })
            .unwrap_or_else(|e| panic!("cannot start thread gentle: {e}"));
        if let Err(payload) = handle.join() {
            std::panic::resume_unwind(payload);
        }
    });
}
