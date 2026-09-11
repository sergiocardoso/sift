---
title: Recursive Toggle
description: Changing a watch's recursive scope after it's already registered.
---

The **Recursive** checkbox in a watch's submenu toggles `--recursive` scope on a watch that's already registered — something the CLI itself has no dedicated subcommand for yet (`sift watch add --recursive` only ever sets this once, at registration).

## Live effect

Toggling it takes effect on the running daemon's very next reconcile — no need to pause and resume the watch for it to apply.

## Same strategy rules as registration

Enabling recursive scope on a watch reuses the exact same check `sift watch add --recursive` applies: a watch using `audio`, `video`, `photos`, or `documents` still can't be made recursive, for the same reason it's refused at registration — see [Recursive Watch](/watch/recursive-watch/). Disabling recursive scope is always allowed, regardless of strategy.

## Related

- [Managing Watches](/tray-app/managing-watches/) — where this checkbox lives in the menu.
- [Recursive Watch](/watch/recursive-watch/) — the underlying boundaries and strategy restrictions.
