//! Synthetic paste keystrokes.
//!
//! | Session | Mechanism |
//! | --- | --- |
//! | Windows | `SendInput` (enigo) |
//! | macOS | `CGEvent` (enigo). Needs Accessibility permission. |
//! | X11 | XTest (enigo, x11rb) |
//! | Wayland | `zwp_virtual_keyboard_v1` (enigo; wlroots, niri, KDE, Hyprland), then `/dev/uinput` |
//!
//! enigo sends every event through every backend it connected to, so on
//! Wayland it is pointed at an invalid X11 display (and vice versa). This stops
//! XWayland apps from receiving the paste twice.

use std::time::Duration;

use super::InsertError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteChord {
    /// Ctrl+V (Windows, Linux).
    CtrlV,
    /// Ctrl+Shift+V (Linux terminal emulators).
    CtrlShiftV,
    /// Cmd+V (macOS).
    CmdV,
}

impl PasteChord {
    /// Ctrl+V becomes Cmd+V on macOS.
    pub fn for_platform(self) -> Self {
        if cfg!(target_os = "macos") {
            PasteChord::CmdV
        } else {
            self
        }
    }
}

/// Application ids / window classes of terminal emulators, which paste with
/// Ctrl+Shift+V on Linux. Compared case-insensitively.
const TERMINALS: &[&str] = &[
    "kitty",
    "alacritty",
    "foot",
    "footclient",
    "org.wezfurlong.wezterm",
    "wezterm",
    "com.mitchellh.ghostty",
    "ghostty",
    "org.gnome.terminal",
    "gnome-terminal-server",
    "org.gnome.console",
    "org.gnome.ptyxis",
    "org.kde.konsole",
    "konsole",
    "xterm",
    "urxvt",
    "tilix",
    "terminator",
    "xfce4-terminal",
    "st",
    "st-256color",
    "rio",
];

/// The paste shortcut for the focused application.
pub fn paste_chord(app_id: Option<&str>) -> PasteChord {
    if cfg!(target_os = "macos") {
        return PasteChord::CmdV;
    }
    if cfg!(target_os = "linux")
        && let Some(id) = app_id
        && TERMINALS.iter().any(|t| t.eq_ignore_ascii_case(id))
    {
        return PasteChord::CtrlShiftV;
    }
    PasteChord::CtrlV
}

pub trait Keystroker: Send {
    fn name(&self) -> &'static str;
    fn paste(&mut self, chord: PasteChord) -> Result<(), InsertError>;
}

/// Keystrokes through enigo.
pub struct EnigoKeys {
    enigo: enigo::Enigo,
    name: &'static str,
}

impl EnigoKeys {
    pub fn new() -> Result<Self, InsertError> {
        let mut settings = enigo::Settings {
            // Ask for Accessibility permission on macOS when it is missing.
            open_prompt_to_get_permissions: true,
            ..Default::default()
        };
        let mut name = "enigo";
        if cfg!(target_os = "linux") {
            // Unreachable display names keep enigo on exactly one protocol.
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                settings.x11_display = Some("voiceflow-disabled:99".into());
                name = "wayland-virtual-keyboard";
            } else {
                settings.wayland_display = Some("voiceflow-disabled".into());
                name = "x11-xtest";
            }
        }
        let enigo = enigo::Enigo::new(&settings).map_err(|e| {
            let msg = e.to_string();
            if msg.to_ascii_lowercase().contains("permission") {
                InsertError::PermissionDenied(msg)
            } else {
                InsertError::PasteUnavailable(msg)
            }
        })?;
        Ok(Self { enigo, name })
    }
}

impl Keystroker for EnigoKeys {
    fn name(&self) -> &'static str {
        self.name
    }

    fn paste(&mut self, chord: PasteChord) -> Result<(), InsertError> {
        use enigo::{Direction, Key, Keyboard};
        let mods: &[Key] = match chord {
            PasteChord::CtrlV => &[Key::Control],
            PasteChord::CtrlShiftV => &[Key::Control, Key::Shift],
            PasteChord::CmdV => &[Key::Meta],
        };
        let map = |e: enigo::InputError| InsertError::PasteUnavailable(e.to_string());
        let mut pressed = Vec::new();
        let mut result = Ok(());
        for m in mods {
            match self.enigo.key(*m, Direction::Press) {
                Ok(()) => pressed.push(*m),
                Err(e) => {
                    result = Err(map(e));
                    break;
                }
            }
        }
        if result.is_ok() {
            result = self
                .enigo
                .key(Key::Unicode('v'), Direction::Click)
                .map_err(map);
        }
        // Always release modifiers, even after an error.
        for m in pressed.iter().rev() {
            let _ = self.enigo.key(*m, Direction::Release);
        }
        result
    }
}

