//! Floating dictation overlay window.
//!
//! The window is created hidden at startup, so showing it only needs a
//! position update and a `show()`. It is transparent, undecorated,
//! non-focusable, skipped by the taskbar, and ignores the cursor unless it
//! needs to offer an action.
//!
//! Placement:
//! - Windows, macOS, X11: bottom-center of the work area of the monitor under
//!   the cursor, recomputed on every show to follow monitor and scale changes.
//! - Wayland: clients cannot position themselves. With the wlr layer-shell
//!   protocol (wlroots, niri, KDE, Hyprland) the overlay becomes a layer
//!   surface anchored to the bottom edge. Without it (GNOME), the compositor
//!   places a normal window.

#[cfg(all(target_os = "linux", feature = "layer-shell"))]
mod layer_shell;

use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::OverlayPosition;

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const LABEL: &str = "overlay";
/// Logical size of the overlay window. The pill is centered inside it, and the
/// spare transparent area leaves room for the shadow and wider error text.
pub const WIDTH: f64 = 360.0;
pub const HEIGHT: f64 = 76.0;
/// Logical distance between the overlay and the bottom of the work area.
pub const BOTTOM_MARGIN: f64 = 40.0;

/// Native overlay operations requested by the workflow.
pub trait OverlayController: Send + Sync {
    fn show(&self);
    fn hide(&self);
    /// Lets the overlay receive clicks, for example for a "Copy text" button.
    fn set_interactive(&self, interactive: bool);
    /// Applies user preferences. A disabled overlay is never shown.
    fn configure(&self, position: OverlayPosition, enabled: bool);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Top-left physical position that centers `size` horizontally in `area`,
/// `margin` pixels above its bottom edge. Clamped so the window stays inside
/// the area even when the area is smaller than the window.
pub fn bottom_center(area: Rect, size: PhysicalSize<u32>, margin: u32) -> PhysicalPosition<i32> {
    place(area, size, margin, OverlayPosition::Bottom)
}

/// Horizontally centered position at the top or bottom edge of `area`.
pub fn place(
    area: Rect,
    size: PhysicalSize<u32>,
    margin: u32,
    edge: OverlayPosition,
) -> PhysicalPosition<i32> {
    let free_x = area.width.saturating_sub(size.width) as i32;
    let x = area.x + free_x / 2;
    let max_y = area.y + area.height.saturating_sub(size.height) as i32;
    let y = match edge {
        OverlayPosition::Bottom => area.y + area.height as i32 - size.height as i32 - margin as i32,
        OverlayPosition::Top => area.y + margin as i32,
    };
    PhysicalPosition::new(x, y.clamp(area.y, max_y))
}

pub fn physical_size(scale: f64) -> PhysicalSize<u32> {
    PhysicalSize::new(
        (WIDTH * scale).round() as u32,
        (HEIGHT * scale).round() as u32,
    )
}

pub struct TauriOverlay<R: Runtime> {
    app: AppHandle<R>,
    layer_shell: bool,
    visible: AtomicBool,
    interactive: AtomicBool,
    enabled: AtomicBool,
    top: AtomicBool,
}

impl<R: Runtime> TauriOverlay<R> {
    /// Creates the hidden overlay window.
    pub fn create(app: &AppHandle<R>) -> tauri::Result<Self> {
        let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
            .title("VoiceFlow Overlay")
            .inner_size(WIDTH, HEIGHT)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .visible(false)
            .build()?;
        // Input shape changes need a realized window (tao unwraps the
        // GdkWindow), so click-through is applied on show, not here.

        #[cfg(all(target_os = "linux", feature = "layer-shell"))]
        let layer_shell = layer_shell::try_init(&window);
        #[cfg(not(all(target_os = "linux", feature = "layer-shell")))]
        let layer_shell = {
            let _ = &window;
            false
        };
        tracing::info!(layer_shell, "overlay window created");

        Ok(Self {
            app: app.clone(),
            layer_shell,
            visible: AtomicBool::new(false),
            interactive: AtomicBool::new(false),
            enabled: AtomicBool::new(true),
            top: AtomicBool::new(false),
        })
    }

    fn window(&self) -> Option<WebviewWindow<R>> {
        self.app.get_webview_window(LABEL)
    }

    /// Applies click-through. Only valid while the window is shown.
    fn apply_input_shape(&self, window: &WebviewWindow<R>) {
        let ignore = !self.interactive.load(Ordering::SeqCst);
        if let Err(e) = window.set_ignore_cursor_events(ignore) {
            tracing::warn!(error = %e, "overlay input shape update failed");
        }
    }

