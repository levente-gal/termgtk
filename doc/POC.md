# Herdr Native Renderer — First POC

## Goal

Build a Linux-only proof of concept that renders an existing Herdr-managed terminal inside a custom native GTK4 application, without modifying Herdr or the application running inside the terminal.

The POC must prove these ideas:

1. Herdr can remain the persistent process/session backend.
2. We can consume a live Herdr terminal stream ourselves.
3. We can parse the ANSI/VT stream into structured terminal cells using `libghostty-vt`.
4. We can render those cells ourselves instead of using a normal terminal emulator.
5. We can surround the terminal with genuinely native graphical UI:
   - rounded corners
   - 1px borders
   - real drop shadows
   - native spacing/padding
6. We can recognize at least some box-drawing characters inside an arbitrary TUI and replace them with graphical separators/borders.
7. The terminal remains interactive.

The POC is specifically intended to answer:

> Can an unmodified TUI be progressively rendered as something that looks like a native application?

Do not attempt to build a complete terminal emulator or full Herdr replacement.

---

# Target environment

Primary development target:

- Linux
- Fedora
- Wayland
- Niri-compatible
- Rust
- GTK4
- libadwaita where useful
- Herdr already installed
- Zig available for building `libghostty-vt`

Use Rust for the application.

Use GTK4 for the window/UI.

Use `libghostty-vt` for VT parsing and terminal state.

Do NOT use:

- Electron
- Tauri
- xterm.js
- a webview
- Ghostty as an external terminal window
- terminal screenshots
- framebuffer post-processing
- Herdr source modifications

---

# Important architectural decision

For this POC, DO NOT talk directly to Herdr's private internals.

Use Herdr's documented public CLI/API surfaces.

The architecture should be:

```text
                    Herdr server
                         │
                  persistent pane
                         │
           herdr terminal session control
                         │
                  NDJSON frames
                         │
                base64 ANSI bytes
                         │
                  libghostty-vt
                         │
                 terminal cell grid
                         │
            semantic enhancement pass
                         │
                    GTK4 renderer
                         │
             native Wayland application
```

Herdr owns:

- processes
- PTYs
- session persistence
- workspaces
- panes
- terminal lifetime

Our application owns:

- graphical layout
- typography
- terminal rendering
- shadows
- borders
- animation
- semantic enhancement

---

# POC scope

The first POC should render ONE focused Herdr pane.

Do not initially reproduce the complete Herdr workspace layout.

The native window should look approximately like:

```text
╭─────────────────────────────────────────────────────────╮
│  Herdr Native                                      ⋮    │
│                                                         │
│  alpha-api                                              │
│                                                         │
│   ╭─────────────────────────────────────────────────╮   │
│   │                                                 │   │
│   │             terminal contents                   │   │
│   │                                                 │   │
│   ╰─────────────────────────────────────────────────╯   │
│             subtle real drop shadow                     │
│                                                         │
╰─────────────────────────────────────────────────────────╯
```

The terminal panel itself must be a real graphical panel.

It must NOT be drawn using:

```text
┌────┐
│    │
└────┘
```

The border, corners and shadow must come from GTK/GSK.

---

# Phase 0 — project bootstrap

Create:

```text
herdr-native-poc/
├── Cargo.toml
├── build.rs
├── README.md
├── src/
│   ├── main.rs
│   │
│   ├── app.rs
│   │
│   ├── herdr/
│   │   ├── mod.rs
│   │   ├── snapshot.rs
│   │   └── terminal_stream.rs
│   │
│   ├── terminal/
│   │   ├── mod.rs
│   │   ├── ghostty.rs
│   │   └── model.rs
│   │
│   ├── render/
│   │   ├── mod.rs
│   │   ├── terminal_view.rs
│   │   ├── cell_renderer.rs
│   │   └── box_detector.rs
│   │
│   └── ui/
│       ├── mod.rs
│       └── theme.rs
│
└── vendor/
    └── ghostty/
```

Suggested Rust dependencies:

```text
gtk4
libadwaita
glib
gio

serde
serde_json
base64

anyhow
thiserror

tracing
tracing-subscriber
```

Use additional dependencies only when justified.

---

# Phase 1 — verify the Herdr integration independently

Before implementing GTK rendering, prove that the Herdr side works.

Run:

```bash
herdr status server
```

If no server is running, report a clear error.

Do NOT automatically kill or replace an existing Herdr server.

Then run:

```bash
herdr api snapshot
```

Parse the returned JSON.

Extract at minimum:

```text
focused_workspace_id
focused_tab_id
focused_pane_id
```

Also extract:

```text
workspace label
pane label if available
terminal title if available
```

Do not guess the exact JSON structure.

Inspect the actual current output and model only the subset required by this POC.

Herdr also exposes:

```bash
herdr api schema --json
```

Use that command whenever protocol details are uncertain.

Prefer the installed Herdr version's schema over assumptions copied from documentation.

---

# Phase 2 — consume a terminal stream

Start READ ONLY.

Spawn:

```bash
herdr terminal session observe <pane-id> --cols 120 --rows 40
```

Read stdout line by line.

Herdr emits newline-delimited JSON records.

Handle at least:

```text
terminal.frame
terminal.closed
```

For every `terminal.frame`:

1. Parse JSON.
2. Extract the base64 ANSI payload.
3. Decode base64.
4. Send the raw bytes into the terminal parser.

Do not parse ANSI manually.

Create an abstraction:

```rust
enum TerminalEvent {
    Data(Vec<u8>),
    Closed,
}
```

`HerdrTerminalStream` should expose terminal events without the GTK code knowing anything about Herdr's NDJSON representation.

---

# Phase 3 — integrate libghostty-vt

Vendor or pin a specific Ghostty revision.

Do not depend on `main` implicitly.

Record the exact Ghostty commit in the README.

Important:

`libghostty-vt` functionality is mature, but its public C API signatures may still change.

Therefore isolate ALL Ghostty-specific FFI inside:

```text
src/terminal/ghostty.rs
```

Nothing outside that module should call raw Ghostty C functions.

If useful, introduce a tiny C shim instead of exposing a large bindgen-generated API to the Rust application.

The Rust-facing interface should look approximately like:

```rust
pub struct TerminalEngine {
    ...
}

impl TerminalEngine {
    pub fn new(cols: u16, rows: u16) -> Result<Self>;

    pub fn feed(&mut self, bytes: &[u8]);

    pub fn resize(&mut self, cols: u16, rows: u16);

    pub fn snapshot(&self) -> TerminalSnapshot;
}
```

Our own neutral representation:

```rust
struct TerminalSnapshot {
    cols: usize,
    rows: usize,
    cursor: Cursor,
    cells: Vec<Cell>,
}

struct Cell {
    x: usize,
    y: usize,

    text: String,

    foreground: Color,
    background: Color,

    bold: bool,
    italic: bool,
    underline: bool,

    wide: bool,
}
```

Adapt this model to the actual current `libghostty-vt` rendering API.

Do not leak Ghostty structs into the renderer.

---

# Phase 4 — build the simplest possible GTK terminal renderer

Implement a custom GTK4 widget:

```text
TerminalView
```

It receives:

```text
TerminalSnapshot
```

and renders it using GTK's snapshot/GSK drawing APIs.

Do NOT start with OpenGL/wgpu/Vulkan.

GTK4 already has a GPU-backed scene graph.

That is sufficient for this POC.

## Typography

Start with:

```text
JetBrains Mono
```

Fallback to the user's default monospace font if unavailable.

Determine actual cell width and height from Pango font metrics.

Do not hardcode character dimensions.

For the first implementation:

- group consecutive cells with compatible styling where practical;
- render backgrounds as rectangles;
- render text with Pango;
- handle normal foreground/background colors;
- handle bold;
- preserve Unicode box characters initially.

Exact terminal fidelity is less important than proving the rendering architecture.

---

# Phase 5 — add the native container

Wrap `TerminalView` inside a visually native card.

Use approximately:

```text
outer app background:
#111318

terminal card:
#171a20

border:
rgba(255,255,255,0.08)

border radius:
8 px

shadow:
0 4px 18px rgba(0,0,0,0.30)

inner padding:
8–12 px
```

Do not obsess over exact colors.

Make these centralized theme constants.

The card MUST use graphical rendering.

Prefer GTK/GSK facilities such as:

```text
rounded clips
border render nodes
outset shadows
```

rather than approximating effects using text characters.

This is the first critical visual proof.

---

# Phase 6 — make the terminal interactive

Once read-only rendering works, replace `observe` with:

```bash
herdr terminal session control <pane-id> --takeover --cols N --rows N
```

IMPORTANT:

Using `--takeover` changes terminal controller ownership.

Make this visible in logs.

Do not silently steal control during startup without explaining it.

The process has:

```text
stdout → terminal.frame stream
stdin  → NDJSON control commands
```

Implement:

```text
terminal.input
terminal.resize
terminal.release
```

Use the exact current schema from:

```bash
herdr api schema --json
```

Do not invent JSON field names.

## Minimum keyboard support

Support:

```text
normal printable characters
Enter
Backspace
Tab
Escape

Arrow Up
Arrow Down
Arrow Left
Arrow Right

Ctrl+C
Ctrl+D
Ctrl+L
Ctrl+R
```

That should be sufficient for an initial shell and many TUIs.

If `libghostty-vt` provides a usable input encoding API at the pinned revision, prefer it over manually maintaining escape sequences.

Otherwise isolate the temporary key encoder in its own module.

Do NOT implement every possible keyboard protocol in this POC.

---

# Phase 7 — terminal resize

When the GTK terminal view changes size:

```text
pixel width
÷
cell width
=
cols
```

and:

```text
pixel height
÷
cell height
=
rows
```

Calculate:

```text
cols >= 1
rows >= 1
```

Debounce rapid GTK resize events slightly if required.

Send:

```text
terminal.resize
```

to Herdr.

Then resize `libghostty-vt` to the same dimensions.

The important invariant is:

```text
Herdr controller dimensions
==
libghostty-vt dimensions
==
our rendered grid dimensions
```

---

# Phase 8 — semantic box-drawing detection

This is the experimental part of the POC.

Create:

```text
src/render/box_detector.rs
```

Recognize at minimum:

```text
─
│

┌
┐
└
┘

╭
╮
╰
╯
```

Optionally recognize:

```text
├
┤
┬
┴
┼
```

later.

Do NOT attempt general UI understanding.

## First transformation: separators

A horizontal sequence such as:

```text
────────────────────────────
```

should NOT be rendered as glyphs.

Instead suppress those cells and draw:

```text
1 physical-pixel line
```

at the appropriate vertical position.

Use a subtle color based on the terminal foreground/background.

