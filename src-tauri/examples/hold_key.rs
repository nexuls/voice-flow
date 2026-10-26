//! Development aid (Linux): holds a key on a virtual uinput keyboard.
//!
//!     cargo run --example hold_key -- F19 1500
//!
//! Lets the push-to-talk flow be tested without touching the keyboard. Needs
//! write access to /dev/uinput.
//!
//! Only F19 is offered: in the default XKB "inet" map, F13-F18 and F20-F24
//! are media/launcher keys (F20 is XF86AudioMicMute), and the compositor
//! acts on them because it also sees the virtual keyboard.

#[cfg(target_os = "linux")]
fn main() {
    use std::time::Duration;

    use evdev::uinput::VirtualDevice;
    use evdev::{AttributeSet, EventType, InputEvent, KeyCode};

    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "F19".into());
    let hold_ms: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1000);
    let code = match name.as_str() {
        "F19" => KeyCode::KEY_F19,
        other => panic!("unsupported key {other}; use F19"),
    };
    let mut keys = AttributeSet::<KeyCode>::new();
    for k in [
        KeyCode::KEY_A,
        KeyCode::KEY_SPACE,
        KeyCode::KEY_LEFTCTRL,
        code,
    ] {
        keys.insert(k);
    }
    let mut dev = VirtualDevice::builder()
        .and_then(|b| b.name("voiceflow-hold-key").with_keys(&keys))
        .and_then(|b| b.build())
        .expect("create uinput device");
    // The app rescans /dev/input every 2 s, so wait for it to see us.
    std::thread::sleep(Duration::from_millis(2500));
    let ev = |v| [InputEvent::new(EventType::KEY.0, code.0, v)];
    dev.emit(&ev(1)).unwrap();
    std::thread::sleep(Duration::from_millis(hold_ms));
    dev.emit(&ev(0)).unwrap();
    std::thread::sleep(Duration::from_millis(200));
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("hold_key is Linux-only");
}
