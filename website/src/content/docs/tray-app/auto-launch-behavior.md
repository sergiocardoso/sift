---
title: Auto-launch Behavior
description: When the tray starts itself automatically, and how to launch it explicitly.
---

## Automatic, silent, best-effort

When `sift-tray` is installed next to `sift` or available on `PATH`, `sift watch start` and `sift watch resume` try to launch it automatically.

This is deliberately silent and best-effort:

- Watch still succeeds if the tray isn't installed;
- a headless environment (no display) doesn't cause a failure;
- only one tray instance ever runs at a time, via a singleton lock.

Every failure mode here — not installed, no display server, already running, spawn error — is swallowed on purpose. `watch start`/`resume` must never fail, warn, or block on whether a tray icon could be shown.

## Explicit launch

```bash
sift watch tray
```

Launches `sift-tray` on its own, without starting or resuming a watch — and unlike the automatic path above, this reports exactly what happened:

- `Started sift-tray.` — spawned, and confirmed it acquired its singleton lock.
- `sift-tray is already running.` — nothing was spawned.
- an error if `sift-tray` isn't installed, with instructions.
- an error if it was spawned but never came up within a couple of seconds — worth checking its log for details in that case.

## Related

- [Installation](/tray-app/installation/) — getting the `sift-tray` binary in the first place.
- [Watch → Register and Start](/watch/register-and-start/) — where the automatic launch is triggered from.
