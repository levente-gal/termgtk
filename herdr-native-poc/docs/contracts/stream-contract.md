# stream-contract.md

Herdr `0.8.0` / protocol `19`. Facts from live `observe` and `herdr api schema --json`. No `control`, no `--takeover`.

## Commands

```text
herdr terminal session observe <TARGET> --cols <N> --rows <N>
# NOT run: herdr terminal session control <TARGET> [--takeover] [--cols N] [--rows N]
```

`<TARGET>` accepted as pane id (`w9:p2`) or terminal id (`term_…`) in failure tests.

Stdout: NDJSON (one JSON object per line). Stderr: empty on successful observe frames and on missing-target `terminal.closed`.

## Snapshot paths (focused)

Envelope from `herdr api snapshot`:

- `.id` (observed: `"cli:api:snapshot"`)
- `.result.type` = `"session_snapshot"`
- `.result.snapshot`

Focused IDs:

- `.result.snapshot.focused_workspace_id`
- `.result.snapshot.focused_tab_id`
- `.result.snapshot.focused_pane_id`

Labels / title (live focused session):

- Workspace label: `.result.snapshot.workspaces[].label` (match `workspace_id == focused_workspace_id`)
- Tab label: `.result.snapshot.tabs[].label` (match `tab_id == focused_tab_id`)
- Terminal title: `.result.snapshot.panes[].terminal_title` and `.terminal_title_stripped` (match `pane_id == focused_pane_id`)
- Also on agents: `.result.snapshot.agents[].terminal_title` / `.terminal_title_stripped` when `agents[].focused == true` or `pane_id` matches

Related pane fields observed on the focused pane object:

- `.pane_id`, `.tab_id`, `.workspace_id`, `.terminal_id`, `.focused`, `.cwd`, `.foreground_cwd`, `.agent`, `.agent_status`, `.revision`, `.scroll`

**Note:** schema `PaneInfo` allows optional `label` / `title` / `state_labels`, but the live focused pane object did **not** include `label` or `title` keys.

See redacted `snapshot.json`.

## NDJSON event shapes (observe stdout)

### `terminal.frame`

Discriminator field: `type` = `"terminal.frame"`.

Observed fields (exact names):

| Field | Observed values |
| --- | --- |
| `type` | `"terminal.frame"` |
| `seq` | integer; first frame of a new observe process was `1` |
| `full` | boolean; first frame was `true` |
| `encoding` | `"ansi"` |
| `width` | integer; matches `--cols` |
| `height` | integer; matches `--rows` |
| `bytes` | string; **base64** ANSI payload |

**Base64 payload path:** `.bytes`

No other keys were present on observed frames.

### `terminal.closed`

Discriminator field: `type` = `"terminal.closed"`.

Observed fields:

| Field | Observed values |
| --- | --- |
| `type` | `"terminal.closed"` |
| `reason` | string, e.g. `"terminal session observe failed: terminal target w999:p999 not found"` |

Exit status for that case: `0`. Stderr empty.

## Frame init vs incremental

- First record after starting observe on a live pane: `terminal.frame` with `full: true`, `seq: 1`, and a large `bytes` payload (complete viewport ANSI for the requested `width`×`height`).
- Idle focused pane observed for up to **8s** produced **only** that initial full frame (no further lines).
- Therefore: **first frame initializes complete terminal state** (`full: true`). Live **incremental** (`full: false`) frames were **not** observed without injecting pane output. Fixture `fixtures/incremental-update.ndjson` uses the same field set with synthetic `full: false` / `seq: 2` for decoder tests only.
- Records have **`seq`** (sequence number). No separate reset marker field and no acknowledgement records were seen on observe stdout.
- Reconnect: a second independent `observe` process again emitted `seq: 1`, `full: true` (sequence does not continue across processes). Payload dimensions follow that process’s `--cols`/`--rows`.

## Observe-only behaviors

| Situation | Observed / status |
| --- | --- |
| Pane focus change while observing | **Not exercised** (would require focus mutation). Schema events include `pane_focused` / `tab_focused` / `workspace_focused` on the socket API event bus; observe NDJSON showed only `terminal.frame` / `terminal.closed`. |
| Pane / target disappears or missing | stdout `terminal.closed` + `reason`; process exit `0` |
| Observer exits (SIGINT/SIGTERM via `timeout`) | exit `124` from `timeout`; stderr empty; no `terminal.closed` required before death |
| Controller disconnect | **Not exercised** (requires `control` / `--takeover`) |
| Resize accepted/rejected on control stdin | **Not exercised**. Observe-only: starting observe with different `--cols`/`--rows` changes `width`/`height` on the initial `terminal.frame` only for that stream (does not prove PTY resize). |
| Input rejected | **Not exercised** (requires control stdin) |