Optionally add:

```text
1–3 px soft dark shadow
```

below it.

Similarly:

```text
│
│
│
│
```

should be rendered as a true vertical graphical separator.

This should immediately look smoother than font-based box drawing.

---

# Phase 9 — simple rectangle detection

Then implement detection of CLOSED SIMPLE BOXES.

For example:

```text
┌──────────────────┐
│                  │
│ content          │
│                  │
└──────────────────┘
```

Recognize this as:

```text
PanelRect
```

Only handle rectangles whose perimeter consists entirely of:

```text
horizontal border
vertical border
four compatible corners
```

Ignore complicated intersections.

Pseudo algorithm:

```text
for every top-left corner:
    search right for compatible top-right corner

    verify all cells between are horizontal

    search downward for compatible bottom-left/bottom-right

    verify vertical sides

    verify bottom edge

    if valid:
        emit PanelRect
```

Terminal grids are small.

A simple implementation is fine.

Correctness and debuggability matter more than algorithmic cleverness.

---

# Phase 10 — render detected boxes natively

For every detected `PanelRect`:

1. Suppress the original border glyphs.
2. Preserve the text inside.
3. Draw a graphical rounded rectangle behind/around it.
4. Draw a real 1px border.
5. Add a subtle graphical shadow.

Example transformation:

```text
SOURCE TUI

┌──────────────────────┐
│ Settings             │
├──────────────────────┤
│ Foo                  │
│ Bar                  │
└──────────────────────┘
```

becomes conceptually:

```text
╭────────────────────────╮
│                        │
│  Settings              │
│                        │
│  Foo                   │
│  Bar                   │
│                        │
╰────────────────────────╯
        soft shadow
```

The actual corners must be graphical curves, not `╭` characters.

---

# Important rule: progressive enhancement

Never make terminal rendering depend on successful semantic detection.

The pipeline should be:

```text
cell
 │
 ├─ recognized semantic UI primitive
 │      ↓
 │   graphical rendering
 │
 └─ not recognized
        ↓
     normal terminal rendering
```

If detection fails, the terminal must still work.

This is fundamental to the project.

---

# Handling panel shadows in dense TUIs

TUIs often place boxes directly beside each other:

```text
┌──────────┬───────────┐
│ A        │ B         │
└──────────┴───────────┘
```

Do NOT blindly give every detected rectangle a huge shadow.

For this POC use this hierarchy:

### Standalone box

If there is blank/default-background space surrounding the rectangle:

```text
rounded frame
+
normal drop shadow
```

### Shared separator

If panels touch:

```text
1 px separator
+
very subtle directional shadow/highlight
```

Do not introduce fake spacing that changes terminal geometry.

The semantic renderer must remain an overlay over the original cell grid.

---

# Phase 11 — raw/enhanced comparison toggle

Add a simple application toggle:

```text
[ Enhance TUI rendering ]
```

When OFF:

```text
draw terminal faithfully
```

When ON:

```text
separator detection
+
box detection
+
native borders/shadows
```

This is extremely important for evaluating the POC.

The difference should be visually obvious without restarting the application.

A keyboard shortcut such as:

```text
Ctrl+Shift+E
```

is also useful.

---

# Desired POC demo

Use an existing Herdr pane running something visually structured.

Good candidates:

```text
lazygit
btop
k9s
yazi
```

Do not modify those applications.

A successful demonstration should show:

```text
Herdr server
    ↓
existing unmodified TUI
    ↓
native GTK application
```

with:

- live terminal contents
- keyboard interaction
- original colors
- native outer terminal card
- rounded graphical border
- real drop shadow
- at least one graphical separator inside the TUI
- optional simple internal graphical box
- raw/enhanced mode toggle

---

# Acceptance criteria

The POC is considered successful when all of these work.

## Backend

- Existing Herdr server is detected.
- `herdr api snapshot` is parsed.
- Focused pane is identified.
- No Herdr source code is modified.

## Terminal

- Existing pane contents appear in the GTK application.
- Live terminal updates appear.
- ANSI colors are visible.
- Unicode renders correctly for normal common cases.
- Cursor is visible.
- Basic keyboard navigation works.
- Shell input works.
- An interactive TUI such as lazygit can be operated.

## Resizing

- Resizing the GTK window changes terminal rows/columns.
- Herdr and terminal parser remain synchronized.
- No obvious corrupted screen state appears after resize.

## Native rendering

The outer terminal surface has actual:

- border radius
- 1px border
- drop shadow
- padding

These must be rendered as graphics, not Unicode characters.

## Semantic enhancement

With enhancement OFF:

```text
────────
```

is ordinary terminal text.

With enhancement ON:

```text
────────
```

becomes an actual graphical 1px line.

At least one simple box-drawing rectangle must be detected and rendered graphically OR, if the chosen test TUI does not contain an isolated rectangle suitable for shadows, the separator-shadow transformation must be demonstrated clearly.

## Compatibility

If a cell or border cannot be understood:

```text
render normally
```

Never drop terminal content merely because semantic recognition fails.

---

# Explicitly out of scope

Do NOT implement these unless they are required to unblock the POC:

- full Herdr workspace replacement
- multiple panes simultaneously
- Herdr sidebar recreation
- direct Herdr socket client
- Herdr event subscription
- remote Herdr machines
- SSH
- tmux
- Kitty graphics
- sixel
- hyperlinks
- clipboard integration
- drag/drop
- IME
- accessibility
- mouse reporting
- terminal search
- terminal selection
- copy/paste
- persistent application settings
- session restoration
- native tab bar
- arbitrary TUI semantic reconstruction
- full smooth scrolling
- animations
- GPU shader code
- custom Wayland protocol work
- custom terminal server
- replacement PTY implementation

Herdr already solves persistence and PTY ownership.

Do not rebuild those.

---

# Stretch goal 1 — native Herdr workspace sidebar

ONLY after the core POC succeeds.

Use data from:

```bash
herdr api snapshot
```

Render workspaces using ordinary GTK widgets:

```text
┌─────────────────┐
│ Workspaces      │
│                 │
│ ● alpha-api     │
│   optimizer     │
│   frontend      │
│                 │
└─────────────────┘
```

Selecting a workspace can use the public Herdr CLI/API to focus it.

Then reconnect the terminal stream to the newly focused pane.

Do not attempt to duplicate Herdr's full navigation model.

---

# Stretch goal 2 — native Herdr pane layout

Herdr's API exposes tab layout snapshots and pane geometry.

Eventually the architecture can become:

```text
Herdr layout tree
       ↓
native GTK layout

┌────────────────────────────────────┐
│                                    │
│   Terminal card A                  │
│   rounded + shadow                 │
│                                    │
├────────────────────────────────────┤
│                                    │
│   Terminal card B                  │
│                                    │
└────────────────────────────────────┘
```

At that stage Herdr's terminal-character pane borders disappear entirely.

Do NOT build this during the first vertical slice.

---

# Stretch goal 3 — smooth scrolling

Do not fake smooth scrolling by blending screenshots.

Investigate the current `libghostty-vt` render-state APIs for viewport/overscan support.

The correct long-term architecture is:

```text
terminal rows
+
pixel offset
+
overscan rows
=
fractional animated scrolling
```

But this is not required for the first POC.

Internal scrolling implemented by a TUI itself is a different problem and may require frame-to-frame semantic detection.

Keep it out of scope initially.

---

# Development strategy

Build this as vertical slices.

Do NOT implement the entire architecture before testing anything visually.

Order:

```text
1. Herdr snapshot
2. Herdr observe stream
3. ANSI bytes printed/logged
4. libghostty-vt parses bytes
5. inspect cell grid
6. GTK window
7. render plain cells
8. native outer card + shadow
9. interactive control
10. resize
11. separator detection
12. graphical separator
13. simple box detection
14. graphical box + shadow
15. raw/enhanced toggle
```

Every numbered step should leave the program runnable.

---

# Diagnostics

Add a debug overlay activated by an environment variable:

```bash
HERDR_NATIVE_DEBUG=1
```

It should optionally display:

```text
pane id
cols × rows
cell width × height
number of frames received
number of detected separators
number of detected boxes
```

Also allow semantic detections to be highlighted temporarily.

For example:

```text
detected separator → translucent red overlay
detected box       → translucent blue overlay
```

Only in debug mode.

This will make the semantic renderer much easier to develop.

---

# Performance expectations

Do not optimize prematurely.

Target typical grids such as:

```text
120 × 40
160 × 50
200 × 60
```

Avoid creating a heavyweight GTK widget for every terminal cell.

The terminal should be ONE custom drawing widget.

Rendering model:

```text
TerminalView
  ↓
GtkSnapshot / GSK
```

not:

```text
GtkLabel × 10,000
```

Cache:

- font metrics
- Pango layouts where useful
- unchanged semantic box results when feasible

But only after correctness.

---

# Failure behavior

If Herdr is unavailable:

Show:

```text
Herdr server is not running.
Start it with: herdr server
```

If the pane disappears:

Show a disconnected state.

If `libghostty-vt` fails:

log the actual error and stop cleanly.

If semantic detection fails:

render raw cells.

Semantic enhancement must NEVER be able to break basic terminal usability.

---

# Source/API stability

At development start:

1. Record:

```bash
herdr --version
```

2. Export:

```bash
herdr api schema --output herdr-api.schema.json
```

3. Pin a Ghostty git revision.

4. Record:

```bash
zig version
rustc --version
cargo --version
```

Commit these details to the repository.

Because `libghostty-vt` API signatures are still evolving, treat the Ghostty adapter as replaceable infrastructure.

Never spread raw Ghostty FFI calls throughout the project.

---

# README

At the end of the POC, document:

```text
What the POC proves
Architecture
Dependencies
How to build
How to run
How to choose a Herdr pane
How controller takeover works
How to toggle enhancements
Current limitations
Screenshots: raw vs enhanced
Pinned Herdr/Ghostty versions
```

Include one raw/enhanced comparison screenshot if possible.

---

# Final deliverable

The final result should be runnable approximately as:

```bash
cargo run
```

and should:

1. discover the currently focused Herdr pane;
2. open a native GTK window;
3. render its terminal contents;
4. allow keyboard interaction;
5. place it inside a native rounded/shadowed panel;
6. graphically replace at least simple TUI separators;
7. optionally graphically enhance simple detected boxes;
8. provide a toggle between raw and enhanced rendering.

The success criterion is NOT:

> We built another terminal emulator.

The success criterion is:

> We proved that an existing, completely unmodified terminal application can be rendered through a semantic/native graphical layer while Herdr continues to own the actual process and terminal session.

Optimize the implementation for proving or disproving that hypothesis quickly.

