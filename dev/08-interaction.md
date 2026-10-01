# 08: Control, Input, and Resize

## Owner Paths

`herdr-native-poc/src/herdr/terminal_stream.rs` control additions, a dedicated
input-encoding module if needed, and minimal app/view event wiring.

## Dependencies

Read-only vertical-slice gate and exact installed control schema.

## Work

1. Make takeover explicit before launch: UI affordance or clearly logged startup message stating pane ID and ownership change.
2. Stop and reap the observer, invalidate its snapshot generation, revalidate
   the pane, then spawn the discovered `control <pane> --takeover --cols N
   --rows N` command only after the agreed interaction point. Reinitialize from
   the controller stream according to the discovered resync contract.
3. Send exact schema-derived NDJSON to stdin for input, resize, and release;
   serialize writes so records cannot interleave, and surface each discovered
   acknowledgement/rejection outcome.
4. Map printable text, Enter, Backspace, Tab, Escape, arrows, Ctrl+C/D/L/R.
   Keep application shortcuts out of the terminal input path. Prefer pinned
   Ghostty input encoding when suitable; otherwise keep limited escape mapping
   isolated and tested. Compose input, IME, mouse, clipboard, key-release, and
   unsupported keyboard protocols remain explicitly out of scope.
5. Give `TerminalView` focus and route keys only while focused.
6. Calculate dimensions from allocated logical pixels divided by measured cell
   metrics, clamp each to at least 1, debounce rapid changes, and serialize a
   single pending target. Apply the engine resize only at the discovered
   controller acknowledgement/frame boundary; retain the last confirmed
   dimensions if resize is rejected.
7. Release control on every orderly shutdown path, including window close,
   stream failure, and process termination. Time-bound cleanup and log release
   failure without masking the original exit error. Document that forced process
   termination cannot guarantee release and record Herdr's disconnect behavior.

## Invariant

```text
confirmed Herdr controller dimensions == TerminalEngine dimensions == rendered snapshot dimensions
```

## Gate Demo

- Shell typing and required controls work.
- A selected TUI supports keyboard navigation.
- Window resize changes remote terminal geometry without obvious corruption.
- Logs show takeover, resize acknowledgements/rejections, and release.
