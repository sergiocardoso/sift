---
title: Lifecycle Guarantees
description: What each watch transition confirms before it returns.
---

Watch's CLI commands don't just fire a signal and return — the important transitions wait for the daemon to confirm the change actually happened.

- **`start` / `resume`** only return success after the daemon has confirmed that the root is actually being monitored — not just that a request was sent.
- **`pause` / `stop`** wait for the daemon's teardown acknowledgement before returning.
- **Filesystem events are associated with the watch generation that observed them.** A stale event from before a restart can never be organized as if it were current.
- **Automatic mutation revalidates that the watch is still running and authorized** immediately before it acts — not just at the moment the event first arrived.
- **Files created while paused are not backfilled on resume.** Only events from the moment of resume onward are handled.

## Why this matters

Without these guarantees, a script that runs `sift watch start ~/Inbox` immediately followed by dropping a file into `~/Inbox` could race the daemon — the file might land before monitoring actually began. Because `start` only returns once monitoring is confirmed, that race doesn't exist: if the command succeeded, the watch is genuinely live.

## The single background daemon

All registered watches are served by one background daemon process, not one process per watch.

```bash
sift watch daemon status
sift watch daemon stop
```

`daemon stop` asks the running daemon to shut down safely — every registered watch stays registered, just not actively monitored, until something starts it again.

## Related

- [Register and Start](/watch/register-and-start/) and [Pause, Resume and Stop](/watch/pause-resume-stop/) — the commands these guarantees apply to.
- [Stability Window](/watch/stability-window/) — the debounce that runs *within* an active, confirmed watch.