/// Kernel-level virtual keyboard (`/dev/uinput`). Works on any Linux
/// compositor if the user may write to `/dev/uinput`. It sends key codes, so
/// it assumes `V` sits at the QWERTY position (true for QWERTY, QWERTZ and AZERTY).
#[cfg(target_os = "linux")]
pub struct UinputKeys {
    device: evdev::uinput::VirtualDevice,
}

#[cfg(target_os = "linux")]
impl UinputKeys {
    pub fn new() -> Result<Self, InsertError> {
        use evdev::{AttributeSet, KeyCode};
        let mut keys = AttributeSet::<KeyCode>::new();
        for k in [
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_V,
        ] {
            keys.insert(k);
        }
        let device = evdev::uinput::VirtualDevice::builder()
            .and_then(|b| b.name("VoiceFlow paste keyboard").with_keys(&keys))
            .and_then(|b| b.build())
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    InsertError::PermissionDenied("no write access to /dev/uinput".into())
                } else {
                    InsertError::PasteUnavailable(format!("uinput: {e}"))
                }
            })?;
        Ok(Self { device })
    }

    fn emit(&mut self, code: evdev::KeyCode, value: i32) -> Result<(), InsertError> {
        use evdev::{EventType, InputEvent};
        self.device
            .emit(&[InputEvent::new(EventType::KEY.0, code.0, value)])
            .map_err(|e| InsertError::PasteUnavailable(format!("uinput: {e}")))
    }
}

#[cfg(target_os = "linux")]
impl Keystroker for UinputKeys {
    fn name(&self) -> &'static str {
        "uinput"
    }

    fn paste(&mut self, chord: PasteChord) -> Result<(), InsertError> {
        use evdev::KeyCode as K;
        let mods: &[K] = match chord {
            PasteChord::CtrlShiftV => &[K::KEY_LEFTCTRL, K::KEY_LEFTSHIFT],
            _ => &[K::KEY_LEFTCTRL],
        };
        let mut result = Ok(());
        let mut pressed = Vec::new();
        for m in mods {
            match self.emit(*m, 1) {
                Ok(()) => pressed.push(*m),
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        if result.is_ok() {
            result = self.emit(K::KEY_V, 1).and_then(|()| {
                std::thread::sleep(Duration::from_millis(8));
                self.emit(K::KEY_V, 0)
            });
        }
        for m in pressed.iter().rev() {
            let _ = self.emit(*m, 0);
        }
        result
    }
}

/// Creates the best available keystroke backend, or reports why none exists.
/// `VOICEFLOW_PASTE_BACKEND=uinput` forces the uinput backend on Linux (testing).
pub fn detect() -> Result<Box<dyn Keystroker>, InsertError> {
    #[cfg(target_os = "linux")]
    if std::env::var("VOICEFLOW_PASTE_BACKEND").as_deref() == Ok("uinput") {
        let k = UinputKeys::new()?;
        std::thread::sleep(Duration::from_millis(300));
        return Ok(Box::new(k));
    }
    let primary = EnigoKeys::new();
    match primary {
        Ok(k) => Ok(Box::new(k)),
        #[cfg(target_os = "linux")]
        Err(e) => {
            tracing::info!(error = %e, "enigo unavailable; trying uinput");
            let k = UinputKeys::new()?;
            // The compositor needs a moment to pick up a new input device.
            std::thread::sleep(Duration::from_millis(300));
            Ok(Box::new(k))
        }
        #[cfg(not(target_os = "linux"))]
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_use_ctrl_shift_v_on_linux() {
        if cfg!(target_os = "linux") {
            assert_eq!(paste_chord(Some("kitty")), PasteChord::CtrlShiftV);
            assert_eq!(paste_chord(Some("Alacritty")), PasteChord::CtrlShiftV);
            assert_eq!(
                paste_chord(Some("org.gnome.Terminal")),
                PasteChord::CtrlShiftV
            );
            assert_eq!(paste_chord(Some("firefox")), PasteChord::CtrlV);
            assert_eq!(paste_chord(None), PasteChord::CtrlV);
        }
    }

    #[test]
    fn macos_always_uses_cmd_v() {
        if cfg!(target_os = "macos") {
            assert_eq!(paste_chord(Some("kitty")), PasteChord::CmdV);
        }
    }
}
