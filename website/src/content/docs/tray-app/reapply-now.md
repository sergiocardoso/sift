---
title: Reapply Now
description: Running one organize pass on a watched folder on demand.
---

**Reapply now**, in a watched folder's submenu, runs one `sift organize --apply` pass on that folder immediately — the same manual "reapply" you'd otherwise need a terminal for.

It uses whichever `--recursive` scope is currently registered for that watch, exactly as the running daemon itself would apply it — so the result is identical to what would eventually happen through the watch's own event-driven organizing, just triggered immediately instead of waiting for a filesystem event.

## When this is useful

- You edited the folder's `.sift.toml` and want the change applied to files that are already there, without waiting for something new to arrive.
- Files landed in the folder before the watch was started (Watch never backfills pre-existing files on its own — see [Watch → Overview](/watch/overview/)).
- You just want to confirm the current policy against what's actually in the folder right now.

## Related

- [Recursive Toggle](/tray-app/recursive-toggle/) — the scope Reapply Now uses.
- [Organize Files](/organizing/organize-files/) — the same `organize --apply` from the CLI.
