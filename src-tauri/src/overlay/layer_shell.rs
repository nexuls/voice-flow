//! wlr layer-shell placement for the overlay on Wayland.
//!
//! Turns the overlay into a layer surface on the `Overlay` layer, anchored to
//! the bottom edge (horizontally centered), with no keyboard interactivity and
//! no exclusive zone. The compositor puts it on the focused output.

use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use tauri::{Runtime, WebviewWindow};

use super::{BOTTOM_MARGIN, HEIGHT, WIDTH};

/// Returns true if the window was turned into a layer surface.
pub fn try_init<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() || !gtk_layer_shell::is_supported() {
        return false;
    }
    let Ok(gtk_window) = window.gtk_window() else {
        return false;
    };
    // A layer surface must be configured before the window is mapped.
    // Tauri realizes the window on creation, so unrealize it first.
    let was_realized = gtk_window.is_realized();
    if was_realized {
        gtk_window.unrealize();
    }
    tracing::debug!(was_realized, "configuring overlay layer surface");
    gtk_window.init_layer_shell();
    gtk_window.set_namespace("voiceflow-overlay");
    gtk_window.set_layer(Layer::Overlay);
    gtk_window.set_keyboard_mode(KeyboardMode::None);
    gtk_window.set_exclusive_zone(-1);
    gtk_window.set_anchor(Edge::Bottom, true);
    gtk_window.set_layer_shell_margin(Edge::Bottom, BOTTOM_MARGIN as i32);
    // Layer surfaces take their size from the widget, not from the Tauri
    // builder, so request it explicitly (logical pixels).
    gtk_window.set_size_request(WIDTH as i32, HEIGHT as i32);
    true
}

/// Moves the layer surface to the top or bottom edge. Must run on the GTK
/// main thread.
pub fn set_edge<R: Runtime>(window: &WebviewWindow<R>, top: bool) {
    let Ok(gtk_window) = window.gtk_window() else {
        return;
    };
    gtk_window.set_anchor(Edge::Bottom, !top);
    gtk_window.set_anchor(Edge::Top, top);
    let margin = BOTTOM_MARGIN as i32;
    gtk_window.set_layer_shell_margin(Edge::Bottom, if top { 0 } else { margin });
    gtk_window.set_layer_shell_margin(Edge::Top, if top { margin } else { 0 });
}
