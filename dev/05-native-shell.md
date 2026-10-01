# 05: Native Application Shell

## Owner Paths

`herdr-native-poc/src/app.rs`, `herdr-native-poc/src/ui/**`, and
presentation-only assets.

## Dependencies

The renderer can be passed as a widget; no live terminal is required.

## Work

1. Create a GTK/libadwaita application window with title/header context for Herdr and the discovered pane/workspace label.
2. Create a genuine graphical outer background and terminal card. Centralize approximate POC values in `ui/theme.rs`:
   - background `#111318`
   - card `#171a20`
   - low-opacity border
   - 8px radius
   - 4px/18px shadow
   - 8-12px padding
3. Use GTK/GSK/CSS graphical facilities, never Unicode border glyphs for the card.
4. Add an `Enhance TUI rendering` toggle and application action with Ctrl+Shift+E. It changes renderer state immediately without reconnecting.
5. Add `HERDR_NATIVE_DEBUG=1` overlay hooks: pane ID, confirmed rows/columns,
   cell metrics, scale factor, frame count, detection counts, and optional
   highlight state. Never display terminal payloads in this overlay.
6. Provide unavailable, changed-pane, disconnected, resyncing, and controller
   error presentation states supplied by the integrator.

## Verification

- Fixture terminal is surrounded by rounded graphical card/border/shadow.
- Toggle can be changed by mouse and shortcut.
- Debug overlay is absent unless the environment variable is set.
- Window is usable at a small mobile-like size and normal desktop size.
- Card clipping, borders, and one-physical-pixel overlays remain correct at
  supported scale factors.
