//! Backend-independent shortcut description, e.g. `Ctrl+Space`.
//!
//! One `Accelerator` maps onto every native backend: global-hotkey `Code`s,
//! Linux evdev key codes and XDG portal trigger strings.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl Modifiers {
    pub fn is_empty(&self) -> bool {
        !(self.ctrl || self.alt || self.shift || self.super_key)
    }
}

macro_rules! keys {
    ($( $variant:ident => $name:literal, $code:ident, $evdev:ident, $xkb:literal; )*) => {
        /// Keys that may be used as the trigger key of a shortcut.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Key { $( $variant, )* }

        impl Key {
            pub const ALL: &'static [Key] = &[ $( Key::$variant, )* ];

            /// Canonical display name, also accepted by the parser.
            pub fn name(self) -> &'static str {
                match self { $( Key::$variant => $name, )* }
            }

            /// W3C UI Events `code` as used by global-hotkey.
            pub fn code(self) -> keyboard_codes::Code {
                match self { $( Key::$variant => keyboard_codes::Code::$code, )* }
            }

            /// Linux input-event-codes value.
            pub fn evdev_code(self) -> u16 {
                match self { $( Key::$variant => linux_codes::$evdev, )* }
            }

            /// XKB keysym name, used for XDG portal trigger strings.
            pub fn xkb_name(self) -> &'static str {
                match self { $( Key::$variant => $xkb, )* }
            }
        }
    };
}

/// The `Code` type re-exported by the global shortcut plugin.
pub mod keyboard_codes {
    pub use tauri_plugin_global_shortcut::Code;
}

