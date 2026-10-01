# 00: Installed API Discovery

## Purpose

Replace all protocol and FFI assumptions with facts from the developer machine. This is a research-only wave and has three parallel agents.

## Agent A: Herdr Protocol

Run, without changing server state:

```bash
herdr --version
herdr status server
herdr api schema --json
herdr api snapshot
```

If a server is available, use a focused pane to capture a short `observe` sample at `120x40`. Do not use `control` or `--takeover` in this task.

Record in the canonical `herdr-native-poc/docs/contracts/` artifacts defined in
`dev/README.md`:

- Herdr version and server failure output.
- Actual snapshot JSON paths for focused workspace, tab, pane, labels, and terminal title.
- Exact NDJSON discriminators and base64 payload path for `terminal.frame` and `terminal.closed`.
- Exact `terminal.input`, `terminal.resize`, and `terminal.release` request schemas from the installed API schema.
- Any command exit/status behavior worth surfacing to users.
- Whether the first frame initializes complete terminal state or is incremental;
  whether records have sequence numbers, reset markers, or acknowledgements;
  and how reconnecting behaves.
- The behavior when a pane changes focus, disappears, the observer exits, a
  controller disconnects, a resize is accepted/rejected, or input is rejected.
- A bounded stderr sample and exit status for success, unavailable server, and
  closed-pane cases. Do not record terminal payloads in logs or fixtures.

Deliver redacted fixtures for initial stream, incremental update, reconnect,
resize, close, malformed record, and invalid payload tests. Never include
private terminal content in a committed fixture. Add a fixture-redaction check
that rejects known local usernames, home paths, and unredacted terminal titles.

## Agent B: Ghostty FFI

Choose an explicit Ghostty revision and inspect its vendorable source/build instructions. Determine:

- Whether `libghostty-vt` exposes a stable enough C API for terminal creation, feed, resize, grid iteration, styles, cursor, and destruction.
- Exact library/header generation and link steps required from Cargo `build.rs` using Zig.
- Whether a C shim is required to turn the current API into the planned Rust interface.
- Known limitations for colours, wide cells, combining text, cursor visibility, and keyboard encoding.

Deliver the selected commit hash, source provenance/license, Zig version, API
signatures, required Fedora packages, and API limitations. Do not let raw C
types escape the proposed adapter.

## Agent C: GTK4 Feasibility

Validate versions installed on Fedora and Rust GTK bindings to be used. Identify concrete GTK APIs for:

- a single custom snapshot widget;
- Pango font metric measurement and monospace fallback;
- background/text drawing, rounded clipping, borders, and shadows;
- keyboard event controllers and focus;
- widget allocation changes and debounced resize;
- application actions for Ctrl+Shift+E.

State any binding limitations and the simplest supported alternative.

## Gate

The foundation owner must verify the canonical artifacts before code starts. If
Herdr is unavailable, implement only fixture-driven paths and retain a clear
runtime unavailable state.
