//! Linux evdev shortcut backend.
//!
//! Reads key events from every keyboard under `/dev/input` and detects the
//! accelerator itself. It works on any Wayland compositor and on X11, but:
//! - it needs read access to `/dev/input/event*` (usually the `input` group);
//! - it does not grab the keys, so the focused application also receives the
//!   keystroke (for example a space for `Ctrl+Space`).
//!
//! Only the configured combination is evaluated. No key events are logged or
//! stored.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use evdev::{Device, EventSummary, KeyCode};

use super::accelerator::linux_codes as k;
use super::{
    Accelerator, BackendKind, KeyState, ShortcutError, ShortcutEvent, ShortcutService, ShortcutSink,
};

const RESCAN_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT_MS: i32 = 200;

/// Accelerator matching over the union of all keyboards.
#[derive(Debug)]
pub struct ComboTracker {
    accel: Accelerator,
    /// Number of devices on which a key is currently held.
    held: HashMap<u16, u32>,
    active: bool,
}

#[derive(Clone, Copy)]
enum ModifierGroup {
    Ctrl,
    Alt,
    Shift,
    Super,
}

impl ModifierGroup {
    fn codes(self) -> [u16; 2] {
        match self {
            ModifierGroup::Ctrl => [k::KEY_LEFTCTRL, k::KEY_RIGHTCTRL],
            ModifierGroup::Alt => [k::KEY_LEFTALT, k::KEY_RIGHTALT],
            ModifierGroup::Shift => [k::KEY_LEFTSHIFT, k::KEY_RIGHTSHIFT],
            ModifierGroup::Super => [k::KEY_LEFTMETA, k::KEY_RIGHTMETA],
        }
    }

    fn of(code: u16) -> Option<Self> {
        [Self::Ctrl, Self::Alt, Self::Shift, Self::Super]
            .into_iter()
            .find(|g| g.codes().contains(&code))
    }
}

impl ComboTracker {
    pub fn new(accel: Accelerator) -> Self {
        Self {
            accel,
            held: HashMap::new(),
            active: false,
        }
    }

    fn is_held(&self, code: u16) -> bool {
        self.held.get(&code).is_some_and(|n| *n > 0)
    }

    fn group_held(&self, g: ModifierGroup) -> bool {
        g.codes().iter().any(|c| self.is_held(*c))
    }

    fn required(&self, g: ModifierGroup) -> bool {
        let m = self.accel.modifiers;
        match g {
            ModifierGroup::Ctrl => m.ctrl,
            ModifierGroup::Alt => m.alt,
            ModifierGroup::Shift => m.shift,
            ModifierGroup::Super => m.super_key,
        }
    }

    /// True if exactly the required modifiers are held.
    fn modifiers_match(&self) -> bool {
        [
            ModifierGroup::Ctrl,
            ModifierGroup::Alt,
            ModifierGroup::Shift,
            ModifierGroup::Super,
        ]
        .into_iter()
        .all(|g| self.group_held(g) == self.required(g))
    }

    /// Feeds one key transition. Returns a state change if the shortcut was
    /// activated or released. Key repeats must not be passed in.
    pub fn on_key(&mut self, code: u16, pressed: bool) -> Option<KeyState> {
        let entry = self.held.entry(code).or_insert(0);
        if pressed {
            *entry += 1;
        } else {
            *entry = entry.saturating_sub(1);
        }
        let trigger = self.accel.key.evdev_code();
        if !self.active {
            // Activate only on the trigger key going down with the exact
            // modifiers, like a native hotkey.
            if pressed && code == trigger && self.modifiers_match() {
                self.active = true;
                return Some(KeyState::Pressed);
            }
        } else {
            let released_trigger = !pressed && code == trigger && !self.is_held(trigger);
            let released_modifier = !pressed
                && ModifierGroup::of(code).is_some_and(|g| self.required(g) && !self.group_held(g));
            if released_trigger || released_modifier {
                self.active = false;
                return Some(KeyState::Released);
            }
        }
        None
    }

    /// Releases every key from a device that disappeared.
    pub fn release_all(&mut self, codes: impl IntoIterator<Item = u16>) -> Option<KeyState> {
        let mut out = None;
        for code in codes {
            if let Some(s) = self.on_key(code, false) {
                out = Some(s);
            }
        }
        out
    }
}

struct Keyboard {
    device: Device,
    held: Vec<u16>,
}

fn is_keyboard(device: &Device) -> bool {
    device.supported_keys().is_some_and(|keys| {
        keys.contains(KeyCode::KEY_A)
            && keys.contains(KeyCode::KEY_SPACE)
            && keys.contains(KeyCode::KEY_LEFTCTRL)
    })
}