<!--
The material below is an accidental duplicate of this specification and is
intentionally excluded from rendered Markdown until it is removed in a
dedicated documentation cleanup.

# Herdr Native Renderer — First POC

## Goal

Build a Linux-only proof of concept that renders an existing Herdr-managed terminal inside a custom native GTK4 application, without modifying Herdr or the application running inside the terminal.

The POC must prove these ideas:

1. Herdr can remain the persistent process/session backend.
2. We can consume a live Herdr terminal stream ourselves.
3. We can parse the ANSI/VT stream into structured terminal cells using `libghostty-vt`.
4. We can render those cells ourselves instead of using a normal terminal emulator.
5. We can surround the terminal with genuinely native graphical UI:
   - rounded corners
   - 1px borders
   - real drop shadows
   - native spacing/padding
6. We can recognize at least some box-drawing characters inside an arbitrary TUI and replace them with graphical separators/borders.
7. The terminal remains interactive.

The POC is specifically intended to answer:

> Can an unmodified TUI be progressively rendered as something that looks like a native application?

Do not attempt to build a complete terminal emulator or full Herdr replacement.

---

# Target environment

Primary development target:

- Linux
- Fedora
- Wayland
- Niri-compatible
- Rust
- GTK4
- libadwaita where useful
- Herdr already installed
- Zig available for building `libghostty-vt`

Use Rust for the application.

Use GTK4 for the window/UI.

Use `libghostty-vt` for VT parsing and terminal state.

Do NOT use:

- Electron
- Tauri
- xterm.js
- a webview
- Ghostty as an external terminal window
- terminal screenshots
- framebuffer post-processing
- Herdr source modifications

---

# Important architectural decision

For this POC, DO NOT talk directly to Herdr's private internals.

Use Herdr's documented public CLI/API surfaces.

The architecture should be:

```text
                    Herdr server
                         │
                  persistent pane
                         │
           herdr terminal session control
                         │
                  NDJSON frames
                         │
                base64 ANSI bytes
                         │
                  libghostty-vt
                         │
                 terminal cell grid
                         │
            semantic enhancement pass
                         │
                    GTK4 renderer
                         │
             native Wayland application
```

Herdr owns:

- processes
- PTYs
- session persistence
- workspaces
- panes
- terminal lifetime

Our application owns:

- graphical layout
- typography
- terminal rendering
- shadows
- borders
- animation
- semantic enhancement

---

# POC scope

The first POC should render ONE focused Herdr pane.

Do not initially reproduce the complete Herdr workspace layout.

The native window should look approximately like:

```text
╭─────────────────────────────────────────────────────────╮
│  Herdr Native                                      ⋮    │
│                                                         │
│  alpha-api                                              │
│                                                         │
│   ╭─────────────────────────────────────────────────╮   │
│   │                                                 │   │
│   │             terminal contents                   │   │
│   │                                                 │   │
│   ╰─────────────────────────────────────────────────╯   │
│             subtle real drop shadow                     │
│                                                         │
╰─────────────────────────────────────────────────────────╯
```

The terminal panel itself must be a real graphical panel.

It must NOT be drawn using:

```text
┌────┐
│    │
└────┘
```

The border, corners and shadow must come from GTK/GSK.

---

# Phase 0 — project bootstrap

Create:

```text
herdr-native-poc/
├── Cargo.toml
├── build.rs
├── README.md
├── src/
│   ├── main.rs
│   │
│   ├── app.rs
│   │
│   ├── herdr/
│   │   ├── mod.rs
│   │   ├── snapshot.rs
│   │   └── terminal_stream.rs
│   │
│   ├── terminal/
│   │   ├── mod.rs
│   │   ├── ghostty.rs
│   │   └── model.rs
│   │
│   ├── render/
│   │   ├── mod.rs
│   │   ├── terminal_view.rs
│   │   ├── cell_renderer.rs
│   │   └── box_detector.rs
│   │
│   └── ui/
│       ├── mod.rs
│       └── theme.rs
│
└── vendor/
    └── ghostty/
```

Suggested Rust dependencies:

```text
gtk4
libadwaita
glib
gio

serde
serde_json
base64

anyhow
thiserror

tracing
tracing-subscriber
```

Use additional dependencies only when justified.

---

# Phase 1 — verify the Herdr integration independently

Before implementing GTK rendering, prove that the Herdr side works.

Run:

```bash
herdr status server
```

If no server is running, report a clear error.

Do NOT automatically kill or replace an existing Herdr server.

Then run:

```bash
herdr api snapshot
```

Parse the returned JSON.

Extract at minimum:

```text
focused_workspace_id
focused_tab_id
focused_pane_id
```

Also extract:

```text
workspace label
pane label if available
terminal title if available
```

Do not guess the exact JSON structure.

Inspect the actual current output and model only the subset required by this POC.

Herdr also exposes:

```bash
herdr api schema --json
```

Use that command whenever protocol details are uncertain.

Prefer the installed Herdr version's schema over assumptions copied from documentation.

---

# Phase 2 — consume a terminal stream

Start READ ONLY.

Spawn:

```bash
herdr terminal session observe <pane-id> --cols 120 --rows 40
```

Read stdout line by line.

Herdr emits newline-delimited JSON records.

Handle at least:

```text
terminal.frame
terminal.closed
```

For every `terminal.frame`:

1. Parse JSON.
2. Extract the base64 ANSI payload.
3. Decode base64.
4. Send the raw bytes into the terminal parser.

Do not parse ANSI manually.

Create an abstraction:

```rust
enum TerminalEvent {
    Data(Vec<u8>),
    Closed,
}
```

`HerdrTerminalStream` should expose terminal events without the GTK code knowing anything about Herdr's NDJSON representation.

---

# Phase 3 — integrate libghostty-vt

Vendor or pin a specific Ghostty revision.

Do not depend on `main` implicitly.

Record the exact Ghostty commit in the README.

Important:

`libghostty-vt` functionality is mature, but its public C API signatures may still change.

Therefore isolate ALL Ghostty-specific FFI inside:

```text
src/terminal/ghostty.rs
```

Nothing outside that module should call raw Ghostty C functions.

If useful, introduce a tiny C shim instead of exposing a large bindgen-generated API to the Rust application.

The Rust-facing interface should look approximately like:

```rust
pub struct TerminalEngine {
    ...
}

impl TerminalEngine {
    pub fn new(cols: u16, rows: u16) -> Result<Self>;

    pub fn feed(&mut self, bytes: &[u8]);

    pub fn resize(&mut self, cols: u16, rows: u16);

    pub fn snapshot(&self) -> TerminalSnapshot;
}
```

Our own neutral representation:

```rust
struct TerminalSnapshot {
    cols: usize,
    rows: usize,
    cursor: Cursor,
    cells: Vec<Cell>,
}

struct Cell {
    x: usize,
    y: usize,

    text: String,

    foreground: Color,
    background: Color,

    bold: bool,
    italic: bool,
    underline: bool,

    wide: bool,
}
```

Adapt this model to the actual current `libghostty-vt` rendering API.

Do not leak Ghostty structs into the renderer.

---

# Phase 4 — build the simplest possible GTK terminal renderer

Implement a custom GTK4 widget:

```text
TerminalView
```

It receives:

```text
TerminalSnapshot
```

and renders it using GTK's snapshot/GSK drawing APIs.

Do NOT start with OpenGL/wgpu/Vulkan.

GTK4 already has a GPU-backed scene graph.

That is sufficient for this POC.

## Typography

Start with:

```text
JetBrains Mono
```

Fallback to the user's default monospace font if unavailable.

Determine actual cell width and height from Pango font metrics.

Do not hardcode character dimensions.

For the first implementation:

- group consecutive cells with compatible styling where practical;
- render backgrounds as rectangles;
- render text with Pango;
- handle normal foreground/background colors;
- handle bold;
- preserve Unicode box characters initially.

Exact terminal fidelity is less important than proving the rendering architecture.

---

# Phase 5 — add the native container

Wrap `TerminalView` inside a visually native card.

Use approximately:

```text
outer app background:
#111318

terminal card:
#171a20

border:
rgba(255,255,255,0.08)

border radius:
8 px

shadow:
0 4px 18px rgba(0,0,0,0.30)

inner padding:
8–12 px
```

Do not obsess over exact colors.

Make these centralized theme constants.

The card MUST use graphical rendering.

Prefer GTK/GSK facilities such as:

```text
rounded clips
border render nodes
outset shadows
```

rather than approximating effects using text characters.

This is the first critical visual proof.

---

# Phase 6 — make the terminal interactive

Once read-only rendering works, replace `observe` with:

```bash
herdr terminal session control <pane-id> --takeover --cols N --rows N
```

IMPORTANT:

Using `--takeover` changes terminal controller ownership.

Make this visible in logs.

Do not silently steal control during startup without explaining it.

The process has:

```text
stdout → terminal.frame stream
stdin  → NDJSON control commands
```

Implement:

```text
terminal.input
terminal.resize
terminal.release
```

Use the exact current schema from:

```bash
herdr api schema --json
```

Do not invent JSON field names.

## Minimum keyboard support

Support:

```text
normal printable characters
Enter
Backspace
Tab
Escape

Arrow Up
Arrow Down
Arrow Left
Arrow Right

Ctrl+C
Ctrl+D
Ctrl+L
Ctrl+R
```

That should be sufficient for an initial shell and many TUIs.

If `libghostty-vt` provides a usable input encoding API at the pinned revision, prefer it over manually maintaining escape sequences.

Otherwise isolate the temporary key encoder in its own module.

Do NOT implement every possible keyboard protocol in this POC.

---

# Phase 7 — terminal resize

When the GTK terminal view changes size:

```text
pixel width
÷
cell width
=
cols
```

and:

```text
pixel height
÷
cell height
=
rows
```

Calculate:

```text
cols >= 1
rows >= 1
```

Debounce rapid GTK resize events slightly if required.

Send:

```text
terminal.resize
```

to Herdr.

Then resize `libghostty-vt` to the same dimensions.

The important invariant is:

```text
Herdr controller dimensions
==
libghostty-vt dimensions
==
our rendered grid dimensions
```

---

# Phase 8 — semantic box-drawing detection

This is the experimental part of the POC.

Create:

```text
src/render/box_detector.rs
```

Recognize at minimum:

```text
─
│

┌
┐
└
┘

╭
╮
╰
╯
```

Optionally recognize:

```text
├
┤
┬
┴
┼
```

later.

Do NOT attempt general UI understanding.

## First transformation: separators

A horizontal sequence such as:

```text
────────────────────────────
```

should NOT be rendered as glyphs.

Instead suppress those cells and draw:

