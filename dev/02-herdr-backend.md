# 02: Herdr Backend

## Owner Paths

`herdr-native-poc/src/herdr/**` only.

## Dependencies

Discovery contract and the neutral `TerminalEvent` definition.

## Work

1. Implement server detection by executing `herdr status server`; provide an error that says how to start Herdr, without killing or replacing anything.
2. Verify the installed Herdr version and schema hash against the discovery
   contract; refuse protocol operations with a clear rediscovery message when
   they differ. Implement `herdr api snapshot`, parse only verified paths, and
   expose a compact `FocusedPane` with IDs and optional labels/title.
3. Spawn `herdr terminal session observe <pane> --cols <n> --rows <n>` with
   argument-vector process creation, piped stdout/stderr, and no shell.
4. Read stdout line-by-line, deserialize NDJSON, decode only verified
   `terminal.frame` base64 payloads, and emit `TerminalEvent::Data`. Enforce the
   discovered ordering/reset semantics; do not assume every frame is complete.
5. Emit a structured close reason for the verified closed event. Surface
   malformed records, process termination, timeout, and bounded stderr with
   contextual errors, never raw terminal payloads.
6. Keep asynchronous/process details hidden from GTK. The backend worker owns
   child I/O and exposes a bounded, backpressured `TerminalEvent` stream; it
   never mutates terminal state or constructs snapshots. The integration worker
   owns terminal mutation and publishes coalesced latest snapshots to GTK.
7. Define `ObserveSession` separately from the later `ControlSession`; the
   latter owns serialized stdin requests, request outcomes, and child cleanup.

## Tests

- Snapshot fixture maps to the expected focused IDs and labels.
- Frame fixture decodes exact ANSI bytes.
- Closed, invalid JSON, absent payload, and invalid base64 are deterministic failures/events.
- Command construction includes provided dimensions and never adds `--takeover`.
- Initial, incremental, reconnect, resize, closed, non-zero-exit, timeout, and
  stderr fixtures produce the documented result without leaking payload data.
- A version/schema mismatch refuses to open a protocol session and tells the
  user to rerun discovery.

## Handoff

The integrator receives `discover_focused_pane()`, `ObserveSession`, and the
later `ControlSession`; no renderer imports serde JSON or starts Herdr commands.
