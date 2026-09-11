---
title: CLI Commands
description: Full command and flag reference.
---

## Top level

| Command | Purpose | Mutates? |
|---|---|---:|
| `sift [path]` | Safe organization preview (shorthand for `organize`, no `--apply`) | No |
| `sift scan [path]` | Inspect directory entries | No |
| `sift organize [path]` | Build (and optionally apply) an organization plan | Only with `--apply` |
| `sift folders [path]` | Classify and move immediate child folders by contents | Only when explicitly applied |
| `sift clean [path]` | Build (and optionally apply) a cleanup plan | Only with `--apply` |
| `sift doctor [path]` | Report-only filesystem findings | No |
| `sift explain <file>` | Explain one organization decision | No |
| `sift config check [path]` | Validate the effective configuration | No |
| `sift history` | Show recorded operations | No |
| `sift undo <id>` | Reverse a successful recorded move | Yes |
| `sift init [path]` | Create a starter `.sift.toml` | Yes |
| `sift watch ...` | Manage continuous organization | Explicit authorization required |

`path` defaults to `.` everywhere it's optional.

## `scan`

```bash
sift scan [path] [--json] [--recursive]
```

Read-only; never mutates.

## `organize`

```bash
sift organize [path] [--apply] [--json] [--verbose] [--recursive]
```

- `--apply` — actually perform the moves (default is dry-run).
- `--verbose` — list every skipped entry individually instead of collapsing large lists.
- `--recursive` — organize every eligible subdirectory in place, each as its own local context. Refused for the `audio`/`video`/`photos`/`documents` strategies.

## `folders`

```bash
sift folders [path] [--apply] [--json] [--remove-duplicates]
```

- `--remove-duplicates` — also compare files inside name-based possible-duplicate folder groups and trash exact content matches, keeping the alphabetically-first folder's copy. No `--recursive` flag exists — candidate selection is always exactly one level of immediate children.

## `clean`

```bash
sift clean [path] [--apply] [--json] [--verbose]
```

No `--recursive` — see [Clean](/organizing/clean/) for why.

## `doctor`

```bash
sift doctor [path] [--json] [--recursive]
```

Read-only regardless of flags.

## `explain`

```bash
sift explain <file> [--root <dir>] [--json]
```

- `--root` — the policy root whose `.sift.toml` (or global/default) applies. Defaults to the file's own parent directory.

## `config check`

```bash
sift config check [path] [--json]
```

## `history` / `undo` / `init`

```bash
sift history
sift undo <operation-id>
sift init [path] [--force]
```

`--force` on `init` overwrites an existing `.sift.toml`.

## `watch`

```bash
sift watch add <path> --auto-apply [--recursive]
sift watch list [--json]
sift watch status [path] [--json]
sift watch start <path>
sift watch pause <path>
sift watch resume <path>
sift watch stop <path>
sift watch remove <path>
sift watch tray
sift watch daemon status
sift watch daemon stop
```

`--auto-apply` on `add` is required — there's no way to register an auto-organizing watch without it. `status` with no path shows every watch plus daemon status. See [Watch](/watch/overview/) and [Tray App](/tray-app/overview/) for the full behavior behind each of these.

## Global flags

- `--json` — machine-readable output instead of a table, available on `scan`, `organize`, `folders`, `clean`, `doctor`, `explain`, `config check`, `watch list`, and `watch status`. See [JSON and Scripting](/organizing/json-and-scripting/).
- `--recursive` — available on `scan`, `organize`, `doctor`, and `watch add` (not `clean`, not `folders`).

## Update notices

At most once every 24 hours, any command may spawn a fully detached background check for a newer GitHub release — it never blocks or slows down the command that triggered it, and a notice from that check only ever shows up starting with the *next* invocation, always on stderr (never stdout, so it can't corrupt `--json` output). Set `SIFT_NO_UPDATE_CHECK=1` to disable it entirely; without it, core Sift operations make no network calls at all.
