# 07: Read-Only Vertical Slice

## Owner Paths

Composition wiring in `herdr-native-poc/src/app.rs` and
`herdr-native-poc/src/main.rs` only, coordinated with owners if their public
APIs need correction.

## Dependencies

Herdr backend, Ghostty engine, raw renderer, and native shell are complete.

## Work

1. At startup, check the Herdr server and discover the focused pane via the
   public CLI/API. Revalidate the pane immediately before opening the session.
2. Construct a terminal engine only after applying the discovered stream
   initialization contract, using view dimensions or a documented `120x40`
   bootstrap size.
3. Start `observe` only. One worker serially feeds frame bytes into the engine
   and publishes bounded, generation-tagged latest snapshots to GTK's main
   context; GTK never mutates the engine.
4. Display focused workspace/pane/title metadata when available.
5. Increment frame count for debug mode.
6. Translate unavailable server, changed/disappeared pane, malformed stream,
   reconnect/resync failure, terminal close, and engine failure to distinct
   visible/logged states. Never log terminal payloads.
7. Confirm raw/enhanced toggle works on the exact same live snapshot without reconnecting.

## Gate Demo

With an existing Herdr pane running a shell or structured TUI:

- the GTK window displays current terminal contents;
- live changes appear;
- ANSI colours, normal Unicode, cursor, and raw box characters appear;
- the terminal has a true graphical outer card;
- no command in this mode changes controller ownership.
- reconnecting cannot display a stale snapshot or unboundedly queue GTK work.

Do not begin interaction work until this demo succeeds.
