# GTK4 Feasibility Contract (Agent C)

Research-only. Host: Fedora 44. Binding line: **gtk4-rs 0.11** against system **GTK 4.22.1** / **libadwaita 1.9.0**. Versions and blockers: [`environment-gtk.md`](./environment-gtk.md).

APIs below are confirmed for **gtk4 0.11.5**, **libadwaita 0.9.2**, **glib/pango 0.22.x** via docs.rs / gtk-rs.org stable 0.11. Do not invent names; when unsure, re-check the cited docs.

---

## Verdict

**Feasible** for a single custom `gtk::Widget` subclass that paints a full `TerminalSnapshot` through `gtk::Snapshot` / GSK — not one widget per cell. Runtime libs are present; **build is blocked** until `-devel` packages and `rustc`/`cargo` ≥ 1.92 are installed (see environment doc).

---

## Chosen bindings

| Role | Crate | Pin | Feature flags |
| --- | --- | --- | --- |
| GTK | `gtk4` as `gtk` | 0.11.5 | `v4_22` (or `gnome_50`) |
| Adwaita | `libadwaita` as `adw` | 0.9.2 | `v1_9`, `gtk_v4_22` |
| GLib / GIO | `glib`, `gio` | 0.22.x | match gtk line |
| Pango | `pango` | 0.22.x | transitive (`v1_46` via gtk4) |

**Why 0.11, not 0.10:** system GTK is 4.22; crate feature `v4_22` exists on 0.11.x. `libadwaita` 0.9.2 requires `gtk4 ^0.11`. MSRV **1.92**.

### Binding limitations

