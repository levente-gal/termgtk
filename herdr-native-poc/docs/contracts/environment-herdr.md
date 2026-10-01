# environment-herdr.md

Captured 2026-09-28 from the installed `herdr` binary. Research only; no `control`, no `--takeover`, server left running.

## Version

- `herdr --version` → `herdr 0.8.0`
- Live `herdr status server` → `status: running`, `version: 0.8.0`, `protocol: 19`, `compatible: yes`
- Live `herdr status server --json` → `"version":"0.8.0"`, `"protocol":19`, `"compatible":true`
- Live `herdr api snapshot` → `.result.snapshot.version` = `"0.8.0"`, `.result.snapshot.protocol` = `19`

## Schema

- Command: `herdr api schema --json`
- Saved as: `herdr-api.schema.json`
- SHA-256: `88ff414aa996e390c2db05a37b95d28dbe4e81b98329f6ed7f7a2cc5c6ebf51a`
- Schema header: `"protocol": 19`, `"schema_version": 1`, `"title": "Herdr API"`
- Control NDJSON (`terminal.input`, `terminal.resize`, `terminal.release`) is not in that schema. Shapes in `stream-contract.md` were matched to this binary's `TerminalControlCommand` strings and were not sent live.

## Server reachability

- **Reachable:** yes (default socket `/home/REDACTED/.config/herdr/herdr.sock`)
- Simulated unavailable via `HERDR_SOCKET_PATH=/tmp/herdr-does-not-exist-$$.sock` (real server not stopped)

### Success (server up)

| Command | Exit | stderr |
| --- | --- | --- |
| `herdr status server` | `0` | empty; stdout `status: running` … |
| `herdr api schema --json` | `0` | empty |
| `herdr api snapshot` | `0` | empty |
| `herdr terminal session observe <pane> --cols 120 --rows 40` (SIGINT/SIGTERM via `timeout`) | `124` (from `timeout`) | empty; stdout has NDJSON frames |

### Unavailable server (`HERDR_SOCKET_PATH` override)

| Command | Exit | Output |
| --- | --- | --- |
| `herdr status server` | `0` | stdout: `status: not running` / `socket: <override>` ; stderr empty |
| `herdr api snapshot` | `1` | stderr JSON: `{"id":"cli:api:snapshot","error":{"code":"server_not_running","message":"no herdr server is running at <socket>; run \`herdr\` to start or attach it"}}` |
| `herdr terminal session observe …` | `1` | stderr text: `herdr: failed to connect to server: No such file or directory (os error 2)` / `Is herdr server running? Start it with \`herdr server\`.` / `Socket path: <override>-client.sock` |

### Closed / missing pane (server up)

| Command | Exit | Output |
| --- | --- | --- |
| `herdr terminal session observe w999:p999 --cols 120 --rows 40` | `0` | stdout NDJSON: `{"reason":"terminal session observe failed: terminal target w999:p999 not found","type":"terminal.closed"}` ; stderr empty |
| `herdr terminal session observe term_deadbeefdeadbe --cols 120 --rows 40` | `0` | same shape with that target in `reason` |

Skill note (from `herdr --skill`): CLI server errors are JSON on stderr with exit status `1`; CLI syntax errors exit with status `2`.

## Exact CLI argv shapes verified

```text
herdr --version
herdr --help
herdr status server
herdr status server --json
herdr api --help
herdr api schema --json
herdr api schema --help
herdr api snapshot
herdr api snapshot --help
herdr terminal --help
herdr terminal session --help
herdr terminal session observe --help
herdr terminal session observe <TARGET> --cols <N> --rows <N>
herdr terminal session control --help
# control argv shape (NOT executed): herdr terminal session control <TARGET> [--takeover] [--cols N] [--rows N]
herdr terminal attach --help
# attach argv shape (NOT executed with --takeover): herdr terminal attach <TERMINAL_ID> [--takeover]
```

Observe used live: `herdr terminal session observe w9:p2 --cols 120 --rows 40` (and `--cols 80 --rows 24` for dimension check).

## Blocker / schema gap

`herdr-api.schema.json` (socket API) does **not** define NDJSON stream types `terminal.frame` / `terminal.closed`, nor control stdin requests `terminal.input` / `terminal.resize` / `terminal.release`. Those appear only on the `herdr terminal session observe|control` process streams. Exact JSON Schema for the three control requests is therefore unavailable from `herdr api schema --json` without running `control`/`--takeover`.
