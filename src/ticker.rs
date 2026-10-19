//! Background ticker. It never touches windows or input itself: it only posts
//! a tick message to the UI thread, where all window work happens. Posted
//! messages are also dispatched by modal loops (menus, file dialogs), so the
//! zoom keeps working while those are open.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, INFINITE, SetWaitableTimer,
    TIMER_ALL_ACCESS, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

pub struct Ticker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    stop: AtomicBool,
    pending: AtomicBool,
    interval_ms: AtomicU32,
}

impl Ticker {
    pub fn start(target: HWND, msg: u32, interval_ms: u32) -> Self {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            interval_ms: AtomicU32::new(interval_ms),
        });
        let s = shared.clone();
        let target = target as usize; // HWND isn't Send; it's only used as an opaque id here.
        let thread = std::thread::spawn(move || run(&s, target as HWND, msg));
        Self {
            shared,
            thread: Some(thread),
        }
    }

    pub fn set_interval(&self, ms: u32) {
        self.shared.interval_ms.store(ms, Ordering::Relaxed);
    }

    /// Call when handling the tick message, so the next one can be posted.
    /// At most one tick is queued at a time, so a busy UI thread is never flooded.
    pub fn ack(&self) {
        self.shared.pending.store(false, Ordering::Release);
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(s: &Shared, target: HWND, msg: u32) {
    unsafe {
        let mut timer = CreateWaitableTimerExW(
            std::ptr::null(),
            std::ptr::null(),
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            TIMER_ALL_ACCESS,
        );
        if timer.is_null() {
            // High-resolution timers need Windows 10 1803+.
            timer = CreateWaitableTimerExW(std::ptr::null(), std::ptr::null(), 0, TIMER_ALL_ACCESS);
        }
        while !s.stop.load(Ordering::Relaxed) {
            let ms = s.interval_ms.load(Ordering::Relaxed).max(1);
            if timer.is_null() {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            } else {
                let due = -(ms as i64 * 10_000); // relative, in 100 ns units
                SetWaitableTimer(timer, &due, 0, None, std::ptr::null(), 0);
                WaitForSingleObject(timer, INFINITE);
            }
            if !s.pending.swap(true, Ordering::AcqRel) && PostMessageW(target, msg, 0, 0) == 0 {
                s.pending.store(false, Ordering::Release);
            }
        }
        if !timer.is_null() {
            CloseHandle(timer);
        }
    }
}
