---
title: Overview
description: The optional desktop tray UI for managing watched folders.
---

`sift-tray` is a small, separate desktop UI for Linux and macOS that lists watched folders and lets you manage them without a terminal.

It is intentionally thin: it uses the exact same Sift library and Watch registry functions the CLI itself calls (`watch::registry::list`, `watch::cmd_watch_pause`/`cmd_watch_resume`, ...) — it never reimplements watch state transitions, so behavior can never drift between the tray and the CLI. It also never talks to the watch daemon directly; the daemon polls the registry file on its own, so a state change made from the tray is picked up exactly the way `sift watch pause`/`resume` already is from the CLI.

## What it lets you do

- open a watched folder;
- pause or resume it;
- reapply organization immediately;
- toggle recursive mode where supported;
- remove the watch;
- see the current Watch state at a glance.

## A separate binary on purpose

`sift-tray` is a distinct binary from `sift`, so the core CLI never pulls in GUI dependencies (GTK/AppIndicator on Linux). Most installs don't ship it — `install.sh` only installs `sift` today. See [Installation](/tray-app/installation/).

## Pages in this section

- [Installation](/tray-app/installation/) — getting the binary.
- [Managing Watches](/tray-app/managing-watches/) — the menu, folder by folder.
- [Reapply Now](/tray-app/reapply-now/) — one-click organize.
- [Recursive Toggle](/tray-app/recursive-toggle/) — changing scope on an already-registered watch.
- [Auto-launch Behavior](/tray-app/auto-launch-behavior/) — when the tray starts itself, and how to start it explicitly.
