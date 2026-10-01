//! Give the DSP thread real-time priority, so a busy desktop (a video call with screen share, a
//! compile) cannot starve it. Starvation shows up as crackling in "Sordino Mic".
//!
//! Order of attempts, like PipeWire itself:
//! 1. `SCHED_FIFO` directly (works when the user has an rtprio limit, e.g. via
//!    `realtime-privileges` on Arch or `audio` group limits);
//! 2. the RealtimeKit D-Bus service (`MakeThreadRealtime`), the default on most desktops;
//! 3. RealtimeKit's `MakeThreadHighPriority` (a better nice level);
//! 4. stay as we are.

use std::fmt;

/// Priority we ask for. Below PipeWire's own data threads, far above normal threads.
const RT_PRIORITY: u32 = 15;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Priority {
    Normal,
    High,
    Realtime,
}

impl Priority {
    pub fn as_u8(self) -> u8 {
        match self {
            Priority::Normal => 0,
            Priority::High => 1,
            Priority::Realtime => 2,
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Priority::Normal => "normal (could not be raised)",
            Priority::High => "high (nice)",
            Priority::Realtime => "real-time",
        })
    }
}

fn gettid() -> u64 {
    // SAFETY: plain syscall without arguments.
    unsafe { libc::syscall(libc::SYS_gettid) as u64 }
}

fn try_sched_fifo(prio: i32) -> bool {
    let param = libc::sched_param {
        sched_priority: prio,
    };
    // SAFETY: `param` is a valid sched_param and 0 means "the calling thread".
    unsafe {
        libc::sched_setscheduler(0, libc::SCHED_FIFO | libc::SCHED_RESET_ON_FORK, &param) == 0
    }
}

/// RealtimeKit refuses threads without an RLIMIT_RTTIME, because a runaway real-time thread
/// would freeze the machine. The kernel kills the process if the limit is exceeded, so the DSP
/// loop must block regularly (it does).
fn set_rttime_limit() {
    let lim = libc::rlimit {
        rlim_cur: 200_000,
        rlim_max: 200_000,
    };
    // SAFETY: `lim` is a valid rlimit.
    unsafe {
        libc::setrlimit(libc::RLIMIT_RTTIME, &lim);
    }
}

fn via_rtkit(tid: u64) -> Option<Priority> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let call = |method: &str, body: &(u64, u32)| {
        conn.call_method(
            Some("org.freedesktop.RealtimeKit1"),
            "/org/freedesktop/RealtimeKit1",
            Some("org.freedesktop.RealtimeKit1"),
            method,
            body,
        )
    };
    set_rttime_limit();
    if call("MakeThreadRealtime", &(tid, RT_PRIORITY)).is_ok()
        || call("MakeThreadRealtime", &(tid, 10)).is_ok()
    {
        return Some(Priority::Realtime);
    }
    let nice = conn.call_method(
        Some("org.freedesktop.RealtimeKit1"),
        "/org/freedesktop/RealtimeKit1",
        Some("org.freedesktop.RealtimeKit1"),
        "MakeThreadHighPriority",
        &(tid, -11i32),
    );
    nice.is_ok().then_some(Priority::High)
}

/// Raise the priority of the calling thread as far as the system allows.
pub fn promote_current_thread() -> Priority {
    // Debug switch for A/B tests of the priority.
    if std::env::var_os("SORDINO_NO_RT").is_some() {
        return Priority::Normal;
    }
    set_rttime_limit();
    if try_sched_fifo(RT_PRIORITY as i32) {
        return Priority::Realtime;
    }
    via_rtkit(gettid()).unwrap_or(Priority::Normal)
}
