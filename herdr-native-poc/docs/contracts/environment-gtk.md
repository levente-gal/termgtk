# Environment — GTK / Rust (Agent C)

Host: Fedora 44 (Forty Four), x86_64. Captured 2026-09-28.

## Toolchain

| Tool | Status | Version |
| --- | --- | --- |
| `rustc` | installed via rustup | `1.98.1 (48a229cea 2026-09-01)` at `~/.cargo/bin/rustc` |
| `cargo` | installed via rustup | `1.98.1 (797e8a9bc 2026-08-05)` at `~/.cargo/bin/cargo` |
| Fedora `rust` / `cargo` packages | not installed | `sudo dnf` needs a password in this session |

gtk4-rs **0.11** line MSRV is **1.92** ([crates.io/crates/gtk4](https://crates.io/crates/gtk4)). Install at least that before building.

## System libraries (`rpm -q`)

| Package | NEVR |
| --- | --- |
| `gtk4` | `gtk4-4.22.1-2.fc44` |
| `libadwaita` | `libadwaita-1.9.0-1.fc44` |
| `pango` | `pango-1.57.1-1.fc44` |
| `glib2` | `glib2-2.88.0-1.fc44` |

Shared objects present: `/usr/lib64/libgtk-4.so.1.2200.1`, `/usr/lib64/libadwaita-1.so.0`.

## pkg-config

All of these **fail** (no `.pc` files — devel packages missing):

```text
pkg-config --modversion gtk4          → Package 'gtk4' not found
pkg-config --modversion libadwaita-1  → Package 'libadwaita-1' not found
pkg-config --modversion pango         → Package 'pango' not found
pkg-config --modversion glib-2.0      → Package 'glib-2.0' not found
```

Expected modversions once devel is installed (must match runtime): **gtk4 4.22.1**, **libadwaita-1 1.9.0**, **pango 1.57.1**, **glib-2.0 2.88.0**.

## Chosen crates.io crates (match system GTK 4.22 / Adwaita 1.9 / GLib 2.88)

| Crate | Version | Features |
| --- | --- | --- |
| `gtk4` (import as `gtk`) | **0.11.5** | `v4_22` or `gnome_50` (`gnome_50` → `v4_22` + `gio/v2_88`) |
| `libadwaita` (import as `adw`) | **0.9.2** | `v1_9`, `gtk_v4_22` |
| `glib` | **0.22** (^0.22.0; latest 0.22.10) | default; enable `v2_88` via gtk `gnome_50` if needed |
| `gio` | **0.22** (^0.22.0) | pulled by gtk4 / libadwaita |
| `pango` | **0.22** (^0.22.0; gtk4 enables `v1_46`) | transitive via gtk4 |
| `gdk4` / `gsk4` / `graphene-rs` | **0.11** / **0.11** / **0.22** | transitive via gtk4 |

`libadwaita` 0.9.2 depends on `gtk4 ^0.11`, `glib ^0.22`, `gio ^0.22`, `pango ^0.22` ([crates.io deps](https://crates.io/api/v1/crates/libadwaita/0.9.2/dependencies)).

Suggested `Cargo.toml` fragment:

```toml
[dependencies]
gtk = { package = "gtk4", version = "0.11.5", features = ["v4_22"] }
adw = { package = "libadwaita", version = "0.9.2", features = ["v1_9", "gtk_v4_22"] }
glib = "0.22"
gio = "0.22"
```

## Fonts (host observation)

| Family | Installed? |
| --- | --- |
| `JetBrains Mono` (exact) | **No** (`fc-match 'JetBrains Mono'` → Noto Sans) |
| `JetBrainsMono Nerd Font` | Yes (user fonts) |
| Default `monospace` | Resolves to **Noto Sans Mono** (`fc-match monospace`) |
| `Adwaita Mono` | Yes (`adwaita-mono-fonts`) |

Contract resolution remains: try **JetBrains Mono**, else **monospace**. Do not treat Nerd Font as the required primary name.

## Blockers (must clear before compile)

1. **Missing devel / GIR packages** — `gtk4-devel`, `libadwaita-devel`, `pango-devel`, `glib2-devel`, `cairo-devel`, `cairo-gobject-devel`, `graphene-devel`, `gobject-introspection-devel`, `pkgconf-pkg-config` (none installed; no `/usr/include/gtk-4.0`, no `Gtk-4.0.gir`).
2. **Rust toolchain** — cleared. rustup stable `1.98.1` is the default (`~/.cargo/bin` must be on `PATH`). GTK devel packages are still missing; `sudo -n dnf` cannot install them because this session has no sudo password.

Suggested install:

```bash
sudo dnf install gtk4-devel libadwaita-devel pango-devel glib2-devel \
  cairo-devel cairo-gobject-devel graphene-devel gobject-introspection-devel \
  pkgconf-pkg-config rust cargo
```

## Related

Full API cheat-sheet and scale-factor notes: [`gtk.md`](./gtk.md).