/// Values from `linux/input-event-codes.h`. Kept here so the mapping can be
/// unit-tested on every platform.
#[allow(dead_code)]
pub mod linux_codes {
    pub const KEY_ESC: u16 = 1;
    pub const KEY_1: u16 = 2;
    pub const KEY_2: u16 = 3;
    pub const KEY_3: u16 = 4;
    pub const KEY_4: u16 = 5;
    pub const KEY_5: u16 = 6;
    pub const KEY_6: u16 = 7;
    pub const KEY_7: u16 = 8;
    pub const KEY_8: u16 = 9;
    pub const KEY_9: u16 = 10;
    pub const KEY_0: u16 = 11;
    pub const KEY_MINUS: u16 = 12;
    pub const KEY_EQUAL: u16 = 13;
    pub const KEY_BACKSPACE: u16 = 14;
    pub const KEY_TAB: u16 = 15;
    pub const KEY_Q: u16 = 16;
    pub const KEY_W: u16 = 17;
    pub const KEY_E: u16 = 18;
    pub const KEY_R: u16 = 19;
    pub const KEY_T: u16 = 20;
    pub const KEY_Y: u16 = 21;
    pub const KEY_U: u16 = 22;
    pub const KEY_I: u16 = 23;
    pub const KEY_O: u16 = 24;
    pub const KEY_P: u16 = 25;
    pub const KEY_LEFTBRACE: u16 = 26;
    pub const KEY_RIGHTBRACE: u16 = 27;
    pub const KEY_ENTER: u16 = 28;
    pub const KEY_LEFTCTRL: u16 = 29;
    pub const KEY_A: u16 = 30;
    pub const KEY_S: u16 = 31;
    pub const KEY_D: u16 = 32;
    pub const KEY_F: u16 = 33;
    pub const KEY_G: u16 = 34;
    pub const KEY_H: u16 = 35;
    pub const KEY_J: u16 = 36;
    pub const KEY_K: u16 = 37;
    pub const KEY_L: u16 = 38;
    pub const KEY_SEMICOLON: u16 = 39;
    pub const KEY_APOSTROPHE: u16 = 40;
    pub const KEY_GRAVE: u16 = 41;
    pub const KEY_LEFTSHIFT: u16 = 42;
    pub const KEY_BACKSLASH: u16 = 43;
    pub const KEY_Z: u16 = 44;
    pub const KEY_X: u16 = 45;
    pub const KEY_C: u16 = 46;
    pub const KEY_V: u16 = 47;
    pub const KEY_B: u16 = 48;
    pub const KEY_N: u16 = 49;
    pub const KEY_M: u16 = 50;
    pub const KEY_COMMA: u16 = 51;
    pub const KEY_DOT: u16 = 52;
    pub const KEY_SLASH: u16 = 53;
    pub const KEY_RIGHTSHIFT: u16 = 54;
    pub const KEY_LEFTALT: u16 = 56;
    pub const KEY_SPACE: u16 = 57;
    pub const KEY_F1: u16 = 59;
    pub const KEY_F2: u16 = 60;
    pub const KEY_F3: u16 = 61;
    pub const KEY_F4: u16 = 62;
    pub const KEY_F5: u16 = 63;
    pub const KEY_F6: u16 = 64;
    pub const KEY_F7: u16 = 65;
    pub const KEY_F8: u16 = 66;
    pub const KEY_F9: u16 = 67;
    pub const KEY_F10: u16 = 68;
    pub const KEY_SCROLLLOCK: u16 = 70;
    pub const KEY_F11: u16 = 87;
    pub const KEY_F12: u16 = 88;
    pub const KEY_RIGHTCTRL: u16 = 97;
    pub const KEY_RIGHTALT: u16 = 100;
    pub const KEY_HOME: u16 = 102;
    pub const KEY_UP: u16 = 103;
    pub const KEY_PAGEUP: u16 = 104;
    pub const KEY_LEFT: u16 = 105;
    pub const KEY_RIGHT: u16 = 106;
    pub const KEY_END: u16 = 107;
    pub const KEY_DOWN: u16 = 108;
    pub const KEY_PAGEDOWN: u16 = 109;
    pub const KEY_INSERT: u16 = 110;
    pub const KEY_DELETE: u16 = 111;
    pub const KEY_PAUSE: u16 = 119;
    pub const KEY_LEFTMETA: u16 = 125;
    pub const KEY_RIGHTMETA: u16 = 126;
    pub const KEY_F13: u16 = 183;
    pub const KEY_F14: u16 = 184;
    pub const KEY_F15: u16 = 185;
    pub const KEY_F16: u16 = 186;
    pub const KEY_F17: u16 = 187;
    pub const KEY_F18: u16 = 188;
    pub const KEY_F19: u16 = 189;
    pub const KEY_F20: u16 = 190;
    pub const KEY_F21: u16 = 191;
    pub const KEY_F22: u16 = 192;
    pub const KEY_F23: u16 = 193;
    pub const KEY_F24: u16 = 194;
}

