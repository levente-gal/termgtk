# 09: Validation and Documentation

## Parallel Owners

QA owns validation evidence; documentation owner owns `herdr-native-poc/README.md`. Neither changes core implementation without routing defects to the responsible work package owner.

## Acceptance Matrix

| Area | Evidence |
| --- | --- |
| Backend | Existing server detection; actual snapshot resolves focused pane; no Herdr source modified |
| Stream | Existing pane appears and live updates are visible; reconnect/resync never retains a stale frame or unbounded GTK queue |
| Terminal | Supported ANSI colour forms, common Unicode, cursor, shell typing, and keyboard navigation work; unsupported features match the documented fallback matrix |
| Resize | Repeated accepted and rejected resizes keep confirmed controller/parser/render dimensions aligned |
| Native UI | Outer card visibly has graphical radius, border, shadow, and padding |
| Semantics | Off renders raw glyphs; on turns at least one qualifying separator into a graphical line; ambiguous, intersecting, overlapping, and unrecognized glyphs remain raw |
| Failure paths | Unavailable server, changed/closed pane, malformed record, child failure, rejected resize, and detector failure give clear UI/log states without logging payloads |
| Ownership | Takeover is explicit; observer-to-controller handoff, orderly shutdown release, and documented forced-termination behavior are evidenced |
| Reproducibility | Locked Ghostty source, `Cargo.lock`, Zig/system dependency versions, schema hash, and all redacted contract fixtures are present |

## Demo Procedure

1. Record the canonical discovery artifacts, including installed Herdr, Ghostty
   commit, Zig, Rust, Cargo, GTK, and schema-hash versions.
2. Start or identify an existing Herdr pane running `lazygit`, `btop`, `k9s`, or `yazi`; do not alter the application.
3. Run `cargo run`, verify focused-pane selection, and capture raw mode.
4. Enable enhancement with the toggle/shortcut and capture the same state.
5. Exercise text input, controls, accepted/rejected resize, observer reconnect,
   and orderly close after explicitly accepting takeover.
6. Repeat against a basic shell to distinguish application issues from the
   chosen TUI, then replay the fixture suite without a live Herdr server.

## README Requirements

- Hypothesis proved and architecture diagram.
- Fedora/Wayland dependencies and build/run steps.
- Focused-pane selection and controller takeover/release behavior.
- Enhancement toggle and debug environment variable.
- Exact Herdr/Ghostty versions and Ghostty commit.
- Supported fidelity/input matrix, schema hash, redaction policy, and stream
  reconnect/resize semantics.
- Current limitations and explicit out-of-scope items.
- Raw/enhanced comparison image when practical.

## Completion

The POC is complete only after the matrix is backed by a successful local run or a clearly labeled environment blocker. Do not claim full terminal-emulator fidelity; report the result as evidence for or against progressive semantic/native rendering.