```text
1 physical-pixel line
```

at the appropriate vertical position.

Use a subtle color based on the terminal foreground/background.

Optionally add:

```text
1–3 px soft dark shadow
```

below it.

Similarly:

```text
│
│
│
│
```

should be rendered as a true vertical graphical separator.

This should immediately look smoother than font-based box drawing.

---

# Phase 9 — simple rectangle detection

Then implement detection of CLOSED SIMPLE BOXES.

For example:

```text
┌──────────────────┐
│                  │
│ content          │
│                  │
└──────────────────┘
```

Recognize this as:

```text
PanelRect
```

Only handle rectangles whose perimeter consists entirely of:

```text
horizontal border
vertical border
four compatible corners
```

Ignore complicated intersections.

Pseudo algorithm:

```text
for every top-left corner:
    search right for compatible top-right corner

    verify all cells between are horizontal

    search downward for compatible bottom-left/bottom-right

    verify vertical sides

    verify bottom edge

    if valid:
        emit PanelRect
```

Terminal grids are small.

A simple implementation is fine.

Correctness and debuggability matter more than algorithmic cleverness.

---

# Phase 10 — render detected boxes natively

For every detected `PanelRect`:

1. Suppress the original border glyphs.
2. Preserve the text inside.
3. Draw a graphical rounded rectangle behind/around it.
4. Draw a real 1px border.
5. Add a subtle graphical shadow.

Example transformation:

```text
SOURCE TUI

┌──────────────────────┐
│ Settings             │
├──────────────────────┤
│ Foo                  │
│ Bar                  │
└──────────────────────┘
```

becomes conceptually:

```text
╭────────────────────────╮
│                        │
│  Settings              │
│                        │
│  Foo                   │
│  Bar                   │
│                        │
╰────────────────────────╯
        soft shadow
```

The actual corners must be graphical curves, not `╭` characters.

---

# Important rule: progressive enhancement

Never make terminal rendering depend on successful semantic detection.

The pipeline should be:

```text
cell
 │
 ├─ recognized semantic UI primitive
 │      ↓
 │   graphical rendering
 │
 └─ not recognized
        ↓
     normal terminal rendering
```

If detection fails, the terminal must still work.

This is fundamental to the project.

---

# Handling panel shadows in dense TUIs

TUIs often place boxes directly beside each other:

```text
┌──────────┬───────────┐
│ A        │ B         │
└──────────┴───────────┘
```

Do NOT blindly give every detected rectangle a huge shadow.

For this POC use this hierarchy:

### Standalone box

If there is blank/default-background space surrounding the rectangle:

```text
rounded frame
+
normal drop shadow
```

### Shared separator

If panels touch:

```text
1 px separator
+
very subtle directional shadow/highlight
```

Do not introduce fake spacing that changes terminal geometry.

The semantic renderer must remain an overlay over the original cell grid.

---

# Phase 11 — raw/enhanced comparison toggle

Add a simple application toggle:

```text
[ Enhance TUI rendering ]
```

When OFF:

```text
draw terminal faithfully
```

When ON:

```text
separator detection
+
box detection
+
native borders/shadows
```

This is extremely important for evaluating the POC.

The difference should be visually obvious without restarting the application.

A keyboard shortcut such as:

```text
Ctrl+Shift+E
```

is also useful.

---

# Desired POC demo

Use an existing Herdr pane running something visually structured.

Good candidates:

```text
lazygit
btop
k9s
yazi
```

Do not modify those applications.

A successful demonstration should show:

```text
Herdr server
    ↓
existing unmodified TUI
    ↓
native GTK application
```

with:

- live terminal contents
- keyboard interaction
- original colors
- native outer terminal card
- rounded graphical border
- real drop shadow
- at least one graphical separator inside the TUI
- optional simple internal graphical box
- raw/enhanced mode toggle

---

# Acceptance criteria

The POC is considered successful when all of these work.

## Backend

- Existing Herdr server is detected.
- `herdr api snapshot` is parsed.
- Focused pane is identified.
- No Herdr source code is modified.

## Terminal

- Existing pane contents appear in the GTK application.
- Live terminal updates appear.
- ANSI colors are visible.
- Unicode renders correctly for normal common cases.
- Cursor is visible.
- Basic keyboard navigation works.
- Shell input works.
- An interactive TUI such as lazygit can be operated.

## Resizing

- Resizing the GTK window changes terminal rows/columns.
- Herdr and terminal parser remain synchronized.
- No obvious corrupted screen state appears after resize.

## Native rendering

The outer terminal surface has actual:

- border radius
- 1px border
- drop shadow
- padding

These must be rendered as graphics, not Unicode characters.

## Semantic enhancement

With enhancement OFF:

```text
────────
```

is ordinary terminal text.

With enhancement ON:

```text
────────
```

becomes an actual graphical 1px line.

At least one simple box-drawing rectangle must be detected and rendered graphically OR, if the chosen test TUI does not contain an isolated rectangle suitable for shadows, the separator-shadow transformation must be demonstrated clearly.

## Compatibility

If a cell or border cannot be understood:

```text
render normally
```

Never drop terminal content merely because semantic recognition fails.

---

# Explicitly out of scope

Do NOT implement these unless they are required to unblock the POC:

- full Herdr workspace replacement
- multiple panes simultaneously
- Herdr sidebar recreation
- direct Herdr socket client
- Herdr event subscription
- remote Herdr machines
- SSH
- tmux
- Kitty graphics
- sixel
- hyperlinks
- clipboard integration
- drag/drop
- IME
- accessibility
- mouse reporting
- terminal search
- terminal selection
- copy/paste
- persistent application settings
- session restoration
- native tab bar
- arbitrary TUI semantic reconstruction
- full smooth scrolling
- animations
- GPU shader code
- custom Wayland protocol work
- custom terminal server
- replacement PTY implementation

Herdr already solves persistence and PTY ownership.

Do not rebuild those.

---

# Stretch goal 1 — native Herdr workspace sidebar

ONLY after the core POC succeeds.

Use data from:

```bash
herdr api snapshot
```

Render workspaces using ordinary GTK widgets:

```text
┌─────────────────┐
│ Workspaces      │
│                 │
│ ● alpha-api     │
│   optimizer     │
│   frontend      │
│                 │
└─────────────────┘
```

Selecting a workspace can use the public Herdr CLI/API to focus it.

Then reconnect the terminal stream to the newly focused pane.

Do not attempt to duplicate Herdr's full navigation model.

---

# Stretch goal 2 — native Herdr pane layout

Herdr's API exposes tab layout snapshots and pane geometry.

Eventually the architecture can become:

```text
Herdr layout tree
       ↓
native GTK layout

┌────────────────────────────────────┐
│                                    │
│   Terminal card A                  │
│   rounded + shadow                 │
│                                    │
├────────────────────────────────────┤
│                                    │
│   Terminal card B                  │
│                                    │
└────────────────────────────────────┘
```

At that stage Herdr's terminal-character pane borders disappear entirely.

Do NOT build this during the first vertical slice.

---

# Stretch goal 3 — smooth scrolling

Do not fake smooth scrolling by blending screenshots.

Investigate the current `libghostty-vt` render-state APIs for viewport/overscan support.

The correct long-term architecture is:

```text
terminal rows
+
pixel offset
+
overscan rows
=
fractional animated scrolling
```

But this is not required for the first POC.

Internal scrolling implemented by a TUI itself is a different problem and may require frame-to-frame semantic detection.

Keep it out of scope initially.

---

# Development strategy

Build this as vertical slices.

Do NOT implement the entire architecture before testing anything visually.

Order:

```text
1. Herdr snapshot
2. Herdr observe stream
3. ANSI bytes printed/logged
4. libghostty-vt parses bytes
5. inspect cell grid
6. GTK window
7. render plain cells
8. native outer card + shadow
9. interactive control
10. resize
11. separator detection
12. graphical separator
13. simple box detection
14. graphical box + shadow
15. raw/enhanced toggle
```

Every numbered step should leave the program runnable.

---

# Diagnostics

Add a debug overlay activated by an environment variable:

```bash
HERDR_NATIVE_DEBUG=1
```

It should optionally display:

```text
pane id
cols × rows
cell width × height
number of frames received
number of detected separators
number of detected boxes
```

Also allow semantic detections to be highlighted temporarily.

For example:

```text
detected separator → translucent red overlay
detected box       → translucent blue overlay
```

Only in debug mode.

This will make the semantic renderer much easier to develop.

---

# Performance expectations

Do not optimize prematurely.

Target typical grids such as:

```text
120 × 40
160 × 50
200 × 60
```

Avoid creating a heavyweight GTK widget for every terminal cell.

The terminal should be ONE custom drawing widget.

Rendering model:

```text
TerminalView
  ↓
GtkSnapshot / GSK
```

not:

```text
GtkLabel × 10,000
```

Cache:

- font metrics
- Pango layouts where useful
- unchanged semantic box results when feasible

But only after correctness.

---

# Failure behavior

If Herdr is unavailable:

Show:

```text
Herdr server is not running.
Start it with: herdr server
```

If the pane disappears:

Show a disconnected state.

If `libghostty-vt` fails:

log the actual error and stop cleanly.

If semantic detection fails:

render raw cells.

Semantic enhancement must NEVER be able to break basic terminal usability.

---

# Source/API stability

At development start:

1. Record:

```bash
herdr --version
```

2. Export:

```bash
herdr api schema --output herdr-api.schema.json
```

3. Pin a Ghostty git revision.

4. Record:

```bash
zig version
rustc --version
cargo --version
```

Commit these details to the repository.

Because `libghostty-vt` API signatures are still evolving, treat the Ghostty adapter as replaceable infrastructure.

Never spread raw Ghostty FFI calls throughout the project.

---

# README

At the end of the POC, document:

```text
What the POC proves
Architecture
Dependencies
How to build
How to run
How to choose a Herdr pane
How controller takeover works
How to toggle enhancements
Current limitations
Screenshots: raw vs enhanced
Pinned Herdr/Ghostty versions
```

Include one raw/enhanced comparison screenshot if possible.

---

# Final deliverable

The final result should be runnable approximately as:

```bash
cargo run
```

and should:

1. discover the currently focused Herdr pane;
2. open a native GTK window;
3. render its terminal contents;
4. allow keyboard interaction;
5. place it inside a native rounded/shadowed panel;
6. graphically replace at least simple TUI separators;
7. optionally graphically enhance simple detected boxes;
8. provide a toggle between raw and enhanced rendering.

The success criterion is NOT:

> We built another terminal emulator.

The success criterion is:

> We proved that an existing, completely unmodified terminal application can be rendered through a semantic/native graphical layer while Herdr continues to own the actual process and terminal session.

