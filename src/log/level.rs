use super::buffer::encode_time;
use super::log::{CONFIGURED_LEVEL, Level, current_level_value, info, level_name, set_level_value};
use crate::gocompat::json::{Encode, EncodeState, Error as JsonError};
use crate::gocompat::time::{Duration, Time};
use crate::kv;
use crate::sync::{Condvar, Mutex, spawn};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct LevelInfo {
    pub level: &'static str,
    pub configured: &'static str,
    pub until: Option<Time>,
}

impl Encode for LevelInfo {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "level");
        e.write_string(self.level.as_bytes());
        e.write_field_name(&mut first, "configured");
        e.write_string(self.configured.as_bytes());
        e.write_field_name(&mut first, "until");
        match &self.until {
            Some(t) => encode_time(t, e)?,
            None => e.write_null(),
        }
        e.end_struct(first);
        Ok(())
    }
}

struct Override {
    until: Option<Time>,
    timer: Option<Arc<Timer>>,
}

static OVERRIDE: Mutex<Override> = Mutex::new(Override {
    until: None,
    timer: None,
});

#[derive(Clone, Copy, PartialEq, Eq)]
enum TimerState {
    Pending,
    Stopped,
    Fired,
}

struct Timer {
    state: Mutex<TimerState>,
    cv: Condvar,
}

impl Timer {
    fn after_func(d: Duration, f: fn()) -> Arc<Timer> {
        let timer = Arc::new(Timer {
            state: Mutex::new(TimerState::Pending),
            cv: Condvar::new(),
        });
        let wait = std::time::Duration::from_nanos(u64::try_from(d.0).unwrap_or(0));
        let deadline = Instant::now().checked_add(wait);
        let t = Arc::clone(&timer);
        spawn("log-level-timer", move || {
            let mut state = t.state.lock();
            loop {
                if *state == TimerState::Stopped {
                    return;
                }
                match deadline {
                    Some(deadline) => {
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        state = t.cv.wait_timeout(state, deadline - now).0;
                    }
                    None => state = t.cv.wait(state),
                }
            }
            *state = TimerState::Fired;
            drop(state);
            f();
        });
        timer
    }

    fn stop(&self) {
        let mut state = self.state.lock();
        if *state == TimerState::Pending {
            *state = TimerState::Stopped;
            self.cv.notify_all();
        }
    }
}

pub fn configured_level() -> Level {
    Level(CONFIGURED_LEVEL.load(Ordering::SeqCst))
}

pub fn current_level() -> LevelInfo {
    let o = OVERRIDE.lock();
    LevelInfo {
        level: level_name(current_level_value()),
        configured: level_name(configured_level()),
        until: o.until.clone(),
    }
}

pub fn set_level_for(level: Level, d: Duration) -> LevelInfo {
    let mut o = OVERRIDE.lock();

    if let Some(timer) = o.timer.take() {
        timer.stop();
    }

    if level == configured_level() || d.0 <= 0 {
        o.until = None;
        set_level_value(configured_level());
        drop(o);

        return current_level();
    }

    set_level_value(level);
    o.until = Some(Time::now().add(d));
    o.timer = Some(Timer::after_func(d, restore_configured_level));
    drop(o);

    current_level()
}

fn restore_configured_level() {
    {
        let mut o = OVERRIDE.lock();
        o.until = None;
        o.timer = None;
        set_level_value(configured_level());
    }

    info(
        "log level restored",
        kv![
            "component" => "audit",
            "level" => level_name(configured_level()),
            "reason" => "expired",
        ],
    );
}

pub(super) fn cancel_override() {
    let mut o = OVERRIDE.lock();

    if let Some(timer) = o.timer.take() {
        timer.stop();
    }

    o.until = None;
}