    fn position(&self, window: &WebviewWindow<R>) -> tauri::Result<()> {
        let monitor = match self.app.cursor_position() {
            Ok(p) => self.app.monitor_from_point(p.x, p.y)?,
            Err(_) => None,
        }
        .or(self.app.primary_monitor()?);
        let Some(monitor) = monitor else {
            return Ok(());
        };
        let scale = monitor.scale_factor();
        let size = physical_size(scale);
        let wa = monitor.work_area();
        let area = Rect {
            x: wa.position.x,
            y: wa.position.y,
            width: wa.size.width,
            height: wa.size.height,
        };
        let edge = if self.top.load(Ordering::SeqCst) {
            OverlayPosition::Top
        } else {
            OverlayPosition::Bottom
        };
        window.set_size(size)?;
        window.set_position(place(
            area,
            size,
            (BOTTOM_MARGIN * scale).round() as u32,
            edge,
        ))?;
        Ok(())
    }
}

impl<R: Runtime> OverlayController for TauriOverlay<R> {
    fn show(&self) {
        if !self.enabled.load(Ordering::SeqCst) {
            return;
        }
        let Some(window) = self.window() else { return };
        if !self.layer_shell
            && let Err(e) = self.position(&window)
        {
            tracing::warn!(error = %e, "overlay positioning failed");
        }
        if let Err(e) = window.show() {
            tracing::warn!(error = %e, "overlay show failed");
            return;
        }
        self.visible.store(true, Ordering::SeqCst);
        // Window requests run in order on the main thread, so this runs
        // after the window is mapped.
        self.apply_input_shape(&window);
    }

    fn hide(&self) {
        self.visible.store(false, Ordering::SeqCst);
        self.interactive.store(false, Ordering::SeqCst);
        if let Some(window) = self.window()
            && let Err(e) = window.hide()
        {
            tracing::warn!(error = %e, "overlay hide failed");
        }
    }

    fn set_interactive(&self, interactive: bool) {
        self.interactive.store(interactive, Ordering::SeqCst);
        if self.visible.load(Ordering::SeqCst)
            && let Some(window) = self.window()
        {
            self.apply_input_shape(&window);
        }
    }

    fn configure(&self, position: OverlayPosition, enabled: bool) {
        let top = position == OverlayPosition::Top;
        self.enabled.store(enabled, Ordering::SeqCst);
        let moved = self.top.swap(top, Ordering::SeqCst) != top;
        if !enabled {
            self.hide();
        }
        #[cfg(all(target_os = "linux", feature = "layer-shell"))]
        if moved
            && self.layer_shell
            && let Some(window) = self.window()
        {
            let _ = self
                .app
                .run_on_main_thread(move || layer_shell::set_edge(&window, top));
        }
        let _ = moved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
    };

    #[test]
    fn centers_horizontally_above_bottom_margin() {
        let p = bottom_center(AREA, PhysicalSize::new(360, 60), 48);
        assert_eq!(p, PhysicalPosition::new(780, 1040 - 60 - 48));
    }

    #[test]
    fn respects_secondary_monitor_offsets() {
        // Monitor to the left of the primary, with a negative origin.
        let left = Rect {
            x: -2560,
            y: -200,
            width: 2560,
            height: 1400,
        };
        let p = bottom_center(left, PhysicalSize::new(720, 120), 96);
        assert_eq!(
            p,
            PhysicalPosition::new(-2560 + 920, -200 + 1400 - 120 - 96)
        );
    }

    #[test]
    fn stays_inside_tiny_area() {
        let tiny = Rect {
            x: 10,
            y: 10,
            width: 200,
            height: 50,
        };
        let p = bottom_center(tiny, PhysicalSize::new(360, 60), 48);
        assert_eq!(p, PhysicalPosition::new(10, 10));
    }

    #[test]
    fn top_placement_uses_margin_from_top() {
        let p = place(AREA, PhysicalSize::new(360, 76), 40, OverlayPosition::Top);
        assert_eq!(p, PhysicalPosition::new(780, 40));
    }

    #[test]
    fn scales_size_for_hidpi() {
        assert_eq!(physical_size(1.0), PhysicalSize::new(360, 76));
        assert_eq!(physical_size(2.0), PhysicalSize::new(720, 152));
        assert_eq!(physical_size(1.25), PhysicalSize::new(450, 95));
    }
}
