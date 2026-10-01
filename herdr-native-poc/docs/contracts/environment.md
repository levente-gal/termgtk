# environment.md

Captured 2026-09-28. Details live in the sibling contract files.

| Component | Version |
| --- | --- |
| Herdr | 0.8.0, protocol 19, server was running |
| Herdr schema SHA-256 | `88ff414aa996e390c2db05a37b95d28dbe4e81b98329f6ed7f7a2cc5c6ebf51a` |
| Ghostty | `b40acce58dcf77df52231c3798ea58e924647c89` (tip, MIT). Stable v1.3.1 has no terminal C API |
| Zig | 0.16.0 required. Not on `PATH`; official tarball built `libghostty-vt` in `/tmp` |
| Rust | rustc/cargo 1.98.1 via rustup (`~/.cargo/bin`) |
| GTK | runtime 4.22.1, libadwaita 1.9.0, pango 1.57.1, glib 2.88.0 |
| Rust bindings | gtk4 0.11.5 (`v4_22`), libadwaita 0.9.2 (`v1_9`, `gtk_v4_22`), glib/gio 0.22 |
| GTK devel | **not installed** (`pkg-config gtk4` fails; `sudo` needs a password) |

See `stream-contract.md`, `ghostty.md`, and `gtk.md`.
