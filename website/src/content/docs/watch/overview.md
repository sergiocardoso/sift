---
title: Overview
description: Turning selected folders into continuously organized inboxes.
---

Sift Watch turns selected folders into continuously organized inboxes.

```text
invoice.pdf appears
        ↓
wait until stable
        ↓
evaluate policy
        ↓
Documents/invoice.pdf
```

## Explicit, persistent authorization

Automatic mutation requires an explicit, persistent flag when the watch is registered:

```bash
sift watch add ~/Inbox --auto-apply
```

There's no way to have Watch mutate a folder without having passed `--auto-apply` at registration time.

## Pre-existing files are never backfilled

Starting a watch never touches files that already existed before it started — Watch only ever reacts to filesystem events from that point forward.

```bash
sift organize ~/Inbox --apply   # organize what's already there, once
sift watch start ~/Inbox        # then watch for new arrivals
```

## Live config

A running watch resolves its `.sift.toml` the same way manual `organize` does, and hot-reloads it — see [Configuration Lookup](/configuration/configuration-lookup/) and [Lifecycle Guarantees](/watch/lifecycle-guarantees/) for exactly what happens when the file changes while the watch is running.

## Pages in this section

- [Register and Start](/watch/register-and-start/) — the `add` → `stopped` → `start` → `running` flow.
- [Pause, Resume and Stop](/watch/pause-resume-stop/) — the rest of the lifecycle.
- [Stability Window](/watch/stability-window/) — why Watch waits before organizing a new file.
- [Recursive Watch](/watch/recursive-watch/) — the same boundaries as recursive organize, applied continuously.
- [Lifecycle Guarantees](/watch/lifecycle-guarantees/) — what each transition confirms before returning.

## The desktop companion

The optional [Tray App](/tray-app/overview/) manages watched folders visually, without a terminal.