Optimize the implementation for proving or disproving that hypothesis quickly.# Herdr Native Renderer — First POC

## Goal

Build a Linux-only proof of concept that renders an existing Herdr-managed terminal inside a custom native GTK4 application, without modifying Herdr or the application running inside the terminal.

The POC must prove these ideas:

1. Herdr can remain the persistent process/session backend.
2. We can consume a live Herdr terminal stream ourselves.
3. We can parse the ANSI/VT stream into structured terminal cells using `libghostty-vt`.
4. We can render those cells ourselves instead of using a normal terminal emulator.
5. We can surround the terminal with genuinely native graphical UI:
   - rounded corners
   - 1px borders
   - real drop shadows
   - native spacing/padding
6. We can recognize at least some box-drawing characters inside an arbitrary TUI and replace them with graphical separators/borders.
7. The terminal remains interactive.

The POC is specifically intended to answer:

> Can an unmodified TUI be progressively rendered as something that looks like a native application?

Do not attempt to build a complete terminal emulator or full Herdr replacement.

---

# Target environment

Primary development target:

- Linux
- Fedora
- Wayland
- Niri-compatible
- Rust
- GTK4
- libadwaita where useful
- Herdr already installed
- Zig available for building `libghostty-vt`

Use Rust for the application.

Use GTK4 for the window/UI.

Use `libghostty-vt` for VT parsing and terminal state.

Do NOT use:

- Electron
- Tauri
- xterm.js
- a webview
- Ghostty as an external terminal window
- terminal screenshots
- framebuffer post-processing
- Herdr source modifications

---

# Important architectural decision

For this POC, DO NOT talk directly to Herdr's private internals.

Use Herdr's documented public CLI/API surfaces.

The architecture should be:

```text
                    Herdr server
                         │
                  persistent pane
                         │
           herdr terminal session control
                         │
                  NDJSON frames
                         │
                base64 ANSI bytes
                         │
                  libghostty-vt
                         │
                 terminal cell grid
                         │
            semantic enhancement pass
                         │
                    GTK4 renderer
                         │
             native Wayland application
```

Herdr owns:

- processes
- PTYs
- session persistence
- workspaces
- panes
- terminal lifetime

Our application owns:

- graphical layout
- typography
- terminal rendering
- shadows
- borders
- animation
- semantic enhancement

---

# POC scope

The first POC should render ONE focused Herdr pane.

Do not initially reproduce the complete Herdr workspace layout.

The native window should look approximately like:

```text
╭─────────────────────────────────────────────────────────╮
│  Herdr Native                                      ⋮    │
│                                                         │
│  alpha-api                                              │
│                                                         │
│   ╭─────────────────────────────────────────────────╮   │
│   │                                                 │   │
│   │             terminal contents                   │   │
│   │                                                 │   │
│   ╰─────────────────────────────────────────────────╯   │
│             subtle real drop shadow                     │
│                                                         │
╰─────────────────────────────────────────────────────────╯
```

The terminal panel itself must be a real graphical panel.

It must NOT be drawn using:

```text
┌────┐
│    │
└────┘
```

The border, corners and shadow must come from GTK/GSK.

---

# Phase 0 — project bootstrap

Create:

```text
herdr-native-poc/
├── Cargo.toml
├── build.rs
├── README.md
├── src/
│   ├── main.rs
│   │
│   ├── app.rs
│   │
│   ├── herdr/
│   │   ├── mod.rs
│   │   ├── snapshot.rs
│   │   └── terminal_stream.rs
│   │
│   ├── terminal/
│   │   ├── mod.rs
│   │   ├── ghostty.rs
│   │   └── model.rs
│   │
│   ├── render/
│   │   ├── mod.rs
│   │   ├── terminal_view.rs
│   │   ├── cell_renderer.rs
│   │   └── box_detector.rs
│   │
│   └── ui/
│       ├── mod.rs
│       └── theme.rs
│
└── vendor/
    └── ghostty/
```

Suggested Rust dependencies:

```text
gtk4
libadwaita
glib
gio

serde
serde_json
base64

anyhow
thiserror

tracing
tracing-subscriber
```

Use additional dependencies only when justified.

---

# Phase 1 — verify the Herdr integration independently

Before implementing GTK rendering, prove that the Herdr side works.

Run:

```bash
herdr status server
```

If no server is running, report a clear error.

Do NOT automatically kill or replace an existing Herdr server.

Then run:

```bash
herdr api snapshot
```

Parse the returned JSON.

Extract at minimum:

```text
focused_workspace_id
focused_tab_id
focused_pane_id
```

Also extract:

```text
workspace label
pane label if available
terminal title if available
```

Do not guess the exact JSON structure.

Inspect the actual current output and model only the subset required by this POC.

Herdr also exposes:

```bash
herdr api schema --json
```

Use that command whenever protocol details are uncertain.

Prefer the installed Herdr version's schema over assumptions copied from documentation.

---

# Phase 2 — consume a terminal stream

Start READ ONLY.

Spawn:

```bash
herdr terminal session observe <pane-id> --cols 120 --rows 40
```

Read stdout line by line.

Herdr emits newline-delimited JSON records.

Handle at least:

```text
terminal.frame
terminal.closed
```

For every `terminal.frame`:

1. Parse JSON.
2. Extract the base64 ANSI payload.
3. Decode base64.
4. Send the raw bytes into the terminal parser.

Do not parse ANSI manually.

Create an abstraction:

```rust
enum TerminalEvent {
    Data(Vec<u8>),
    Closed,
}
```

`HerdrTerminalStream` should expose terminal events without the GTK code knowing anything about Herdr's NDJSON representation.

---

# Phase 3 — integrate libghostty-vt

Vendor or pin a specific Ghostty revision.

Do not depend on `main` implicitly.

Record the exact Ghostty commit in the README.

Important:

`libghostty-vt` functionality is mature, but its public C API signatures may still change.

Therefore isolate ALL Ghostty-specific FFI inside:

```text
src/terminal/ghostty.rs
```

Nothing outside that module should call raw Ghostty C functions.

If useful, introduce a tiny C shim instead of exposing a large bindgen-generated API to the Rust application.

The Rust-facing interface should look approximately like:

```rust
pub struct TerminalEngine {
    ...
}

impl TerminalEngine {
    pub fn new(cols: u16, rows: u16) -> Result<Self>;

    pub fn feed(&mut self, bytes: &[u8]);

    pub fn resize(&mut self, cols: u16, rows: u16);

    pub fn snapshot(&self) -> TerminalSnapshot;
}
```

Our own neutral representation:

```rust
struct TerminalSnapshot {
    cols: usize,
    rows: usize,
    cursor: Cursor,
    cells: Vec<Cell>,
}

struct Cell {
    x: usize,
    y: usize,

    text: String,

    foreground: Color,
    background: Color,

    bold: bool,
    italic: bool,
    underline: bool,

    wide: bool,
}
```

Adapt this model to the actual current `libghostty-vt` rendering API.

Do not leak Ghostty structs into the renderer.

---

# Phase 4 — build the simplest possible GTK terminal renderer

Implement a custom GTK4 widget:

```text
TerminalView
```

It receives:

```text
TerminalSnapshot
```

and renders it using GTK's snapshot/GSK drawing APIs.

Do NOT start with OpenGL/wgpu/Vulkan.

GTK4 already has a GPU-backed scene graph.

That is sufficient for this POC.

## Typography

Start with:

```text
JetBrains Mono
```

Fallback to the user's default monospace font if unavailable.

Determine actual cell width and height from Pango font metrics.

Do not hardcode character dimensions.

For the first implementation:

- group consecutive cells with compatible styling where practical;
- render backgrounds as rectangles;
- render text with Pango;
- handle normal foreground/background colors;
- handle bold;
- preserve Unicode box characters initially.

Exact terminal fidelity is less important than proving the rendering architecture.

---

# Phase 5 — add the native container

Wrap `TerminalView` inside a visually native card.

Use approximately:

```text
outer app background:
#111318

terminal card:
#171a20

border:
rgba(255,255,255,0.08)

border radius:
8 px

shadow:
0 4px 18px rgba(0,0,0,0.30)

inner padding:
8–12 px
```

Do not obsess over exact colors.

Make these centralized theme constants.

The card MUST use graphical rendering.

Prefer GTK/GSK facilities such as:

```text
rounded clips
border render nodes
outset shadows
```

rather than approximating effects using text characters.

This is the first critical visual proof.

---

# Phase 6 — make the terminal interactive

Once read-only rendering works, replace `observe` with:

```bash
herdr terminal session control <pane-id> --takeover --cols N --rows N
```

IMPORTANT:

Using `--takeover` changes terminal controller ownership.

Make this visible in logs.

Do not silently steal control during startup without explaining it.

The process has:

```text
stdout → terminal.frame stream
stdin  → NDJSON control commands
```

Implement:

```text
terminal.input
terminal.resize
terminal.release
```

Use the exact current schema from:

```bash
herdr api schema --json
```

Do not invent JSON field names.

## Minimum keyboard support

Support:

```text
normal printable characters
Enter
Backspace
Tab
Escape

Arrow Up
Arrow Down
Arrow Left
Arrow Right

Ctrl+C
Ctrl+D
Ctrl+L
Ctrl+R
```

That should be sufficient for an initial shell and many TUIs.

If `libghostty-vt` provides a usable input encoding API at the pinned revision, prefer it over manually maintaining escape sequences.

Otherwise isolate the temporary key encoder in its own module.

Do NOT implement every possible keyboard protocol in this POC.

---

# Phase 7 — terminal resize

When the GTK terminal view changes size:

```text
pixel width
÷
cell width
=
cols
```

and:

```text
pixel height
÷
cell height
=
rows
```

Calculate:

```text
cols >= 1
rows >= 1
```

Debounce rapid GTK resize events slightly if required.

Send:

```text
terminal.resize
```

to Herdr.

Then resize `libghostty-vt` to the same dimensions.

The important invariant is:

```text
Herdr controller dimensions
==
libghostty-vt dimensions
==
our rendered grid dimensions
```

---

# Phase 8 — semantic box-drawing detection

This is the experimental part of the POC.

Create:

```text
src/render/box_detector.rs
```

Recognize at minimum:

```text
─
│

┌
┐
└
┘

╭
╮
╰
╯
```

Optionally recognize:

```text
├
┤
┬
┴
┼
```

later.

Do NOT attempt general UI understanding.

## First transformation: separators

A horizontal sequence such as:

```text
────────────────────────────
```

should NOT be rendered as glyphs.

Instead suppress those cells and draw:

```text
1 physical-pixel line
```

at the appropriate vertical position.