keys! {
    Space => "Space", Space, KEY_SPACE, "space";
    Enter => "Enter", Enter, KEY_ENTER, "Return";
    Tab => "Tab", Tab, KEY_TAB, "Tab";
    Backquote => "`", Backquote, KEY_GRAVE, "grave";
    Minus => "-", Minus, KEY_MINUS, "minus";
    Equal => "=", Equal, KEY_EQUAL, "equal";
    BracketLeft => "[", BracketLeft, KEY_LEFTBRACE, "bracketleft";
    BracketRight => "]", BracketRight, KEY_RIGHTBRACE, "bracketright";
    Backslash => "\\", Backslash, KEY_BACKSLASH, "backslash";
    Semicolon => ";", Semicolon, KEY_SEMICOLON, "semicolon";
    Quote => "'", Quote, KEY_APOSTROPHE, "apostrophe";
    Comma => ",", Comma, KEY_COMMA, "comma";
    Period => ".", Period, KEY_DOT, "period";
    Slash => "/", Slash, KEY_SLASH, "slash";
    Insert => "Insert", Insert, KEY_INSERT, "Insert";
    Delete => "Delete", Delete, KEY_DELETE, "Delete";
    Home => "Home", Home, KEY_HOME, "Home";
    End => "End", End, KEY_END, "End";
    PageUp => "PageUp", PageUp, KEY_PAGEUP, "Prior";
    PageDown => "PageDown", PageDown, KEY_PAGEDOWN, "Next";
    ArrowUp => "Up", ArrowUp, KEY_UP, "Up";
    ArrowDown => "Down", ArrowDown, KEY_DOWN, "Down";
    ArrowLeft => "Left", ArrowLeft, KEY_LEFT, "Left";
    ArrowRight => "Right", ArrowRight, KEY_RIGHT, "Right";
    ScrollLock => "ScrollLock", ScrollLock, KEY_SCROLLLOCK, "Scroll_Lock";
    Pause => "Pause", Pause, KEY_PAUSE, "Pause";
    A => "A", KeyA, KEY_A, "a";
    B => "B", KeyB, KEY_B, "b";
    C => "C", KeyC, KEY_C, "c";
    D => "D", KeyD, KEY_D, "d";
    E => "E", KeyE, KEY_E, "e";
    F => "F", KeyF, KEY_F, "f";
    G => "G", KeyG, KEY_G, "g";
    H => "H", KeyH, KEY_H, "h";
    I => "I", KeyI, KEY_I, "i";
    J => "J", KeyJ, KEY_J, "j";
    K => "K", KeyK, KEY_K, "k";
    L => "L", KeyL, KEY_L, "l";
    M => "M", KeyM, KEY_M, "m";
    N => "N", KeyN, KEY_N, "n";
    O => "O", KeyO, KEY_O, "o";
    P => "P", KeyP, KEY_P, "p";
    Q => "Q", KeyQ, KEY_Q, "q";
    R => "R", KeyR, KEY_R, "r";
    S => "S", KeyS, KEY_S, "s";
    T => "T", KeyT, KEY_T, "t";
    U => "U", KeyU, KEY_U, "u";
    V => "V", KeyV, KEY_V, "v";
    W => "W", KeyW, KEY_W, "w";
    X => "X", KeyX, KEY_X, "x";
    Y => "Y", KeyY, KEY_Y, "y";
    Z => "Z", KeyZ, KEY_Z, "z";
    Digit0 => "0", Digit0, KEY_0, "0";
    Digit1 => "1", Digit1, KEY_1, "1";
    Digit2 => "2", Digit2, KEY_2, "2";
    Digit3 => "3", Digit3, KEY_3, "3";
    Digit4 => "4", Digit4, KEY_4, "4";
    Digit5 => "5", Digit5, KEY_5, "5";
    Digit6 => "6", Digit6, KEY_6, "6";
    Digit7 => "7", Digit7, KEY_7, "7";
    Digit8 => "8", Digit8, KEY_8, "8";
    Digit9 => "9", Digit9, KEY_9, "9";
    F1 => "F1", F1, KEY_F1, "F1";
    F2 => "F2", F2, KEY_F2, "F2";
    F3 => "F3", F3, KEY_F3, "F3";
    F4 => "F4", F4, KEY_F4, "F4";
    F5 => "F5", F5, KEY_F5, "F5";
    F6 => "F6", F6, KEY_F6, "F6";
    F7 => "F7", F7, KEY_F7, "F7";
    F8 => "F8", F8, KEY_F8, "F8";
    F9 => "F9", F9, KEY_F9, "F9";
    F10 => "F10", F10, KEY_F10, "F10";
    F11 => "F11", F11, KEY_F11, "F11";
    F12 => "F12", F12, KEY_F12, "F12";
    F13 => "F13", F13, KEY_F13, "F13";
    F14 => "F14", F14, KEY_F14, "F14";
    F15 => "F15", F15, KEY_F15, "F15";
    F16 => "F16", F16, KEY_F16, "F16";
    F17 => "F17", F17, KEY_F17, "F17";
    F18 => "F18", F18, KEY_F18, "F18";
    F19 => "F19", F19, KEY_F19, "F19";
    F20 => "F20", F20, KEY_F20, "F20";
    F21 => "F21", F21, KEY_F21, "F21";
    F22 => "F22", F22, KEY_F22, "F22";
    F23 => "F23", F23, KEY_F23, "F23";
    F24 => "F24", F24, KEY_F24, "F24";
}

