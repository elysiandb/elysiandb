use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use talos::host::{DocValue, MapRef};

use crate::globals;
use crate::gocompat::json::Map;
use crate::hook::{self, Request};
use crate::sync::Mutex;

const HOOK_HELPER_STACK: usize = 8 << 20;

pub struct HookPool {
    cap: usize,
    used: AtomicUsize,
}

impl HookPool {
    pub fn cap(&self) -> usize {
        self.cap
    }

    fn try_acquire(&self) -> bool {
        let mut cur = self.used.load(Ordering::SeqCst);
        loop {
            if cur >= self.cap {
                return false;
            }
            match self
                .used
                .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(v) => cur = v,
            }
        }
    }

    fn release(&self) {
        self.used.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn hook_helpers() -> &'static HookPool {
    static POOL: OnceLock<HookPool> = OnceLock::new();
    POOL.get_or_init(|| HookPool {
        cap: globals::procs().max(1),
        used: AtomicUsize::new(0),
    })
}

struct Token<'a>(&'a HookPool);

impl Drop for Token<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub fn apply_post_read_hooks<T, F>(
    hooks: &Request,
    entity: &str,
    data: &[Map],
    finish: F,
) -> Vec<Option<T>>
where
    T: Send,
    F: Fn(&MapRef) -> T + Sync,
{
    let slots: Vec<Mutex<Option<T>>> = (0..data.len()).map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);

    let run = || {
        loop {
            let i = next.fetch_add(1, Ordering::SeqCst);
            if i >= data.len() || hooks.stopped() {
                return;
            }
            let doc = hook::document_from_json(&data[i]);
            let doc = hooks.post_read(entity, &doc);
            let out = finish(&doc);
            DocValue::Map(doc).release_graph();
            *slots[i].lock() = Some(out);
        }
    };

    if data.len() <= 1 {
        run();
    } else {
        let pool = hook_helpers();
        let want = data.len().min(pool.cap()) - 1;
        std::thread::scope(|s| {
            let mut helpers = Vec::new();
            for _ in 0..want {
                if !pool.try_acquire() {
                    break;
                }
                let token = Token(pool);
                let run = &run;
                let h = std::thread::Builder::new()
                    .name("hook-helper".to_string())
                    .stack_size(HOOK_HELPER_STACK)
                    .spawn_scoped(s, move || {
                        let _token = token;
                        run();
                    });
                match h {
                    Ok(h) => helpers.push(h),
                    Err(e) => panic!("cannot start thread hook-helper: {e}"),
                }
            }

            run();

            for h in helpers {
                if let Err(p) = h.join() {
                    std::panic::resume_unwind(p);
                }
            }
        });
    }

    slots.into_iter().map(|m| m.into_inner()).collect()
}
