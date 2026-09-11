---
title: Managing Watches
description: The tray menu, folder by folder.
---

The tray icon's menu is rebuilt from the Watch registry every few seconds and immediately after any action, so it never shows stale state for long.

## Top level

- **Add folder…** — opens a native folder picker, registers the chosen folder as a watch, and starts it. Picking a folder here is a deliberate authorization gesture: unlike `sift watch add` on the CLI (which leaves pre-existing files untouched until a separate explicit `organize --apply`), the tray runs one real organize pass on the folder's existing contents *before* starting the watch — so a folder full of existing files doesn't sit unorganized until something new happens to land in it.
- One entry per registered watch, with a status indicator (running, paused, stopped, or a config error) and the folder name.

## Per-watch submenu

- **Open folder** — opens it in your OS's file manager.
- **Reapply now** — runs one `organize --apply` pass immediately. See [Reapply Now](/tray-app/reapply-now/).
- **Pause / Resume** — toggles the watch, identical to `sift watch pause`/`resume`.
- **Recursive** — a checkbox reflecting and toggling `--recursive` scope live. See [Recursive Toggle](/tray-app/recursive-toggle/).
- **Remove** — unregisters the watch (same as `sift watch remove`). Separated below its own divider so an accidental click is less likely.

## Bottom of the menu

- **About** — the OS's native About dialog, with Sift's version and a credit to its author.
- **Quit** — exits the tray app. Registered watches stay registered and keep running under the watch daemon; only the tray UI itself stops.

## Related

- [Watch → Pause, Resume and Stop](/watch/pause-resume-stop/) — the same actions from the CLI.
- [Overview](/tray-app/overview/) — why the tray never reimplements watch logic of its own.
