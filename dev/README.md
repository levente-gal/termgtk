# Parallel Implementation Plan

## Target Layout

Create the application in `herdr-native-poc/`, alongside the existing `doc/` and this `dev/` directory.

## Architecture Boundaries

```text
Herdr CLI / NDJSON -> herdr/** -> TerminalEvent -> terminal/** -> TerminalSnapshot
                                                          -> render/** -> GTK4
```

- `herdr/**` is the only layer that knows Herdr command lines and NDJSON.
- `terminal/ghostty.rs` is the only layer that calls Ghostty C APIs.
- `terminal/model.rs` defines the renderer-neutral cell snapshot.
- `render/**` consumes only `TerminalSnapshot`; its semantic pass is optional.
- `app.rs` composes layers and owns application lifecycle, not protocol parsing.

## Canonical Contracts

Discovery is complete only when these committed, redacted artifacts exist under
`herdr-native-poc/docs/contracts/`:

- `environment.md`: Herdr, Ghostty, Zig, Rust, Cargo, GTK, and libadwaita versions.
- `herdr-api.schema.json`: the exact installed schema, plus its SHA-256 in `environment.md`.
- `snapshot.json` and `observe.ndjson`: a redacted focused-pane snapshot and an ordered stream fixture.
- `stream-contract.md`: frame initialization, ordering, sequence/reset, reconnect, resize, close, stderr, and exit-status behavior observed from the installed Herdr version.
- `ghostty.md`: pinned commit, license/source provenance, C signatures, build inputs, and the supported fidelity/input-encoding subset.
- `gtk.md`: selected Rust bindings, system packages, supported scale factors, and the tested widget/input APIs.

These artifacts are the only source of protocol and FFI truth. `dev/README.md`
is the execution index; the POC document is the product specification.

## Execution Graph

```text
00 discovery (three agents)
        |
01 foundation
   |       |       |       |
02 herdr  03 VT    04 render  06 semantic
   |       |       |       |
   +-------+-------+-------+
               |
        05 native shell
               |
        07 read-only integration
               |
        08 interactive control and resize
               |
        09 validation and documentation
               |
        10 fit the pane to the card
               |
        11 workspace sidebar
               |
        12 focused-tab layout, then smooth scrolling
```

## Agent Rules

- Start only the tasks whose prerequisites are complete.
- Each agent modifies only the paths named in its work package. It must request an interface change instead of editing another agent's area.
- Agents must rebase their implementation on the foundation's published interfaces; they must not introduce parallel terminal models.
- Every agent supplies unit tests or a concrete manual verification procedure before handoff.
- The integration owner merges in this order: model, Herdr stream, Ghostty engine, raw renderer, native shell, semantic overlay, interaction.
- Do not launch control/takeover code until the read-only live view works.

## Required Gates

1. Every canonical discovery artifact above is captured and reviewed. No code may invent JSON fields, frame semantics, or FFI signatures.
2. Ghostty is pinned to a commit and its build/FFI route is verified.
3. The project compiles from the pinned sources and renders a fixed `TerminalSnapshot` before live integration.
4. A live read-only focused pane renders, reconnects safely, and remains bounded under frame load before takeover is added.
5. Controller takeover, resize acknowledgement, and best-effort release are logged and exercised on every orderly shutdown path.
6. Enhancement-off always produces raw cell rendering; detection must not remove unrecognized or ambiguously recognized content.

## Suggested Parallel Dispatches

| Wave | Agents | Inputs | Completion condition |
| --- | --- | --- | --- |
| Discovery | Herdr protocol, Ghostty FFI, GTK feasibility | installed tools, POC | canonical artifacts committed under `docs/contracts/` |
| Foundation | bootstrap/model owner | discovery contracts | clean compile plus model fixtures |
| Core | Herdr, Ghostty, renderer, semantic | foundation interfaces | independently tested modules |
| Composition | native shell, read-only integrator | core modules | live raw pane inside graphical card |
| Control | interaction owner | read-only integration | keyboard, resize, release work |
| Closeout | QA, documentation | complete app | acceptance matrix and README complete |
| Fit | interaction owner | phase 09 | whole pane visible; live resize recorded |
| Sidebar | Herdr + shell | phase 10 | workspace list focuses and reconnects |
| Layout | Herdr + shell + renderer | phase 11 | focused tab's panes; scroll only if Ghostty allows |

Read each numbered file before dispatching its agent. Phases 10–12 start only after the previous gate.