impl Key {
    fn is_function_key(self) -> bool {
        self.name().starts_with('F') && self.name().len() > 1
    }

    fn parse(token: &str) -> Option<Key> {
        let aliases: &[(&str, Key)] = &[
            ("return", Key::Enter),
            ("backtick", Key::Backquote),
            ("grave", Key::Backquote),
            ("arrowup", Key::ArrowUp),
            ("arrowdown", Key::ArrowDown),
            ("arrowleft", Key::ArrowLeft),
            ("arrowright", Key::ArrowRight),
            ("del", Key::Delete),
            ("ins", Key::Insert),
            ("pgup", Key::PageUp),
            ("pgdn", Key::PageDown),
        ];
        let lower = token.to_ascii_lowercase();
        if let Some((_, key)) = aliases.iter().find(|(a, _)| *a == lower) {
            return Some(*key);
        }
        Key::ALL
            .iter()
            .copied()
            .find(|k| k.name().eq_ignore_ascii_case(token))
    }
}

/// A modifier set plus one trigger key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Accelerator {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Default for Accelerator {
    fn default() -> Self {
        Self {
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            key: Key::Space,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AcceleratorError {
    #[error("shortcut is empty")]
    Empty,
    #[error("unknown key or modifier: {0}")]
    UnknownToken(String),
    #[error("shortcut needs exactly one non-modifier key")]
    KeyCount,
    #[error("modifier {0} appears more than once")]
    DuplicateModifier(String),
    #[error("a shortcut without modifiers must use a function key (F1-F24), Pause or ScrollLock")]
    NeedsModifier,
}

impl FromStr for Accelerator {
    type Err = AcceleratorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(AcceleratorError::Empty);
        }
        let mut modifiers = Modifiers::default();
        let mut key = None;
        // `+` itself is not a supported key, so splitting on it is unambiguous.
        for raw in s.split('+') {
            let token = raw.trim();
            if token.is_empty() {
                return Err(AcceleratorError::UnknownToken(s.to_owned()));
            }
            let slot = match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Some(&mut modifiers.ctrl),
                "alt" | "option" => Some(&mut modifiers.alt),
                "shift" => Some(&mut modifiers.shift),
                "super" | "meta" | "cmd" | "command" | "win" | "logo" => {
                    Some(&mut modifiers.super_key)
                }
                _ => None,
            };
            if let Some(flag) = slot {
                if *flag {
                    return Err(AcceleratorError::DuplicateModifier(token.to_owned()));
                }
                *flag = true;
                continue;
            }
            let parsed = Key::parse(token)
                .ok_or_else(|| AcceleratorError::UnknownToken(token.to_owned()))?;
            if key.replace(parsed).is_some() {
                return Err(AcceleratorError::KeyCount);
            }
        }
        let key = key.ok_or(AcceleratorError::KeyCount)?;
        // A bare letter or Space would make the key unusable for typing.
        if modifiers.is_empty()
            && !(key.is_function_key() || matches!(key, Key::Pause | Key::ScrollLock))
        {
            return Err(AcceleratorError::NeedsModifier);
        }
        Ok(Self { modifiers, key })
    }
}

impl fmt::Display for Accelerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.modifiers;
        for (on, name) in [
            (m.ctrl, "Ctrl"),
            (m.alt, "Alt"),
            (m.shift, "Shift"),
            (m.super_key, "Super"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(self.key.name())
    }
}

impl Serialize for Accelerator {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Accelerator {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Accelerator {
    /// Shortcut value for the Tauri global shortcut plugin.
    pub fn to_hotkey(&self) -> tauri_plugin_global_shortcut::Shortcut {
        use tauri_plugin_global_shortcut::Modifiers as M;
        let mut mods = M::empty();
        let m = self.modifiers;
        mods.set(M::CONTROL, m.ctrl);
        mods.set(M::ALT, m.alt);
        mods.set(M::SHIFT, m.shift);
        mods.set(M::SUPER, m.super_key);
        tauri_plugin_global_shortcut::Shortcut::new(
            if mods.is_empty() { None } else { Some(mods) },
            self.key.code(),
        )
    }