Use a subtle color based on the terminal foreground/background.

Optionally add:

```text
1–3 px soft dark shadow
```

below it.

Similarly:

```text
│
│
│
│
```

should be rendered as a true vertical graphical separator.

This should immediately look smoother than font-based box drawing.

---

# Phase 9 — simple rectangle detection

Then implement detection of CLOSED SIMPLE BOXES.

For example:

```text
┌──────────────────┐
│                  │
│ content          │
│                  │
└──────────────────┘
```

Recognize this as:

```text
PanelRect
```

Only handle rectangles whose perimeter consists entirely of:

```text
horizontal border
vertical border
four compatible corners
```

Ignore complicated intersections.

Pseudo algorithm:

```text
for every top-left corner:
    search right for compatible top-right corner

    verify all cells between are horizontal

    search downward for compatible bottom-left/bottom-right

    verify vertical sides

    verify bottom edge

    if valid:
        emit PanelRect
```

Terminal grids are small.

A simple implementation is fine.

Correctness and debuggability matter more than algorithmic cleverness.

---

# Phase 10 — render detected boxes natively

For every detected `PanelRect`:

1. Suppress the original border glyphs.
2. Preserve the text inside.
3. Draw a graphical rounded rectangle behind/around it.
4. Draw a real 1px border.
5. Add a subtle graphical shadow.

Example transformation:

```text
SOURCE TUI

┌──────────────────────┐
│ Settings             │
├──────────────────────┤
│ Foo                  │
│ Bar                  │
└──────────────────────┘
```

becomes conceptually:

```text
╭────────────────────────╮
│                        │
│  Settings              │
│                        │
│  Foo                   │
│  Bar                   │
│                        │
╰────────────────────────╯
        soft shadow
```

The actual corners must be graphical curves, not `╭` characters.

---

# Important rule: progressive enhancement

Never make terminal rendering depend on successful semantic detection.

The pipeline should be:

```text
cell
 │
 ├─ recognized semantic UI primitive
 │      ↓
 │   graphical rendering
 │
 └─ not recognized
        ↓
     normal terminal rendering
```

If detection fails, the terminal must still work.

This is fundamental to the project.

---

# Handling panel shadows in dense TUIs

TUIs often place boxes directly beside each other:

```text
┌──────────┬───────────┐
│ A        │ B         │
└──────────┴───────────┘
```

Do NOT blindly give every detected rectangle a huge shadow.

For this POC use this hierarchy:

### Standalone box

If there is blank/default-background space surrounding the rectangle:

```text
rounded frame
+
normal drop shadow
```

### Shared separator

If panels touch:

```text
1 px separator
+
very subtle directional shadow/highlight
```

Do not introduce fake spacing that changes terminal geometry.

The semantic renderer must remain an overlay over the original cell grid.

---

# Phase 11 — raw/enhanced comparison toggle

Add a simple application toggle:

```text
[ Enhance TUI rendering ]
```

When OFF:

```text
draw terminal faithfully
```

When ON:

```text
separator detection
+
box detection
+
native borders/shadows
```

This is extremely important for evaluating the POC.

The difference should be visually obvious without restarting the application.

A keyboard shortcut such as:

```text
Ctrl+Shift+E
```

is also useful.

---

# Desired POC demo

Use an existing Herdr pane running something visually structured.

Good candidates:

```text
lazygit
btop
k9s
yazi
```

Do not modify those applications.

A successful demonstration should show:

```text
Herdr server
    ↓
existing unmodified TUI
    ↓
native GTK application
```

with:

- live terminal contents
- keyboard interaction
- original colors
- native outer terminal card
- rounded graphical border
- real drop shadow
- at least one graphical separator inside the TUI
- optional simple internal graphical box
- raw/enhanced mode toggle

---

# Acceptance criteria

The POC is considered successful when all of these work.

## Backend

- Existing Herdr server is detected.
- `herdr api snapshot` is parsed.
- Focused pane is identified.
- No Herdr source code is modified.

## Terminal

- Existing pane contents appear in the GTK application.
- Live terminal updates appear.
- ANSI colors are visible.
- Unicode renders correctly for normal common cases.
- Cursor is visible.
- Basic keyboard navigation works.
- Shell input works.
- An interactive TUI such as lazygit can be operated.

## Resizing

- Resizing the GTK window changes terminal rows/columns.
- Herdr and terminal parser remain synchronized.
- No obvious corrupted screen state appears after resize.

## Native rendering

The outer terminal surface has actual:

- border radius
- 1px border
- drop shadow
- padding

These must be rendered as graphics, not Unicode characters.

## Semantic enhancement

With enhancement OFF:

```text
────────
```

is ordinary terminal text.

With enhancement ON:

```text
────────
```

becomes an actual graphical 1px line.

At least one simple box-drawing rectangle must be detected and rendered graphically OR, if the chosen test TUI does not contain an isolated rectangle suitable for shadows, the separator-shadow transformation must be demonstrated clearly.

## Compatibility

If a cell or border cannot be understood:

```text
render normally
```

Never drop terminal content merely because semantic recognition fails.

---

# Explicitly out of scope

Do NOT implement these unless they are required to unblock the POC:

- full Herdr workspace replacement
- multiple panes simultaneously
- Herdr sidebar recreation
- direct Herdr socket client
- Herdr event subscription
- remote Herdr machines
- SSH
- tmux
- Kitty graphics
- sixel
- hyperlinks
- clipboard integration
- drag/drop
- IME
- accessibility
- mouse reporting
- terminal search
- terminal selection
- copy/paste
- persistent application settings
- session restoration
- native tab bar
- arbitrary TUI semantic reconstruction
- full smooth scrolling
- animations
- GPU shader code
- custom Wayland protocol work
- custom terminal server
- replacement PTY implementation

Herdr already solves persistence and PTY ownership.

Do not rebuild those.

---

# Stretch goal 1 — native Herdr workspace sidebar

ONLY after the core POC succeeds.

Use data from:

```bash
herdr api snapshot
```

Render workspaces using ordinary GTK widgets:

```text
┌─────────────────┐
│ Workspaces      │
│                 │
│ ● alpha-api     │
│   optimizer     │
│   frontend      │
│                 │
└─────────────────┘
```

Selecting a workspace can use the public Herdr CLI/API to focus it.

Then reconnect the terminal stream to the newly focused pane.

Do not attempt to duplicate Herdr's full navigation model.

---

# Stretch goal 2 — native Herdr pane layout

Herdr's API exposes tab layout snapshots and pane geometry.

Eventually the architecture can become:

```text
Herdr layout tree
       ↓
native GTK layout

┌────────────────────────────────────┐
│                                    │
│   Terminal card A                  │
│   rounded + shadow                 │
│                                    │
├────────────────────────────────────┤
│                                    │
│   Terminal card B                  │
│                                    │
└────────────────────────────────────┘
```

At that stage Herdr's terminal-character pane borders disappear entirely.

Do NOT build this during the first vertical slice.

---

# Stretch goal 3 — smooth scrolling

Do not fake smooth scrolling by blending screenshots.

Investigate the current `libghostty-vt` render-state APIs for viewport/overscan support.

The correct long-term architecture is:

```text
terminal rows
+
pixel offset
+
overscan rows
=
fractional animated scrolling
```

But this is not required for the first POC.

Internal scrolling implemented by a TUI itself is a different problem and may require frame-to-frame semantic detection.

Keep it out of scope initially.

---

# Development strategy

Build this as vertical slices.

Do NOT implement the entire architecture before testing anything visually.

Order:

```text
1. Herdr snapshot
2. Herdr observe stream
3. ANSI bytes printed/logged
4. libghostty-vt parses bytes
5. inspect cell grid
6. GTK window
7. render plain cells
8. native outer card + shadow
9. interactive control
10. resize
11. separator detection
12. graphical separator
13. simple box detection
14. graphical box + shadow
15. raw/enhanced toggle
```

Every numbered step should leave the program runnable.

---

# Diagnostics

Add a debug overlay activated by an environment variable:

```bash
HERDR_NATIVE_DEBUG=1
```

It should optionally display:

```text
pane id
cols × rows
cell width × height
number of frames received
number of detected separators
number of detected boxes
```

Also allow semantic detections to be highlighted temporarily.

For example:

```text
detected separator → translucent red overlay
detected box       → translucent blue overlay
```

Only in debug mode.

This will make the semantic renderer much easier to develop.

---

# Performance expectations

Do not optimize prematurely.

Target typical grids such as:

```text
120 × 40
160 × 50
200 × 60
```

Avoid creating a heavyweight GTK widget for every terminal cell.

The terminal should be ONE custom drawing widget.

Rendering model:

```text
TerminalView
  ↓
GtkSnapshot / GSK
```

not:

```text
GtkLabel × 10,000
```

Cache:

- font metrics
- Pango layouts where useful
- unchanged semantic box results when feasible

But only after correctness.

---

# Failure behavior

If Herdr is unavailable:

Show:

```text
Herdr server is not running.
Start it with: herdr server
```

If the pane disappears:

Show a disconnected state.

If `libghostty-vt` fails:

log the actual error and stop cleanly.

If semantic detection fails:

render raw cells.

Semantic enhancement must NEVER be able to break basic terminal usability.

---

# Source/API stability

At development start:

1. Record:

```bash
herdr --version
```

2. Export:

```bash
herdr api schema --output herdr-api.schema.json
```

3. Pin a Ghostty git revision.

4. Record:

```bash
zig version
rustc --version
cargo --version
```

Commit these details to the repository.

Because `libghostty-vt` API signatures are still evolving, treat the Ghostty adapter as replaceable infrastructure.

Never spread raw Ghostty FFI calls throughout the project.

---

# README

At the end of the POC, document:

```text
What the POC proves
Architecture
Dependencies
How to build
How to run
How to choose a Herdr pane
How controller takeover works
How to toggle enhancements
Current limitations
Screenshots: raw vs enhanced
Pinned Herdr/Ghostty versions
```

Include one raw/enhanced comparison screenshot if possible.

---

# Final deliverable

The final result should be runnable approximately as:

```bash
cargo run
```

and should:

1. discover the currently focused Herdr pane;
2. open a native GTK window;
3. render its terminal contents;
4. allow keyboard interaction;
5. place it inside a native rounded/shadowed panel;
6. graphically replace at least simple TUI separators;
7. optionally graphically enhance simple detected boxes;
8. provide a toggle between raw and enhanced rendering.

The success criterion is NOT:

> We built another terminal emulator.

The success criterion is:

> We proved that an existing, completely unmodified terminal application can be rendered through a semantic/native graphical layer while Herdr continues to own the actual process and terminal session.