## Control stdin request schemas (`terminal.input` / `terminal.resize` / `terminal.release`)

**Absent from installed `herdr api schema --json`.** Socket methods `pane.send_input`, `pane.resize`, and `pane.release_agent` are a different API and are not this stream.

The installed binary is an unstripped Herdr 0.8.0 build. Its strings match the upstream `TerminalControlCommand` deserializer (`#[serde(tag = "type")]`) field-for-field:

- variant identifiers `terminal.input`, `terminal.resize`, `terminal.scroll`, `terminal.release`
- `Input` has 2 elements; `Resize` has 4; `Scroll` has 6; `Release` has 0
- exact errors: `terminal.input accepts text or bytes, not both`, `invalid terminal.input bytes: `, `terminal.resize cols and rows must be greater than 0`, `terminal.scroll lines must be greater than 0`, `invalid json command: `

These requests were **not** sent to a live pane during discovery (no `control`, no `--takeover`). Treat the shapes below as the contract. Wheel and Page Up/Down send `terminal.scroll` on a control session (`direction` `up` or `down`, `lines` > 0, `source` `wheel` or `page_key`).

```json
{"type":"terminal.input","text":"hello"}
{"type":"terminal.input","bytes":"G1tB"}
{"type":"terminal.resize","cols":100,"rows":30,"cell_width_px":8,"cell_height_px":16}
{"type":"terminal.scroll","direction":"up","lines":3,"source":"wheel","column":12,"row":4}
{"type":"terminal.release"}
```

| Command | Fields | Rules |
| --- | --- | --- |
| `terminal.input` | `text` and/or `bytes`, both optional strings | `text` is raw input; `bytes` is standard base64. Both set is rejected. Neither yields empty input. |
| `terminal.resize` | `cols` u16, `rows` u16, optional `cell_width_px` u32, optional `cell_height_px` u32 | `cols` and `rows` must be `> 0`. Pixel fields default to `0` when omitted. |
| `terminal.release` | none | stdin reader sends detach and stops. Stdin EOF also sends detach. |

Stdout still emits only `terminal.frame` and `terminal.closed`. There is no acknowledgement record. A rejected line is ignored and printed on stderr as `herdr: terminal session control input ignored: …`; the process keeps running. A successful resize is visible only as later `terminal.frame` `width`/`height` values. See `fixtures/control-requests.ndjson`.

## Controller release and disconnect

`terminal.release` makes the stdin reader detach and stop. Stdin EOF also detaches. Neither path emits an acknowledgement record. A rejected resize or input stays on stderr and does not change the last confirmed `width`/`height`.

This POC sends `terminal.release` on window close, stream failure, and worker shutdown, then waits briefly for the child to exit. The release error is logged on its own and does not replace the original stream error.

Forced termination of this process (kill before the release line is flushed) cannot guarantee detach. Herdr only detaches when it reads `terminal.release` or sees EOF on control stdin. Dropping the child with `SIGKILL` skips that EOF. Controller disconnect itself was not captured live during discovery.

## Exit / stderr samples (bounded, no payloads)

Success observe (killed by `timeout` after frames):

```text
exit: 124
stderr: (empty)
stdout: NDJSON terminal.frame lines (payloads omitted from logs)
```

Unavailable server (`HERDR_SOCKET_PATH` fake socket):

```text
exit: 1
stderr:
herdr: failed to connect to server: No such file or directory (os error 2)
Is herdr server running? Start it with `herdr server`.
Socket path: <override>-client.sock
```

Closed / missing pane:

```text
exit: 0
stderr: (empty)
stdout: {"reason":"terminal session observe failed: terminal target w999:p999 not found","type":"terminal.closed"}
```

## Fixture redaction note

Fixtures under `fixtures/` and committed `observe.ndjson` / `snapshot.json` must **not** contain:

- Local username from `whoami` (this capture: `root`) or account name from `$HOME` (`lgal`)
- Home path prefix `/home/lgal` (use `/home/REDACTED`)
- Hostname if embedded in titles (`fedora` appeared as hostname; strip from titles)
- Unredacted terminal titles (live examples replaced with `REDACTED`)
- Real ANSI/base64 terminal payloads (use fake base64 of public `\x1b[31mred\x1b[0m` → `G1szMW1yZWQbWzBt`, or empty `bytes`)

Do not paste observe `bytes` into logs, issues, or docs.
