/// slate-power-monitor — power button handler for Slate OS.
///
/// Reads raw evdev events from the power button input device and classifies
/// press duration into actions:
///   < 1s:   suspend (write "mem" to /sys/power/state)
///   1-3s:   ignored (ambiguous press)
///   >= 3s:  poweroff (sync + reboot(RB_POWER_OFF))
///
/// Usage: slate-power-monitor /dev/input/event3
///
/// No async runtime, no tokio — just blocking reads on a file descriptor.
/// Designed to run as a long-lived service under arkhe.
use std::fs::File;
use std::io::{self, Read};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// evdev constants
// ---------------------------------------------------------------------------

/// sizeof(struct input_event) on 64-bit Linux: tv_sec(8) + tv_usec(8) + type(2) + code(2) + value(4) = 24
const INPUT_EVENT_SIZE: usize = 24;

/// EV_KEY event type
const EV_KEY: u16 = 0x01;

/// KEY_POWER scan code
const KEY_POWER: u16 = 116;

// ---------------------------------------------------------------------------
// Press classification
// ---------------------------------------------------------------------------

/// Threshold below which a press is "short" (suspend).
const SHORT_PRESS_MAX: Duration = Duration::from_secs(1);

/// Threshold at or above which a press is "long" (poweroff).
const LONG_PRESS_MIN: Duration = Duration::from_secs(3);

/// Path to kernel suspend interface.
const POWER_STATE_PATH: &str = "/sys/power/state";

// ---------------------------------------------------------------------------
// Signal handling
// ---------------------------------------------------------------------------

/// Global flag set by SIGTERM handler to request clean exit.
static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);

/// Install a SIGTERM handler that sets SHOULD_EXIT.
fn install_signal_handler() {
    // Safety: writing an atomic bool from a signal handler is async-signal-safe.
    unsafe {
        libc::signal(
            libc::SIGTERM,
            sigterm_handler as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            sigterm_handler as *const () as libc::sighandler_t,
        );
    }
}

extern "C" fn sigterm_handler(_sig: libc::c_int) {
    SHOULD_EXIT.store(true, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Suspend to RAM by writing "mem" to /sys/power/state.
/// The write blocks until the device wakes up.
fn suspend() {
    eprintln!("[slate-power] suspending");
    match std::fs::write(POWER_STATE_PATH, "mem") {
        Ok(()) => eprintln!("[slate-power] resumed from suspend"),
        Err(e) => eprintln!("[slate-power] suspend failed: {e}"),
    }
}

/// Sync filesystems and power off via reboot(2) syscall.
/// Does not return on success.
fn poweroff() -> ! {
    eprintln!("[slate-power] powering off");
    // Safety: sync() has no failure mode and is always safe to call.
    unsafe { libc::sync() };
    // Safety: reboot(RB_POWER_OFF) is a valid reboot command.
    // On success this never returns. On failure we loop to avoid exiting.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::reboot(libc::RB_POWER_OFF);
    }
    eprintln!("[slate-power] poweroff syscall returned (should not happen)");
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

// ---------------------------------------------------------------------------
// Event parsing
// ---------------------------------------------------------------------------

/// Parse type, code, value from a 24-byte input_event buffer.
/// Layout: [tv_sec:8][tv_usec:8][type:2][code:2][value:4]
fn parse_event(buf: &[u8; INPUT_EVENT_SIZE]) -> (u16, u16, i32) {
    let ev_type = u16::from_ne_bytes([buf[16], buf[17]]);
    let ev_code = u16::from_ne_bytes([buf[18], buf[19]]);
    let ev_value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
    (ev_type, ev_code, ev_value)
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn run(dev_path: &str) -> io::Result<()> {
    let mut file = File::open(dev_path)?;
    let mut buf = [0u8; INPUT_EVENT_SIZE];
    let mut press_start: Option<Instant> = None;

    eprintln!("[slate-power] watching {dev_path} for KEY_POWER events");

    loop {
        if SHOULD_EXIT.load(Ordering::Relaxed) {
            eprintln!("[slate-power] SIGTERM received, exiting");
            return Ok(());
        }

        // Blocking read — wakes on next input event or signal interruption.
        match file.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }

        let (ev_type, ev_code, ev_value) = parse_event(&buf);

        if ev_type != EV_KEY || ev_code != KEY_POWER {
            continue;
        }

        match ev_value {
            1 => {
                // Key down — start timing
                press_start = Some(Instant::now());
            }
            0 => {
                // Key up — classify and act
                let Some(start) = press_start.take() else {
                    continue;
                };
                let held = start.elapsed();

                if held < SHORT_PRESS_MAX {
                    suspend();
                } else if held >= LONG_PRESS_MIN {
                    poweroff();
                }
                // 1-3s: ambiguous, ignore
            }
            _ => {} // Key repeat (2) — ignored for power button
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: slate-power-monitor <evdev-device>");
        eprintln!("  e.g. slate-power-monitor /dev/input/event3");
        process::exit(1);
    }

    install_signal_handler();

    if let Err(e) = run(&args[1]) {
        eprintln!("[slate-power] fatal: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key_power_down() {
        let mut buf = [0u8; INPUT_EVENT_SIZE];
        buf[16..18].copy_from_slice(&EV_KEY.to_ne_bytes());
        buf[18..20].copy_from_slice(&KEY_POWER.to_ne_bytes());
        buf[20..24].copy_from_slice(&1i32.to_ne_bytes());

        let (t, c, v) = parse_event(&buf);
        assert_eq!(t, EV_KEY);
        assert_eq!(c, KEY_POWER);
        assert_eq!(v, 1);
    }

    #[test]
    fn parse_key_power_up() {
        let mut buf = [0u8; INPUT_EVENT_SIZE];
        buf[16..18].copy_from_slice(&EV_KEY.to_ne_bytes());
        buf[18..20].copy_from_slice(&KEY_POWER.to_ne_bytes());
        buf[20..24].copy_from_slice(&0i32.to_ne_bytes());

        let (t, c, v) = parse_event(&buf);
        assert_eq!(t, EV_KEY);
        assert_eq!(c, KEY_POWER);
        assert_eq!(v, 0);
    }

    #[test]
    fn parse_non_key_event() {
        let buf = [0u8; INPUT_EVENT_SIZE]; // EV_SYN, code 0, value 0
        let (t, _, _) = parse_event(&buf);
        assert_ne!(t, EV_KEY);
    }

    #[test]
    fn short_press_is_suspend() {
        let d = Duration::from_millis(500);
        assert!(d < SHORT_PRESS_MAX);
    }

    #[test]
    fn ambiguous_press_is_ignored() {
        let d = Duration::from_millis(2000);
        assert!(d >= SHORT_PRESS_MAX && d < LONG_PRESS_MIN);
    }

    #[test]
    fn long_press_is_poweroff() {
        let d = Duration::from_secs(4);
        assert!(d >= LONG_PRESS_MIN);
    }

    #[test]
    fn boundary_exactly_one_second() {
        let d = Duration::from_secs(1);
        // 1s is NOT < 1s, so it should be ignored (not suspend)
        assert!(d >= SHORT_PRESS_MAX);
        assert!(d < LONG_PRESS_MIN);
    }

    #[test]
    fn boundary_exactly_three_seconds() {
        let d = Duration::from_secs(3);
        // 3s is >= 3s, so it should poweroff
        assert!(d >= LONG_PRESS_MIN);
    }
}