Optimize the implementation for proving or disproving that hypothesis quickly.# Herdr Native Renderer — First POC

## Goal

Build a Linux-only proof of concept that renders an existing Herdr-managed terminal inside a custom native GTK4 application, without modifying Herdr or the application running inside the terminal.

The POC must prove these ideas:

1. Herdr can remain the persistent process/session backend.
2. We can consume a live Herdr terminal stream ourselves.
3. We can parse the ANSI/VT stream into structured terminal cells using `libghostty-vt`.
4. We can render those cells ourselves instead of using a normal terminal emulator.
5. We can surround the terminal with genuinely native graphical UI:
   - rounded corners
   - 1px borders
   - real drop shadows
   - native spacing/padding
6. We can recognize at least some box-drawing characters inside an arbitrary TUI and replace them with graphical separators/borders.
7. The terminal remains interactive.

The POC is specifically intended to answer:

> Can an unmodified TUI be progressively rendered as something that looks like a native application?

Do not attempt to build a complete terminal emulator or full Herdr replacement.

---

# Target environment

Primary development target:

- Linux
- Fedora
- Wayland
- Niri-compatible
- Rust
- GTK4
- libadwaita where useful
- Herdr already installed
- Zig available for building `libghostty-vt`

Use Rust for the application.

Use GTK4 for the window/UI.

Use `libghostty-vt` for VT parsing and terminal state.

Do NOT use:

- Electron
- Tauri
- xterm.js
- a webview
- Ghostty as an external terminal window
- terminal screenshots
- framebuffer post-processing
- Herdr source modifications

---

# Important architectural decision

For this POC, DO NOT talk directly to Herdr's private internals.

Use Herdr's documented public CLI/API surfaces.

The architecture should be:

```text
                    Herdr server
                         │
                  persistent pane
                         │
           herdr terminal session control
                         │
                  NDJSON frames
                         │
                base64 ANSI bytes
                         │
                  libghostty-vt
                         │
                 terminal cell grid
                         │
            semantic enhancement pass
                         │
                    GTK4 renderer
                         │
             native Wayland application
```

Herdr owns:

- processes
- PTYs
- session persistence
- workspaces
- panes
- terminal lifetime

Our application owns:

- graphical layout
- typography
- terminal rendering
- shadows
- borders
- animation
- semantic enhancement

---

# POC scope

The first POC should render ONE focused Herdr pane.

Do not initially reproduce the complete Herdr workspace layout.

The native window should look approximately like:

```text
╭─────────────────────────────────────────────────────────╮
│  Herdr Native                                      ⋮    │
│                                                         │
│  alpha-api                                              │
│                                                         │
│   ╭─────────────────────────────────────────────────╮   │
│   │                                                 │   │
│   │             terminal contents                   │   │
│   │                                                 │   │
│   ╰─────────────────────────────────────────────────╯   │
│             subtle real drop shadow                     │
│                                                         │
╰─────────────────────────────────────────────────────────╯
```

The terminal panel itself must be a real graphical panel.

It must NOT be drawn using:

```text
┌────┐
│    │
└────┘
```

The border, corners and shadow must come from GTK/GSK.

---

# Phase 0 — project bootstrap

Create:

```text
herdr-native-poc/
├── Cargo.toml
├── build.rs
├── README.md
├── src/
│   ├── main.rs
│   │
│   ├── app.rs
│   │
│   ├── herdr/
│   │   ├── mod.rs
│   │   ├── snapshot.rs
│   │   └── terminal_stream.rs
│   │
│   ├── terminal/
│   │   ├── mod.rs
│   │   ├── ghostty.rs
│   │   └── model.rs
│   │
│   ├── render/
│   │   ├── mod.rs
│   │   ├── terminal_view.rs
│   │   ├── cell_renderer.rs
│   │   └── box_detector.rs
│   │
│   └── ui/
│       ├── mod.rs
│       └── theme.rs
│
└── vendor/
    └── ghostty/
```

Suggested Rust dependencies:

```text
gtk4
libadwaita
glib
gio

serde
serde_json
base64

anyhow
thiserror

tracing
tracing-subscriber
```

Use additional dependencies only when justified.

---

# Phase 1 — verify the Herdr integration independently

Before implementing GTK rendering, prove that the Herdr side works.

Run:

```bash
herdr status server
```

If no server is running, report a clear error.

Do NOT automatically kill or replace an existing Herdr server.

Then run:

```bash
herdr api snapshot
```

Parse the returned JSON.

Extract at minimum:

```text
focused_workspace_id
focused_tab_id
focused_pane_id
```

Also extract:

```text
workspace label
pane label if available
terminal title if available
```

Do not guess the exact JSON structure.

Inspect the actual current output and model only the subset required by this POC.

Herdr also exposes:

```bash
herdr api schema --json
```

Use that command whenever protocol details are uncertain.

Prefer the installed Herdr version's schema over assumptions copied from documentation.

---

# Phase 2 — consume a terminal stream

Start READ ONLY.

Spawn:

```bash
herdr terminal session observe <pane-id> --cols 120 --rows 40
```

Read stdout line by line.

Herdr emits newline-delimited JSON records.

Handle at least:

```text
terminal.frame
terminal.closed
```

For every `terminal.frame`:

1. Parse JSON.
2. Extract the base64 ANSI payload.
3. Decode base64.
4. Send the raw bytes into the terminal parser.

Do not parse ANSI manually.

Create an abstraction:

```rust
enum TerminalEvent {
    Data(Vec<u8>),
    Closed,
}
```

`HerdrTerminalStream` should expose terminal events without the GTK code knowing anything about Herdr's NDJSON representation.

---

# Phase 3 — integrate libghostty-vt

Vendor or pin a specific Ghostty revision.

Do not depend on `main` implicitly.

Record the exact Ghostty commit in the README.

Important:

`libghostty-vt` functionality is mature, but its public C API signatures may still change.

Therefore isolate ALL Ghostty-specific FFI inside:

```text
src/terminal/ghostty.rs
```

Nothing outside that module should call raw Ghostty C functions.

If useful, introduce a tiny C shim instead of exposing a large bindgen-generated API to the Rust application.

The Rust-facing interface should look approximately like:

```rust
pub struct TerminalEngine {
    ...
}

impl TerminalEngine {
    pub fn new(cols: u16, rows: u16) -> Result<Self>;

    pub fn feed(&mut self, bytes: &[u8]);

    pub fn resize(&mut self, cols: u16, rows: u16);

    pub fn snapshot(&self) -> TerminalSnapshot;
}
```

Our own neutral representation:

```rust
struct TerminalSnapshot {
    cols: usize,
    rows: usize,
    cursor: Cursor,
    cells: Vec<Cell>,
}

struct Cell {
    x: usize,
    y: usize,

    text: String,

    foreground: Color,
    background: Color,

    bold: bool,
    italic: bool,
    underline: bool,

    wide: bool,
}
```

Adapt this model to the actual current `libghostty-vt` rendering API.

Do not leak Ghostty structs into the renderer.

---

# Phase 4 — build the simplest possible GTK terminal renderer

Implement a custom GTK4 widget:

```text
TerminalView
```

It receives:

```text
TerminalSnapshot
```

and renders it using GTK's snapshot/GSK drawing APIs.

Do NOT start with OpenGL/wgpu/Vulkan.

GTK4 already has a GPU-backed scene graph.

That is sufficient for this POC.

## Typography

Start with:

```text
JetBrains Mono
```

Fallback to the user's default monospace font if unavailable.

Determine actual cell width and height from Pango font metrics.

Do not hardcode character dimensions.

For the first implementation:

- group consecutive cells with compatible styling where practical;
- render backgrounds as rectangles;
- render text with Pango;
- handle normal foreground/background colors;
- handle bold;
- preserve Unicode box characters initially.

Exact terminal fidelity is less important than proving the rendering architecture.

---

# Phase 5 — add the native container

Wrap `TerminalView` inside a visually native card.

Use approximately:

```text
outer app background:
#111318

terminal card:
#171a20

border:
rgba(255,255,255,0.08)

border radius:
8 px

shadow:
0 4px 18px rgba(0,0,0,0.30)

inner padding:
8–12 px
```

Do not obsess over exact colors.

Make these centralized theme constants.

The card MUST use graphical rendering.

Prefer GTK/GSK facilities such as:

```text
rounded clips
border render nodes
outset shadows
```

rather than approximating effects using text characters.

This is the first critical visual proof.

---

# Phase 6 — make the terminal interactive

Once read-only rendering works, replace `observe` with:

```bash
herdr terminal session control <pane-id> --takeover --cols N --rows N
```

IMPORTANT:

Using `--takeover` changes terminal controller ownership.

Make this visible in logs.

Do not silently steal control during startup without explaining it.

The process has:

```text
stdout → terminal.frame stream
stdin  → NDJSON control commands
```

Implement:

```text
terminal.input
terminal.resize
terminal.release
```

Use the exact current schema from:

```bash
herdr api schema --json
```

Do not invent JSON field names.

## Minimum keyboard support

Support:

```text
normal printable characters
Enter
Backspace
Tab
Escape

Arrow Up
Arrow Down
Arrow Left
Arrow Right

Ctrl+C
Ctrl+D
Ctrl+L
Ctrl+R
```

That should be sufficient for an initial shell and many TUIs.

If `libghostty-vt` provides a usable input encoding API at the pinned revision, prefer it over manually maintaining escape sequences.

Otherwise isolate the temporary key encoder in its own module.

Do NOT implement every possible keyboard protocol in this POC.

---

# Phase 7 — terminal resize

When the GTK terminal view changes size:

```text
pixel width
÷
cell width
=
cols
```

and:

```text
pixel height
÷
cell height
=
rows
```

Calculate:

```text
cols >= 1
rows >= 1
```

Debounce rapid GTK resize events slightly if required.

Send:

```text
terminal.resize
```

to Herdr.

Then resize `libghostty-vt` to the same dimensions.

The important invariant is:

```text
Herdr controller dimensions
==
libghostty-vt dimensions
==
our rendered grid dimensions
```

---

# Phase 8 — semantic box-drawing detection

This is the experimental part of the POC.

Create:

```text
src/render/box_detector.rs
```

Recognize at minimum:

```text
─
│

┌
┐
└
┘

╭
╮
╰
╯
```

Optionally recognize:

```text
├
┤
┬
┴
┼
```

later.

Do NOT attempt general UI understanding.

## First transformation: separators

A horizontal sequence such as:

```text
────────────────────────────
```

should NOT be rendered as glyphs.

Instead suppress those cells and draw:

```text
1 physical-pixel line
```

at the appropriate vertical position.

Use a subtle color based on the terminal foreground/background.

