//! Focused-window detection, used to avoid pasting into the wrong window.
//!
//! | Platform | Source |
//! | --- | --- |
//! | Windows | `GetForegroundWindow` |
//! | X11 | `_NET_ACTIVE_WINDOW` + `WM_CLASS` |
//! | Wayland: niri | niri IPC (`$NIRI_SOCKET`, `FocusedWindow`) |
//! | Wayland: Hyprland | Hyprland IPC (`j/activewindow`) |
//! | Wayland: others, macOS | not available (no focus check) |
//!
//! Only window ids and application ids are read, never titles or content.

#[cfg(unix)]
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusInfo {
    /// Stable identifier of the focused window for comparison.
    pub id: String,
    /// Application id / class (for example `kitty`), used to choose the paste chord.
    pub app_id: Option<String>,
}

pub trait FocusProvider: Send {
    fn name(&self) -> &'static str;
    /// The focused window, or `None` if it cannot be determined.
    fn focused(&mut self) -> Option<FocusInfo>;
}

pub struct NoFocus;

impl FocusProvider for NoFocus {
    fn name(&self) -> &'static str {
        "none"
    }
    fn focused(&mut self) -> Option<FocusInfo> {
        None
    }
}

#[cfg(unix)]
const IPC_TIMEOUT: Duration = Duration::from_millis(250);

#[cfg(unix)]
fn unix_request(path: &PathBuf, request: &[u8], read_line: bool) -> Option<String> {
    let mut s = UnixStream::connect(path).ok()?;
    s.set_read_timeout(Some(IPC_TIMEOUT)).ok()?;
    s.set_write_timeout(Some(IPC_TIMEOUT)).ok()?;
    s.write_all(request).ok()?;
    let mut out = String::new();
    if read_line {
        BufReader::new(s).read_line(&mut out).ok()?;
    } else {
        s.read_to_string(&mut out).ok()?;
    }
    Some(out)
}

/// Parses a niri `FocusedWindow` reply.
pub fn parse_niri(reply: &str) -> Option<FocusInfo> {
    let v: serde_json::Value = serde_json::from_str(reply).ok()?;
    let w = v.get("Ok")?.get("FocusedWindow")?;
    if w.is_null() {
        return None;
    }
    Some(FocusInfo {
        id: w.get("id")?.to_string(),
        app_id: w.get("app_id").and_then(|a| a.as_str()).map(str::to_owned),
    })
}

/// Parses a Hyprland `j/activewindow` reply.
pub fn parse_hyprland(reply: &str) -> Option<FocusInfo> {
    let v: serde_json::Value = serde_json::from_str(reply).ok()?;
    let id = v.get("address")?.as_str()?.to_owned();
    Some(FocusInfo {
        id,
        app_id: v.get("class").and_then(|c| c.as_str()).map(str::to_owned),
    })
}

#[cfg(unix)]
pub struct NiriFocus {
    socket: PathBuf,
}

#[cfg(unix)]
impl FocusProvider for NiriFocus {
    fn name(&self) -> &'static str {
        "niri"
    }
    fn focused(&mut self) -> Option<FocusInfo> {
        parse_niri(&unix_request(&self.socket, b"\"FocusedWindow\"\n", true)?)
    }
}

#[cfg(unix)]
pub struct HyprlandFocus {
    socket: PathBuf,
}

#[cfg(unix)]
impl FocusProvider for HyprlandFocus {
    fn name(&self) -> &'static str {
        "hyprland"
    }
    fn focused(&mut self) -> Option<FocusInfo> {
        parse_hyprland(&unix_request(&self.socket, b"j/activewindow", false)?)
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
    use x11rb::rust_connection::RustConnection;

    use super::{FocusInfo, FocusProvider};

    pub struct X11Focus {
        conn: RustConnection,
        root: u32,
        active: u32,
    }

    impl X11Focus {
        pub fn connect() -> Option<Self> {
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots[screen].root;
            let active = conn
                .intern_atom(false, b"_NET_ACTIVE_WINDOW")
                .ok()?
                .reply()
                .ok()?
                .atom;
            Some(Self { conn, root, active })
        }
    }

    impl FocusProvider for X11Focus {
        fn name(&self) -> &'static str {
            "x11"
        }
        fn focused(&mut self) -> Option<FocusInfo> {
            let reply = self
                .conn
                .get_property(false, self.root, self.active, AtomEnum::WINDOW, 0, 1)
                .ok()?
                .reply()
                .ok()?;
            let window = reply.value32()?.next()?;
            if window == 0 {
                return None;
            }
            // WM_CLASS is "instance\0class\0". Use the class.
            let class = self
                .conn
                .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| {
                    r.value
                        .split(|b| *b == 0)
                        .filter(|p| !p.is_empty())
                        .nth(1)
                        .map(|p| String::from_utf8_lossy(p).into_owned())
                });
            Some(FocusInfo {
                id: window.to_string(),
                app_id: class,
            })
        }
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    use super::{FocusInfo, FocusProvider};

    pub struct WindowsFocus;

    impl FocusProvider for WindowsFocus {
        fn name(&self) -> &'static str {
            "windows"
        }
        fn focused(&mut self) -> Option<FocusInfo> {
            // SAFETY: GetForegroundWindow has no preconditions.
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.is_null() {
                return None;
            }
            Some(FocusInfo {
                id: format!("{:p}", hwnd),
                app_id: None,
            })
        }
    }
}

/// Picks the best focus provider for the running session.
pub fn detect() -> Box<dyn FocusProvider> {
    #[cfg(windows)]
    {
        Box::new(win::WindowsFocus)
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            if let Some(socket) = std::env::var_os("NIRI_SOCKET") {
                return Box::new(NiriFocus {
                    socket: socket.into(),
                });
            }
            if let (Some(sig), Some(run)) = (
                std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE"),
                std::env::var_os("XDG_RUNTIME_DIR"),
            ) {
                let socket = PathBuf::from(run)
                    .join("hypr")
                    .join(sig)
                    .join(".socket.sock");
                return Box::new(HyprlandFocus { socket });
            }
            return Box::new(NoFocus);
        }
        match x11::X11Focus::connect() {
            Some(f) => Box::new(f),
            None => Box::new(NoFocus),
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Box::new(NoFocus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_niri_replies() {
        let r = r#"{"Ok":{"FocusedWindow":{"id":9,"title":"x","app_id":"kitty","pid":1}}}"#;
        assert_eq!(
            parse_niri(r),
            Some(FocusInfo {
                id: "9".into(),
                app_id: Some("kitty".into())
            })
        );
        assert_eq!(parse_niri(r#"{"Ok":{"FocusedWindow":null}}"#), None);
        assert_eq!(parse_niri(r#"{"Err":"nope"}"#), None);
        assert_eq!(parse_niri("garbage"), None);
    }

    #[test]
    fn parses_hyprland_replies() {
        let r = r#"{"address":"0x55d1","class":"firefox","title":"t"}"#;
        assert_eq!(
            parse_hyprland(r),
            Some(FocusInfo {
                id: "0x55d1".into(),
                app_id: Some("firefox".into())
            })
        );
        assert_eq!(parse_hyprland("{}"), None);
    }
}