    /// Trigger string in the XDG "shortcuts" format, e.g. `CTRL+space`.
    pub fn to_portal_trigger(&self) -> String {
        let m = self.modifiers;
        let mut parts: Vec<&str> = Vec::new();
        for (on, name) in [
            (m.ctrl, "CTRL"),
            (m.alt, "ALT"),
            (m.shift, "SHIFT"),
            (m.super_key, "LOGO"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(self.key.xkb_name());
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Accelerator {
        s.parse().unwrap()
    }

    #[test]
    fn default_is_ctrl_space() {
        assert_eq!(Accelerator::default().to_string(), "Ctrl+Space");
        assert_eq!(parse("Ctrl+Space"), Accelerator::default());
    }

    #[test]
    fn parsing_is_case_insensitive_and_normalises_order() {
        let a = parse(" shift + CONTROL + d ");
        assert_eq!(a.to_string(), "Ctrl+Shift+D");
        assert_eq!(parse(&a.to_string()), a);
    }

    #[test]
    fn modifier_aliases() {
        assert!(parse("Cmd+K").modifiers.super_key);
        assert!(parse("Option+K").modifiers.alt);
        assert!(parse("Win+K").modifiers.super_key);
    }

    #[test]
    fn rejects_invalid_shortcuts() {
        use AcceleratorError::*;
        assert_eq!("".parse::<Accelerator>(), Err(Empty));
        assert_eq!("Ctrl".parse::<Accelerator>(), Err(KeyCount));
        assert_eq!("Ctrl+A+B".parse::<Accelerator>(), Err(KeyCount));
        assert_eq!(
            "Ctrl+Ctrl+A".parse::<Accelerator>(),
            Err(DuplicateModifier("Ctrl".into()))
        );
        assert_eq!("Space".parse::<Accelerator>(), Err(NeedsModifier));
        assert_eq!("A".parse::<Accelerator>(), Err(NeedsModifier));
        assert!(matches!(
            "Ctrl+Nope".parse::<Accelerator>(),
            Err(UnknownToken(_))
        ));
        assert!(matches!(
            "Ctrl++".parse::<Accelerator>(),
            Err(UnknownToken(_))
        ));
    }

    #[test]
    fn bare_function_keys_are_allowed() {
        assert_eq!(parse("F9").to_string(), "F9");
        assert_eq!(parse("Pause").to_string(), "Pause");
    }

    #[test]
    fn every_key_round_trips_through_its_name() {
        for key in Key::ALL {
            let a = Accelerator {
                modifiers: Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                key: *key,
            };
            assert_eq!(parse(&a.to_string()), a, "key {key:?}");
        }
    }

    #[test]
    fn portal_trigger_format() {
        assert_eq!(Accelerator::default().to_portal_trigger(), "CTRL+space");
        assert_eq!(parse("Super+Alt+F9").to_portal_trigger(), "ALT+LOGO+F9");
    }

    #[test]
    fn evdev_mapping_matches_kernel_codes() {
        assert_eq!(Key::Space.evdev_code(), 57);
        assert_eq!(Key::A.evdev_code(), 30);
        assert_eq!(Key::F12.evdev_code(), 88);
    }

    #[test]
    fn serde_uses_display_string() {
        let json = serde_json::to_string(&Accelerator::default()).unwrap();
        assert_eq!(json, "\"Ctrl+Space\"");
        let back: Accelerator = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Accelerator::default());
        assert!(serde_json::from_str::<Accelerator>("\"Space\"").is_err());
    }

    #[test]
    fn hotkey_conversion_keeps_modifiers() {
        use tauri_plugin_global_shortcut::{Code, Modifiers as M};
        let hk = parse("Ctrl+Shift+Space").to_hotkey();
        assert_eq!(hk.key, Code::Space);
        assert_eq!(hk.mods, M::CONTROL | M::SHIFT);
    }
}
