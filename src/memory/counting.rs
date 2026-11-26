use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicI64, Ordering};

use mimalloc::MiMalloc;

static ALLOCATED: AtomicI64 = AtomicI64::new(0);

pub struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { MiMalloc.alloc(layout) };
        if !p.is_null() {
            ALLOCATED.fetch_add(layout.size() as i64, Ordering::Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { MiMalloc.alloc_zeroed(layout) };
        if !p.is_null() {
            ALLOCATED.fetch_add(layout.size() as i64, Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { MiMalloc.dealloc(ptr, layout) };
        ALLOCATED.fetch_sub(layout.size() as i64, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { MiMalloc.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            ALLOCATED.fetch_add(new_size as i64 - layout.size() as i64, Ordering::Relaxed);
        }
        p
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

pub fn allocated() -> i64 {
    ALLOCATED.load(Ordering::Relaxed)
}

#[cfg(target_os = "linux")]
pub fn resident() -> i64 {
    let Ok(statm) = std::fs::read("/proc/self/statm") else {
        return 0;
    };
    let Some(field) = statm.split(|&b| b == b' ').nth(1) else {
        return 0;
    };
    let mut pages: i64 = 0;
    for &b in field {
        if !b.is_ascii_digit() {
            break;
        }
        pages = pages.wrapping_mul(10).wrapping_add(i64::from(b - b'0'));
    }
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size <= 0 {
        return 0;
    }

    pages.wrapping_mul(size as i64)
}

#[cfg(target_os = "macos")]
pub fn resident() -> i64 {
    let mut info = std::mem::MaybeUninit::<libc::mach_task_basic_info>::zeroed();
    let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
    #[allow(deprecated)]
    let task = unsafe { libc::mach_task_self() };
    let kr = unsafe {
        libc::task_info(
            task,
            libc::MACH_TASK_BASIC_INFO,
            info.as_mut_ptr().cast::<libc::integer_t>(),
            &mut count,
        )
    };
    if kr != libc::KERN_SUCCESS {
        return 0;
    }
    let info = unsafe { info.assume_init() };

    info.resident_size as i64
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn resident() -> i64 {
    0
}

unsafe extern "C" {
    fn mi_collect(force: bool);
}

pub fn free_os_memory() {
    unsafe { mi_collect(true) }
}