- Default feature set is GTK **4.0** APIs only; enable `v4_22` (or `gnome_50`) to match the installed library and unlock 4.12+ helpers such as `CssProvider::load_from_string` (gated `v4_12`+).
- `WidgetImpl::snapshot` signature in 0.11 is `fn snapshot(&self, snapshot: &Snapshot)` — access the public widget via `self.obj()`, not a separate `widget:` parameter ([docs](https://docs.rs/gtk4/0.11.5/gtk4/subclass/widget/trait.WidgetImpl.html)).
- `EventControllerKey::forward` is marked deprecated since GTK 4.24 in bindings; unused on 4.22, but avoid new reliance on it.
- Without `pkg-config` (devel), `gtk4-sys` / `system-deps` **cannot** compile.

### Simplest supported alternative

1. **Bindings:** stay on gtk4 **0.11.5** + libadwaita **0.9.2**. If Adwaita is dropped, use `gtk::Application` + `gtk::ApplicationWindow` on the same gtk4 crate (still need gtk4-devel).
2. **Card chrome:** prefer CSS (`CssProvider` + `gtk::style_context_add_provider_for_display`) for the outer card; keep **Snapshot** for terminal cells. Do not use Unicode box glyphs for the card.
3. **Do not** fall back to one `Label`/`DrawingArea` per cell. `DrawingArea::set_draw_func` is Cairo, not Snapshot/GSK — only as a last resort for experiments, not the POC renderer.

---

## Supported scale factors

| Label | Factor | How to produce (manual) |
| --- | --- | --- |
| 100% | 1.0 | Default / `GDK_SCALE=1` |
| 125% | 1.25 | Fractional Wayland scale (e.g. Niri output scale 1.25) |
| 200% | 2.0 | Integer 2× / `GDK_SCALE=2` |

### Reading scale

| API | Type | Use |
| --- | --- | --- |
| `gtk::prelude::WidgetExt::scale_factor(&self) -> i32` | integer ceiling-style buffer scale | coarse; also `connect_scale_factor_notify` |
| `gdk::prelude::SurfaceExt::scale(&self) -> f64` | fractional device scale | **prefer for snapping** at 125% ([gdk4 Surface](https://gtk-rs.org/gtk4-rs/stable/0.11/docs/gdk4/struct.Surface.html)) |
| `gdk::prelude::SurfaceExt::scale_factor(&self) -> i32` | integer | “next larger integer” vs `scale` |

Resolve surface: `widget.native().and_then(|n| n.surface())` (via `gtk::prelude::WidgetExt` / `NativeExt`).

### Device-pixel snap

```text
snap(logical, scale) = round(logical * scale) / scale
```

Use `scale` from `SurfaceExt::scale()` (f64). Apply to cell origins, borders, and 1-physical-pixel rules before `Snapshot` appends. Recompute on `notify::scale-factor` and surface scale changes; queue redraw with `WidgetExt::queue_draw`.

---

## Concrete APIs by concern

### 1. Single custom snapshot widget (not per-cell)

| Item | Name | Docs |
| --- | --- | --- |
| Subclass macros | `glib::object_subclass`, `glib::wrapper!` | gtk-rs subclassing |
| Parent | `type ParentType = gtk::Widget` | |
| Impl traits | `ObjectImpl`, `WidgetImpl` | |
| Paint vfunc | `WidgetImpl::snapshot(&self, snapshot: &gtk::Snapshot)` | [WidgetImpl](https://docs.rs/gtk4/0.11.5/gtk4/subclass/widget/trait.WidgetImpl.html) |
| Size | `WidgetImpl::size_allocate`, `WidgetImpl::measure` | same |
| Redraw | `WidgetExt::queue_draw` | [WidgetExt](https://docs.rs/gtk4/0.11.5/gtk4/prelude/trait.WidgetExt.html) |
| Focus | `set_focusable(true)`, `set_can_focus(true)`, `grab_focus`, `has_focus` | WidgetExt |
| Pango helpers | `create_pango_context`, `create_pango_layout`, `pango_context` | WidgetExt |

Pattern: one `TerminalView(ObjectSubclass<imp::TerminalView>) @extends gtk::Widget`; store snapshot in `imp`; on update replace model + `queue_draw()`. **Zero** child widgets for cells.

### 2. Pango font metrics and monospace fallback

| Step | API |
| --- | --- |
| Describe | `pango::FontDescription::new()`, `set_family`, `set_size` (Pango units) or `from_string` |
| Context | `WidgetExt::create_pango_context()` or `pango_context()` |
| Resolve | `pango::Context::load_font(&desc) -> Option<Font>`; `list_families` if needed |
| Metrics | `Context::metrics(Some(&desc), language) -> FontMetrics` |
| Metrics fields | `FontMetrics::approximate_char_width`, `approximate_digit_width`, `ascent`, `descent`, `height` (feature `v1_44`) ([source](https://docs.rs/pango/0.22.9/src/pango/auto/font_metrics.rs.html)) |
| To pixels | divide by `pango::SCALE` (`pub const SCALE: i32`) |
| Layout text | `WidgetExt::create_pango_layout(Some(text))`, then `SnapshotExt::append_layout` |

**Fallback policy (contract):**

1. `set_family("JetBrains Mono")` (and optional size).
2. If `load_font` fails or advances for grid glyphs are unstable across samples, switch to `set_family("monospace")` and re-measure.
3. Reject fonts whose reported advances are not stable for the supported grid glyph set (renderer responsibility).

Host note: exact family `JetBrains Mono` is **not** installed; `monospace` → Noto Sans Mono. Nerd Font variants exist but are not the primary name.

### 3. Background / text / rounded clip / border / shadow (GSK + CSS)

**Terminal / card paint via Snapshot** ([SnapshotExt](https://docs.rs/gtk4/latest/gtk4/struct.Snapshot.html)):

| Effect | Method |
| --- | --- |
| Solid fill | `append_color(&gdk::RGBA, &graphene::Rect)` |
| Text run | `append_layout(&pango::Layout, &gdk::RGBA)` |
| Rounded clip | `push_rounded_clip(&gsk::RoundedRect)` … draw … `pop()` |
| Border | `append_border(&gsk::RoundedRect, &[f32; 4] widths, &[gdk::RGBA; 4] colors)` |
| Drop shadow | `append_outset_shadow(&outline, &color, dx, dy, spread, blur_radius)` |
| Inset shadow | `append_inset_shadow(...)` (if needed) |
| Axis-aligned clip | `push_clip(&graphene::Rect)` / `pop()` |

**Rounded rect construction** ([gsk4::RoundedRect](https://gtk-rs.org/gtk4-rs/stable/0.11/docs/gsk4/struct.RoundedRect.html)):

- `gsk::RoundedRect::from_rect(bounds, radius)` — equal corners (8px card).
- `gsk::RoundedRect::new(bounds, top_left, top_right, bottom_right, bottom_left)` — per-corner `graphene::Size`.

**Outer card via CSS (shell, not Unicode):**

- `gtk::CssProvider::new()`
- `CssProvider::load_from_string(css)` (needs `v4_12`+)
- `gtk::style_context_add_provider_for_display(display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION)`
- `WidgetExt::add_css_class("terminal-card")` (and similar)

POC theme values (from `dev/05-native-shell.md`): bg `#111318`, card `#171a20`, ~8px radius, low-opacity border, shadow ~4px / 18px blur, 8–12px padding — express in CSS and/or Snapshot; never box-drawing characters for the frame.

### 4. Keyboard controllers and focus

| Item | API |
| --- | --- |
| Key controller | `gtk::EventControllerKey::new()` |
| Attach | `WidgetExt::add_controller(controller)` |
| Press | `connect_key_pressed` → `glib::Propagation` ([EventControllerKey](https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/struct.EventControllerKey.html)) |
| Release | `connect_key_released` |
| IM | `im_context` / `set_im_context` |
| Focus flags | `set_focusable(true)`, `set_can_focus(true)`, `set_focus_on_click(true)` |
| Focus query | `grab_focus()`, `has_focus()`, `has_visible_focus()` |

Use the key controller for terminal input once interactive; keep the enhance toggle on an **application action** (below), not only a local key handler.

### 5. Allocation changes and debounced resize

| Item | API |
| --- | --- |
| Allocate vfunc | `WidgetImpl::size_allocate(&self, width: i32, height: i32, baseline: i32)` |
| Current size | `WidgetExt::width`, `height`, `allocated_width`, `allocated_height` |
| Relayout hint | `queue_resize`, `queue_allocate` |
| Debounce timer | `glib::timeout_add_local(Duration, \|\| ControlFlow)` and/or `timeout_add_local_once` / `timeout_add_once` ([glib source](https://docs.rs/glib/0.22.9/src/glib/source.rs.html)); cancel prior `SourceId` with `SourceId::remove` |
| Scale notify | `WidgetExt::connect_scale_factor_notify` |

Recommended: on `size_allocate`, store pending logical size; restart a short timeout (~50–100 ms); when it fires, recompute grid capacity from cell metrics + scale, notify resize owner, `queue_draw`. Do not spam Herdr resize on every pixel.

### 6. Application action for Ctrl+Shift+E

| Item | API |
| --- | --- |
| App | `adw::Application::builder()` (or gtk `Application`) |
| Window | `adw::ApplicationWindow::builder()` ([0.9.2](https://docs.rs/libadwaita/0.9.2/libadwaita/struct.ApplicationWindow.html)) |
| Stateful action | `gio::SimpleAction::new_stateful("toggle-enhance", None, &false.to_variant())` |
| Register | `gio::prelude::ActionMapExt::add_action` / `add_action_entries` |
| Activate | `SimpleAction::connect_activate` / `connect_change_state` |
| Accel (simplest) | `gtk::prelude::GtkApplicationExt::set_accels_for_action(app, "app.toggle-enhance", &["<Control><Shift>e"])` |

**Explicit Shortcut path** (also valid):

- `gtk::KeyvalTrigger::new(gdk::Key::e, CONTROL_MASK \| SHIFT_MASK)` ([KeyvalTrigger](https://gtk-rs.org/gtk4-rs/stable/0.11/docs/gtk4/struct.KeyvalTrigger.html))
- `gtk::NamedAction::new("app.toggle-enhance")`
- `gtk::Shortcut::new(Some(trigger), Some(action))`
- `gtk::ShortcutController::new()`, `add_shortcut`, `set_scope(ShortcutScope::Global)` or managed; `WidgetExt::add_controller`

Toggle must flip renderer enhancement state immediately without reconnecting (shell contract).

---

## Renderer owner cheat-sheet

```text
WIDGET
  glib::wrapper! TerminalView @extends gtk::Widget
  WidgetImpl::snapshot(snapshot)
  WidgetImpl::size_allocate → debounce → grid capacity
  set_focusable(true); add_controller(EventControllerKey)
  queue_draw() on snapshot replace

FONT
  FontDescription::set_family("JetBrains Mono") else "monospace"
  Context::metrics → FontMetrics::{approximate_char_width, height, ascent, descent}
  pixels = pango_units as f64 / pango::SCALE as f64
  reject unstable advances

DRAW (cells)
  append_color(bg, cell_rect)
  append_layout(run_layout, fg)
  push_rounded_clip / push_clip → content → pop
  snap(x, SurfaceExt::scale())

DRAW (card chrome — shell)
  CSS: CssProvider::load_from_string + style_context_add_provider_for_display
  and/or Snapshot: append_outset_shadow + append_border + from_rect(..., 8.0)
  NEVER Unicode box glyphs for the card

SCALE
  test 100% / 125% / 200%
  prefer SurfaceExt::scale() f64; WidgetExt::scale_factor() i32 as secondary

CTRL+SHIFT+E
  SimpleAction::new_stateful("toggle-enhance", ...)
  app.add_action(...); set_accels_for_action(..., &["<Control><Shift>e"])
```

---

## Blockers

| Blocker | Evidence | Unblock |
| --- | --- | --- |
| No `-devel` / GIR | `rpm -q gtk4-devel` … not installed; `pkg-config --modversion gtk4` fails | `dnf install gtk4-devel libadwaita-devel pango-devel glib2-devel cairo-devel cairo-gobject-devel graphene-devel gobject-introspection-devel pkgconf-pkg-config` |
| No Rust | `rustc`/`cargo` not on PATH; rpm packages not installed | Install Fedora `rust`/`cargo` ≥ 1.92 or rustup |

Until both are cleared, crates cannot link against GTK and the renderer cannot be compiled. Runtime GTK/Adwaita/Pango/GLib versions are otherwise sufficient for the APIs above.