Optionally add:

```text
1–3 px soft dark shadow
```

below it.

Similarly:

```text
│
│
│
│
```

should be rendered as a true vertical graphical separator.

This should immediately look smoother than font-based box drawing.

---

# Phase 9 — simple rectangle detection

Then implement detection of CLOSED SIMPLE BOXES.

For example:

```text
┌──────────────────┐
│                  │
│ content          │
│                  │
└──────────────────┘
```

Recognize this as:

```text
PanelRect
```

Only handle rectangles whose perimeter consists entirely of:

```text
horizontal border
vertical border
four compatible corners
```

Ignore complicated intersections.

Pseudo algorithm:

```text
for every top-left corner:
    search right for compatible top-right corner

    verify all cells between are horizontal

    search downward for compatible bottom-left/bottom-right

    verify vertical sides

    verify bottom edge

    if valid:
        emit PanelRect
```

Terminal grids are small.

A simple implementation is fine.

Correctness and debuggability matter more than algorithmic cleverness.

---

# Phase 10 — render detected boxes natively

For every detected `PanelRect`:

1. Suppress the original border glyphs.
2. Preserve the text inside.
3. Draw a graphical rounded rectangle behind/around it.
4. Draw a real 1px border.
5. Add a subtle graphical shadow.

Example transformation:

```text
SOURCE TUI

┌──────────────────────┐
│ Settings             │
├──────────────────────┤
│ Foo                  │
│ Bar                  │
└──────────────────────┘
```

becomes conceptually:

```text
╭────────────────────────╮
│                        │
│  Settings              │
│                        │
│  Foo                   │
│  Bar                   │
│                        │
╰────────────────────────╯
        soft shadow
```

The actual corners must be graphical curves, not `╭` characters.

---

# Important rule: progressive enhancement

Never make terminal rendering depend on successful semantic detection.

The pipeline should be:

```text
cell
 │
 ├─ recognized semantic UI primitive
 │      ↓
 │   graphical rendering
 │
 └─ not recognized
        ↓
     normal terminal rendering
```

If detection fails, the terminal must still work.

This is fundamental to the project.

---

# Handling panel shadows in dense TUIs

TUIs often place boxes directly beside each other:

```text
┌──────────┬───────────┐
│ A        │ B         │
└──────────┴───────────┘
```

Do NOT blindly give every detected rectangle a huge shadow.

For this POC use this hierarchy:

### Standalone box

If there is blank/default-background space surrounding the rectangle:

```text
rounded frame
+
normal drop shadow
```

### Shared separator

If panels touch:

```text
1 px separator
+
very subtle directional shadow/highlight
```

Do not introduce fake spacing that changes terminal geometry.

The semantic renderer must remain an overlay over the original cell grid.

---

# Phase 11 — raw/enhanced comparison toggle

Add a simple application toggle:

```text
[ Enhance TUI rendering ]
```

When OFF:

```text
draw terminal faithfully
```

When ON:

```text
separator detection
+
box detection
+
native borders/shadows
```

This is extremely important for evaluating the POC.

The difference should be visually obvious without restarting the application.

A keyboard shortcut such as:

```text
Ctrl+Shift+E
```

is also useful.

---

# Desired POC demo

Use an existing Herdr pane running something visually structured.

Good candidates:

```text
lazygit
btop
k9s
yazi
```

Do not modify those applications.

A successful demonstration should show:

```text
Herdr server
    ↓
existing unmodified TUI
    ↓
native GTK application
```

with:

- live terminal contents
- keyboard interaction
- original colors
- native outer terminal card
- rounded graphical border
- real drop shadow
- at least one graphical separator inside the TUI
- optional simple internal graphical box
- raw/enhanced mode toggle

---

# Acceptance criteria

The POC is considered successful when all of these work.

## Backend

- Existing Herdr server is detected.
- `herdr api snapshot` is parsed.
- Focused pane is identified.
- No Herdr source code is modified.

## Terminal

- Existing pane contents appear in the GTK application.
- Live terminal updates appear.
- ANSI colors are visible.
- Unicode renders correctly for normal common cases.
- Cursor is visible.
- Basic keyboard navigation works.
- Shell input works.
- An interactive TUI such as lazygit can be operated.

## Resizing

- Resizing the GTK window changes terminal rows/columns.
- Herdr and terminal parser remain synchronized.
- No obvious corrupted screen state appears after resize.

## Native rendering

The outer terminal surface has actual:

- border radius
- 1px border
- drop shadow
- padding

These must be rendered as graphics, not Unicode characters.

## Semantic enhancement

With enhancement OFF:

```text
────────
```

is ordinary terminal text.

With enhancement ON:

```text
────────
```

becomes an actual graphical 1px line.

At least one simple box-drawing rectangle must be detected and rendered graphically OR, if the chosen test TUI does not contain an isolated rectangle suitable for shadows, the separator-shadow transformation must be demonstrated clearly.

## Compatibility

If a cell or border cannot be understood:

```text
render normally
```

Never drop terminal content merely because semantic recognition fails.

---

# Explicitly out of scope

Do NOT implement these unless they are required to unblock the POC:

- full Herdr workspace replacement
- multiple panes simultaneously
- Herdr sidebar recreation
- direct Herdr socket client
- Herdr event subscription
- remote Herdr machines
- SSH
- tmux
- Kitty graphics
- sixel
- hyperlinks
- clipboard integration
- drag/drop
- IME
- accessibility
- mouse reporting
- terminal search
- terminal selection
- copy/paste
- persistent application settings
- session restoration
- native tab bar
- arbitrary TUI semantic reconstruction
- full smooth scrolling
- animations
- GPU shader code
- custom Wayland protocol work
- custom terminal server
- replacement PTY implementation

Herdr already solves persistence and PTY ownership.

Do not rebuild those.

---

# Stretch goal 1 — native Herdr workspace sidebar

ONLY after the core POC succeeds.

Use data from:

```bash
herdr api snapshot
```

Render workspaces using ordinary GTK widgets:

```text
┌─────────────────┐
│ Workspaces      │
│                 │
│ ● alpha-api     │
│   optimizer     │
│   frontend      │
│                 │
└─────────────────┘
```

Selecting a workspace can use the public Herdr CLI/API to focus it.

Then reconnect the terminal stream to the newly focused pane.

Do not attempt to duplicate Herdr's full navigation model.

---

# Stretch goal 2 — native Herdr pane layout

Herdr's API exposes tab layout snapshots and pane geometry.

Eventually the architecture can become:

```text
Herdr layout tree
       ↓
native GTK layout

┌────────────────────────────────────┐
│                                    │
│   Terminal card A                  │
│   rounded + shadow                 │
│                                    │
├────────────────────────────────────┤
│                                    │
│   Terminal card B                  │
│                                    │
└────────────────────────────────────┘
```

At that stage Herdr's terminal-character pane borders disappear entirely.

Do NOT build this during the first vertical slice.

---

# Stretch goal 3 — smooth scrolling

Do not fake smooth scrolling by blending screenshots.

Investigate the current `libghostty-vt` render-state APIs for viewport/overscan support.

The correct long-term architecture is:

```text
terminal rows
+
pixel offset
+
overscan rows
=
fractional animated scrolling
```

But this is not required for the first POC.

Internal scrolling implemented by a TUI itself is a different problem and may require frame-to-frame semantic detection.

Keep it out of scope initially.

---

# Development strategy

Build this as vertical slices.

Do NOT implement the entire architecture before testing anything visually.

Order:

```text
1. Herdr snapshot
2. Herdr observe stream
3. ANSI bytes printed/logged
4. libghostty-vt parses bytes
5. inspect cell grid
6. GTK window
7. render plain cells
8. native outer card + shadow
9. interactive control
10. resize
11. separator detection
12. graphical separator
13. simple box detection
14. graphical box + shadow
15. raw/enhanced toggle
```

Every numbered step should leave the program runnable.

---

# Diagnostics

Add a debug overlay activated by an environment variable:

```bash
HERDR_NATIVE_DEBUG=1
```

It should optionally display:

```text
pane id
cols × rows
cell width × height
number of frames received
number of detected separators
number of detected boxes
```

Also allow semantic detections to be highlighted temporarily.

For example:

```text
detected separator → translucent red overlay
detected box       → translucent blue overlay
```

Only in debug mode.

This will make the semantic renderer much easier to develop.

---

# Performance expectations

Do not optimize prematurely.

Target typical grids such as:

```text
120 × 40
160 × 50
200 × 60
```

Avoid creating a heavyweight GTK widget for every terminal cell.

The terminal should be ONE custom drawing widget.

Rendering model:

```text
TerminalView
  ↓
GtkSnapshot / GSK
```

not:

```text
GtkLabel × 10,000
```

Cache:

- font metrics
- Pango layouts where useful
- unchanged semantic box results when feasible

But only after correctness.

---

# Failure behavior

If Herdr is unavailable:

Show:

```text
Herdr server is not running.
Start it with: herdr server
```

If the pane disappears:

Show a disconnected state.

If `libghostty-vt` fails:

log the actual error and stop cleanly.

If semantic detection fails:

render raw cells.

Semantic enhancement must NEVER be able to break basic terminal usability.

---

# Source/API stability

At development start:

1. Record:

```bash
herdr --version
```

2. Export:

```bash
herdr api schema --output herdr-api.schema.json
```

3. Pin a Ghostty git revision.

4. Record:

```bash
zig version
rustc --version
cargo --version
```

Commit these details to the repository.

Because `libghostty-vt` API signatures are still evolving, treat the Ghostty adapter as replaceable infrastructure.

Never spread raw Ghostty FFI calls throughout the project.

---

# README

At the end of the POC, document:

```text
What the POC proves
Architecture
Dependencies
How to build
How to run
How to choose a Herdr pane
How controller takeover works
How to toggle enhancements
Current limitations
Screenshots: raw vs enhanced
Pinned Herdr/Ghostty versions
```

Include one raw/enhanced comparison screenshot if possible.

---

# Final deliverable

The final result should be runnable approximately as:

```bash
cargo run
```

and should:

1. discover the currently focused Herdr pane;
2. open a native GTK window;
3. render its terminal contents;
4. allow keyboard interaction;
5. place it inside a native rounded/shadowed panel;
6. graphically replace at least simple TUI separators;
7. optionally graphically enhance simple detected boxes;
8. provide a toggle between raw and enhanced rendering.

The success criterion is NOT:

> We built another terminal emulator.

The success criterion is:

> We proved that an existing, completely unmodified terminal application can be rendered through a semantic/native graphical layer while Herdr continues to own the actual process and terminal session.

Optimize the implementation for proving or disproving that hypothesis quickly.
-->
