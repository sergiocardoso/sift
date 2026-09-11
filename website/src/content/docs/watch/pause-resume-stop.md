---
title: Pause, Resume and Stop
description: The rest of the watch lifecycle — pausing, resuming, stopping, and removing.
---

```bash
sift watch pause ~/Inbox
sift watch resume ~/Inbox
sift watch stop ~/Inbox
sift watch remove ~/Inbox
```

## Pause

Stays registered, stops processing events. Any candidate file that was mid-stability-window (not yet organized) is **discarded**, not queued — it isn't retroactively organized on resume either.

## Resume

Only events **from now on** are handled — never a catch-up of what happened while paused. A file that arrived during the pause window is not backfilled; only something happening to it after resume (another write, for instance) would make it a fresh candidate. `resume` also tries the same best-effort tray auto-launch as `start` — see [Auto-launch Behavior](/tray-app/auto-launch-behavior/).

## Stop

Stays registered, stops processing events entirely — the same effective behavior as pause, but framed as the deliberate end of a monitoring session rather than a temporary interruption. Use `start` to resume monitoring later.

## Remove

Unregisters the watch entirely. This only removes the registry entry — it never touches any file the watch may have organized, and is reversible by registering the same folder again with `sift watch add`.

## Confirmed transitions, not fire-and-forget

`pause`/`stop` wait for the daemon to actually confirm teardown before returning — see [Lifecycle Guarantees](/watch/lifecycle-guarantees/).

## Related

- [Register and Start](/watch/register-and-start/) — the other half of the lifecycle.
- [Tray App → Managing Watches](/tray-app/managing-watches/) — the same controls from the desktop tray.