/// Opens every readable keyboard not already open. Returns how many device
/// nodes were present and how many were denied.
fn scan(open: &mut HashMap<PathBuf, Keyboard>) -> (usize, usize) {
    let mut present = 0;
    let mut denied = 0;
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_event = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("event"));
        if !is_event || open.contains_key(&path) {
            continue;
        }
        present += 1;
        match Device::open(&path) {
            Ok(device) if is_keyboard(&device) => {
                if device.set_nonblocking(true).is_ok() {
                    tracing::debug!(device = ?device.name(), "evdev keyboard opened");
                    open.insert(
                        path,
                        Keyboard {
                            device,
                            held: Vec::new(),
                        },
                    );
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied += 1,
            Err(_) => {}
        }
    }
    (present, denied)
}

fn run(
    accel: Accelerator,
    sink: ShortcutSink,
    stop: Arc<AtomicBool>,
    mut keyboards: HashMap<PathBuf, Keyboard>,
) {
    let mut tracker = ComboTracker::new(accel);
    let mut last_scan = Instant::now();
    let emit = |state| {
        sink(ShortcutEvent {
            state,
            at: Instant::now(),
        })
    };

    while !stop.load(Ordering::Relaxed) {
        if last_scan.elapsed() >= RESCAN_INTERVAL {
            scan(&mut keyboards);
            last_scan = Instant::now();
        }
        let paths: Vec<PathBuf> = keyboards.keys().cloned().collect();
        let mut fds: Vec<libc::pollfd> = paths
            .iter()
            .map(|p| libc::pollfd {
                fd: keyboards[p].device.as_raw_fd() as RawFd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        if fds.is_empty() {
            std::thread::sleep(Duration::from_millis(POLL_TIMEOUT_MS as u64));
            continue;
        }
        // SAFETY: `fds` is a valid, exclusively borrowed slice of pollfd.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, POLL_TIMEOUT_MS) };
        if n <= 0 {
            continue;
        }
        for (pfd, path) in fds.iter().zip(&paths) {
            if pfd.revents == 0 {
                continue;
            }
            let kb = keyboards.get_mut(path).expect("path from map");
            let mut gone = pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0;
            match kb.device.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        if let EventSummary::Key(_, code, value) = ev.destructure() {
                            // 1 = press, 0 = release, 2 = autorepeat (ignored).
                            let pressed = match value {
                                1 => true,
                                0 => false,
                                _ => continue,
                            };
                            if pressed {
                                kb.held.push(code.code());
                            } else {
                                kb.held.retain(|c| *c != code.code());
                            }
                            if let Some(state) = tracker.on_key(code.code(), pressed) {
                                emit(state);
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => gone = true,
            }
            if gone {
                tracing::debug!("evdev keyboard removed");
                let kb = keyboards.remove(path).expect("path from map");
                if let Some(state) = tracker.release_all(kb.held) {
                    emit(state);
                }
            }
        }
    }
}

#[derive(Default)]
pub struct EvdevBackend {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl EvdevBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ShortcutService for EvdevBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Evdev
    }

    fn register(&mut self, accel: Accelerator, sink: ShortcutSink) -> Result<(), ShortcutError> {
        self.unregister();
        let mut keyboards = HashMap::new();
        let (present, denied) = scan(&mut keyboards);
        if keyboards.is_empty() {
            return Err(if denied > 0 {
                ShortcutError::PermissionDenied(
                    "cannot read /dev/input; add your user to the 'input' group".into(),
                )
            } else {
                ShortcutError::Unavailable(format!("no keyboard found ({present} input devices)"))
            });
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("shortcut-evdev".into())
            .spawn(move || run(accel, sink, stop, keyboards))
            .map_err(|e| ShortcutError::Failed(e.to_string()))?;
        self.thread = Some(thread);
        Ok(())
    }

    fn unregister(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // The loop wakes at least every POLL_TIMEOUT_MS.
            let _ = t.join();
        }
    }
}

impl Drop for EvdevBackend {
    fn drop(&mut self) {
        self.unregister();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(s: &str) -> ComboTracker {
        ComboTracker::new(s.parse().unwrap())
    }

    #[test]
    fn ctrl_space_press_and_release_trigger() {
        let mut t = tracker("Ctrl+Space");
        assert_eq!(t.on_key(k::KEY_LEFTCTRL, true), None);
        assert_eq!(t.on_key(k::KEY_SPACE, true), Some(KeyState::Pressed));
        assert_eq!(t.on_key(k::KEY_SPACE, false), Some(KeyState::Released));
        assert_eq!(t.on_key(k::KEY_LEFTCTRL, false), None);
    }

    #[test]
    fn releasing_modifier_first_ends_the_shortcut() {
        let mut t = tracker("Ctrl+Space");
        t.on_key(k::KEY_RIGHTCTRL, true);
        t.on_key(k::KEY_SPACE, true);
        assert_eq!(t.on_key(k::KEY_RIGHTCTRL, false), Some(KeyState::Released));
        assert_eq!(t.on_key(k::KEY_SPACE, false), None);
    }

    #[test]
    fn extra_modifier_does_not_match() {
        let mut t = tracker("Ctrl+Space");
        t.on_key(k::KEY_LEFTCTRL, true);
        t.on_key(k::KEY_LEFTSHIFT, true);
        assert_eq!(t.on_key(k::KEY_SPACE, true), None);
    }

    #[test]
    fn modifier_pressed_after_trigger_does_not_activate() {
        let mut t = tracker("Ctrl+Space");
        t.on_key(k::KEY_SPACE, true);
        assert_eq!(t.on_key(k::KEY_LEFTCTRL, true), None);
    }

    #[test]
    fn either_side_modifier_works_and_one_side_release_keeps_it() {
        let mut t = tracker("Ctrl+Space");
        t.on_key(k::KEY_LEFTCTRL, true);
        t.on_key(k::KEY_RIGHTCTRL, true);
        assert_eq!(t.on_key(k::KEY_SPACE, true), Some(KeyState::Pressed));
        assert_eq!(
            t.on_key(k::KEY_LEFTCTRL, false),
            None,
            "right ctrl still held"
        );
        assert_eq!(t.on_key(k::KEY_RIGHTCTRL, false), Some(KeyState::Released));
    }

    #[test]
    fn two_keyboards_holding_trigger() {
        let mut t = tracker("F9");
        assert_eq!(t.on_key(k::KEY_F9, true), Some(KeyState::Pressed));
        assert_eq!(t.on_key(k::KEY_F9, true), None, "second keyboard");
        assert_eq!(t.on_key(k::KEY_F9, false), None, "still held on one");
        assert_eq!(t.on_key(k::KEY_F9, false), Some(KeyState::Released));
    }

    #[test]
    fn unplugged_device_releases_its_keys() {
        let mut t = tracker("Ctrl+Space");
        t.on_key(k::KEY_LEFTCTRL, true);
        t.on_key(k::KEY_SPACE, true);
        assert_eq!(
            t.release_all([k::KEY_LEFTCTRL, k::KEY_SPACE]),
            Some(KeyState::Released)
        );
        // A fresh press works afterwards.
        t.on_key(k::KEY_LEFTCTRL, true);
        assert_eq!(t.on_key(k::KEY_SPACE, true), Some(KeyState::Pressed));
    }

    #[test]
    fn spurious_release_does_not_underflow() {
        let mut t = tracker("Ctrl+Space");
        assert_eq!(t.on_key(k::KEY_LEFTCTRL, false), None);
        t.on_key(k::KEY_LEFTCTRL, true);
        assert_eq!(t.on_key(k::KEY_SPACE, true), Some(KeyState::Pressed));
    }
}

/// Hardware-path test. It creates a uinput keyboard and checks that the backend
/// reports press and release. It needs write access to `/dev/uinput` and read
/// access to `/dev/input`. Run with `cargo test -- --ignored uinput`.
#[cfg(test)]
mod uinput_tests {
    use std::sync::Mutex;

    use evdev::uinput::VirtualDevice;
    use evdev::{AttributeSet, EventType, InputEvent};

    use super::*;

    fn key(dev: &mut VirtualDevice, code: KeyCode, value: i32) {
        dev.emit(&[InputEvent::new(EventType::KEY.0, code.0, value)])
            .unwrap();
    }

    #[test]
    #[ignore = "needs /dev/uinput and /dev/input access"]
    fn uinput_press_release_round_trip() {
        let mut keys = AttributeSet::<KeyCode>::new();
        for k in [
            KeyCode::KEY_A,
            KeyCode::KEY_SPACE,
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_F20,
        ] {
            keys.insert(k);
        }
        let mut dev = VirtualDevice::builder()
            .unwrap()
            .name("voiceflow-test-keyboard")
            .with_keys(&keys)
            .unwrap()
            .build()
            .unwrap();
        // Give udev time to create the device node.
        std::thread::sleep(Duration::from_millis(500));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = seen.clone();
        let sink: ShortcutSink = Arc::new(move |ev| sink_seen.lock().unwrap().push(ev.state));
        let mut backend = EvdevBackend::new();
        backend.register("F20".parse().unwrap(), sink).unwrap();

        // F20 is unbound on typical desktops, so injecting it is harmless.
        key(&mut dev, KeyCode::KEY_F20, 1);
        key(&mut dev, KeyCode::KEY_F20, 2); // autorepeat must be ignored
        std::thread::sleep(Duration::from_millis(100));
        key(&mut dev, KeyCode::KEY_F20, 0);
        std::thread::sleep(Duration::from_millis(400));

        backend.unregister();
        assert_eq!(
            *seen.lock().unwrap(),
            vec![KeyState::Pressed, KeyState::Released]
        );
    }
}
