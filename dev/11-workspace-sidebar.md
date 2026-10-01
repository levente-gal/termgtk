# 11: Workspace Sidebar

## Owner Paths

`herdr-native-poc/src/herdr/snapshot.rs` for the list model,
`herdr-native-poc/src/ui/**` for the list widget, and `herdr-native-poc/src/app.rs`
for selection and reconnect. The terminal renderer stays unaware of workspaces.

## Dependencies

Phase 10 gate. Installed Herdr 0.8.0 CLI, confirmed on this machine:

```text
herdr workspace focus <workspace_id>
```

That command is the public wrapper over socket method `workspace.focus`
(`WorkspaceTarget.workspace_id`). Do not open a socket from this process.
`herdr api` only implements `snapshot` and `schema`.

Snapshot paths already in `docs/contracts/snapshot.json` and
`stream-contract.md`:

- `.result.snapshot.workspaces[]`: `workspace_id`, `label`, `number`, `focused`,
  `active_tab_id`, `pane_count`, `tab_count`
- `.result.snapshot.focused_workspace_id`, `focused_tab_id`, `focused_pane_id`

## Work

1. Parse `workspaces[]` from the same snapshot envelope as `FocusedPane`. Sort
   by `number`. A missing `label` displays `workspace_id`. Do not put cwd,
   agent status, or terminal titles in the list.
2. Add a GTK list to the left of the terminal card. Mark the row whose
   `workspace_id` equals `focused_workspace_id`. Refresh by running
   `herdr api snapshot` again on a slow timer and after a focus command.
   Give every snapshot request a monotonic generation; apply a result only if
   its generation is still newest. There is no event subscription in this phase.
3. Activate a row with argv `herdr`, `workspace`, `focus`, `<workspace_id>`
   and no shell. On success, read the snapshot again and require
   `focused_workspace_id` to equal the requested id before reconnecting to
   `focused_pane_id`; otherwise keep the current stream and show a focus-race
   error. On command failure, keep the current stream and show the command
   error. Do not log snapshot payloads.
4. If this process holds the controller, run the existing `terminal.release`
   path before the focus command, then attach the new pane as read-only.
   Takeover does not follow the user to the next workspace.
5. Update the header from the new `FocusedPane` label. A workspace with no
   focused pane after the command is a disconnect, not an empty grid invention.
6. Tests on the redacted fixture: five workspaces, order by `number`, focused
   id `w9`. Focus argv is exactly those four arguments. A failed focus, a
   mismatched refreshed workspace id, and an older timer snapshot leave
   `pane_id` unchanged.

## Invariant

```text
sidebar selection == snapshot focused_workspace_id
attached pane == snapshot focused_pane_id
at most one controller, and only after an explicit takeover on that pane
```

## Gate Demo

- The list shows the live workspaces, with the current one marked.
- Choosing another workspace focuses it in Herdr and the card follows that
  workspace's focused pane.
- Closing the window while controlling still releases before exit.

## Out of Scope

Creating, renaming, or closing workspaces. A tab strip. Multi-pane layout.
Socket clients, remote Herdr, and SSH.
