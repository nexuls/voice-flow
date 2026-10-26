#!/usr/bin/env python3
"""Development aid: a GTK text window that mirrors its contents to a file.

    python3 scripts/dev/paste_target.py out.txt 20

Used to verify automatic paste (see TESTING.md)."""
import sys, gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, GLib
out, secs = sys.argv[1], float(sys.argv[2])
win = Gtk.Window(title="VoiceFlow paste target")
win.set_default_size(500, 200)
view = Gtk.TextView()
win.add(view)
def dump():
    b = view.get_buffer()
    with open(out, "w") as f:
        f.write(b.get_text(b.get_start_iter(), b.get_end_iter(), True))
    return True
GLib.timeout_add(200, dump)
GLib.timeout_add(int(secs * 1000), lambda: (dump(), Gtk.main_quit()))
win.connect("destroy", Gtk.main_quit)
win.show_all()
view.grab_focus()
Gtk.main()
