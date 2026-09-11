---
title: Register and Start
description: Registering a folder as a watch, and starting monitoring.
---

## Register

```bash
sift watch add ~/Inbox --auto-apply
```

- `--auto-apply` is **required** — explicit, persistent authorization for this watch to mutate the folder automatically once it's running. There's no way to register a watch that auto-organizes without it.
- `--recursive` organizes eligible subdirectories in place too, with the same boundaries as `organize --recursive` (see [Recursive Watch](/watch/recursive-watch/)). Omit it for a non-recursive watch.

A newly registered watch starts in the `stopped` state — registering never starts monitoring by itself, and never touches pre-existing files.

```text
add
 ↓
stopped
 ↓ start
running
 ↕
pause / resume
 ↓ stop
stopped
```

## Start

```bash
sift watch start ~/Inbox
```

Starts monitoring. It does **not** back-fill files that already existed in the folder before this — Watch only reacts to filesystem events from the moment it starts. If you want the existing contents organized first:

```bash
sift organize ~/Inbox --apply
sift watch start ~/Inbox
```

`start` also tries, best-effort and silently, to launch the optional [tray app](/tray-app/overview/) if it's installed and not already running — see [Auto-launch Behavior](/tray-app/auto-launch-behavior/).

`start` only returns success once the daemon has actually confirmed it's monitoring the root — see [Lifecycle Guarantees](/watch/lifecycle-guarantees/).

## Listing and inspecting

```bash
sift watch list
sift watch status ~/Inbox
```

`status` with no path shows every watch plus the daemon's own status.

## Related

- [Pause, Resume and Stop](/watch/pause-resume-stop/) — the rest of the lifecycle, including removal.
- [Stability Window](/watch/stability-window/) — how long a new file sits before Watch acts on it.
