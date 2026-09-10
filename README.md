# Sift

<p align="center">
  <img
    src="https://supabase.flokin.com.br/storage/v1/object/public/projects/sift/fefbec4b-ddda-425f-a9f1-8581b4a8f19b.png"
    alt="Sift"
    width="220"
  />
</p>

<p align="center">
  <strong>Safe, local-first filesystem organization.</strong>
</p>

<p align="center">
  Organize, clean, watch, and understand your folders without giving up control.
</p>

**A local-first CLI for organizing messy folders safely, predictably, and on your terms.**

![License: Apache 2.0](https://img.shields.io/badge/license-Apache%202.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-stable-orange.svg)
![Version](https://img.shields.io/badge/version-0.1.0-lightgrey.svg)

Sift helps turn folders such as `Downloads`, `Desktop`, shared drop folders, 3D asset folders, and temporary workspaces into something you can understand again.

It does that without depending on cloud services or AI, and without silently moving files behind your back.

```text
messy folder
    ↓
scan
    ↓
classify
    ↓
plan
    ↓
review
    ↓
--apply
    ↓
organized filesystem + history
```

The default behavior is deliberately conservative:

```text
no --apply
→ no filesystem mutation
```

---

## Why Sift?

Folders accumulate entropy.

A download directory starts with a few PDFs and screenshots and eventually becomes a mix of documents, archives, videos, source files, exports, datasets, 3D models, temporary files, and forgotten project folders.

The usual solutions have trade-offs:

- manual organization does not scale;
- one-off shell scripts are easy to forget and risky to reuse;
- automation can become dangerous when it overwrites or deletes files;
- AI-based organizers can be difficult to predict, audit, or run entirely locally.

Sift takes a different approach.

### Predictable

Classification is deterministic and based on filenames, extensions, metadata, and explicit rules.

The same input produces the same plan.

### Safe by default

Sift previews changes first. Mutating commands require explicit authorization with `--apply`.

### Local-first

Sift works on your filesystem. It does not need to upload filenames, file contents, or metadata to an external service to perform its core job.

### Explainable

A Sift plan tells you what will happen before it happens:

```text
invoice.pdf     → Documents/
photo.jpg       → Images/
model.stl       → 3D/
data.json       → Data/
```

### Conservative around important data

Software projects, hidden entries, symlinks, collisions, and protected traversal boundaries are intentionally left alone.

### Auditable

Executed operations are recorded in history.

Successful file moves can be reversed with `sift undo` when the live filesystem still makes the undo safe.

### Automatable

When you are ready, **Sift Watch** can turn a folder into a continuously organized inbox while still using the same safety rules as manual organization.

---

## What Sift is — and what it is not

Sift is a filesystem organizer and maintenance CLI.

It is designed to:

- inspect directories;
- organize loose files by type;
- organize eligible nested folders in place with recursive mode;
- identify high-confidence junk;
- diagnose potentially interesting filesystem entries;
- apply custom local rules;
- keep operation history;
- undo successful moves when safe;
- continuously organize new files with Watch.

Sift is **not**:

- a cloud storage service;
- a file sync tool;
- a content-indexing search engine;
- an AI classifier;
- a duplicate-file remover;
- a recursive deletion tool;
- a tool that silently overwrites existing destinations.

---

## A 30-second example

Imagine this directory:

```text
Downloads/
├── invoice.pdf
├── vacation.jpg
├── backup.zip
├── data.json
├── model.stl
├── experiment.rs
├── notes.xyz
└── my-project/
    ├── Cargo.toml
    └── src/
```

Preview the plan:

```bash
sift organize ~/Downloads
```

Sift can plan something like:

```text
invoice.pdf      → Documents/
vacation.jpg     → Images/
backup.zip       → Archives/
data.json         → Data/
model.stl         → 3D/
experiment.rs     → Code/
notes.xyz         → Other/
my-project/       → protected: software project
```

Nothing has changed yet.

Apply only after reviewing:

```bash
sift organize ~/Downloads --apply
```

Then inspect the operation history:

```bash
sift history
```

And, if needed, reverse successful moves:

```bash
sift undo hist-<operation-id>
```

---

# Installation

Sift is written in Rust.

## Install script

```bash
curl -fsSL https://raw.githubusercontent.com/sergiocardoso/sift/main/install.sh | sh
```

Downloads the latest prebuilt release for your OS/architecture (Linux and macOS, x86_64/aarch64), verifies its SHA256 checksum, and installs it to `~/.local/bin/sift` — never with `sudo`. It also checks whether `ffprobe` is on your `PATH`; if not, it only ever *tells* you the right command to install it for your system, and offers to run that command for you only in an interactive terminal, only after you say yes.

`ffprobe` is entirely optional. Sift's `video` organize strategy works without it (MP4/MOV containers only) and uses it automatically for broader format support and richer metadata (`{duration}`, `{fps}`) when it's installed — see "Metadata-driven organize strategies" below.

## Build from source

```bash
git clone https://github.com/sergiocardoso/sift.git
cd sift
cargo build --release
```

The binary will be available at:

```text
target/release/sift
```

Run it directly:

```bash
./target/release/sift --help
```

Or install the local build into your Cargo bin directory:

```bash
cargo install --path .
```

Then:

```bash
sift --help
```

## Update notices

Sift is otherwise local-first and makes no network calls of its own. The one exception: any command may check, in the background, whether a newer release is available on GitHub, and print a one-line notice — to stderr, never stdout, so it can never land inside `--json` output — if so.

This is built to never add latency or noise to a normal command: the check itself never runs inline. A small local cache file tracks when it last ran; at most once every 24 hours, a command spawns a fully detached background process to do the actual (3-second-timeout) network call via `curl` and update the cache — the command that triggered it never waits on it, so a slow or unreachable network never slows anything down, and the notice (if any) only starts showing up starting with the *next* command. If `curl` isn't installed, the check just silently never succeeds — same as everywhere else Sift shells out to an optional external tool.

Set `SIFT_NO_UPDATE_CHECK` (to any value) to disable this entirely — no network call, and no notice printed even from an already-cached result.

## Optional: `sift-tray` (system tray / menu bar UI)

A small, entirely separate app that shows an icon near the clock (macOS menu bar, Windows/Linux tray) listing your watched folders — name, state (running/paused/stopped/config error), with "Open folder" and "Pause"/"Resume" per watch. It's a thin UI shell over the same `sift` library the CLI uses (`watch::registry::list`, the same `cmd_watch_pause`/`cmd_watch_resume` functions `sift watch pause`/`resume` call) — it never talks to the watch daemon directly and never reimplements any watch logic.

Picking a folder from its "Add folder…" dialog is this app's one deliberate authorization gesture (the same contract as `--auto-apply` on the CLI): unlike `sift watch add` on the CLI — where Watch's own "never sweep pre-existing files" rule (see [Why Watch waits before moving a new file](#why-watch-waits-before-moving-a-new-file)) means a folder full of existing files sits untouched until you separately run `sift organize --apply` — the tray runs one real `sift organize --apply` pass on whatever's already in the folder *before* starting the watch, so picking a folder there organizes it immediately, not just from then on.

It lives in its own workspace package specifically so installing/building the `sift` CLI never pulls in GUI dependencies (GTK on Linux, etc.).

Each [GitHub Release](https://github.com/sergiocardoso/sift/releases) includes a prebuilt `sift-tray` archive for Linux (x86_64) and macOS (x86_64/aarch64) alongside `sift` itself — download and extract it next to your installed `sift` binary (e.g. `~/.local/bin`), same as you would with any other release archive. `install.sh` doesn't fetch it automatically yet, so this is a manual step for now. There's no prebuilt Linux aarch64 archive yet (see [Contributing](#contributing) if you'd like to help with cross-compiling GTK for that target), and no Windows build at all — see [Platform support](#platform-support).

Or build it yourself from source:

```bash
cargo build -p sift-tray --release
./target/release/sift-tray
```

On Linux this needs GTK3, an AppIndicator implementation, and `libxdo` (used by the native folder picker) available at build time — e.g. on Debian/Ubuntu: `sudo apt-get install libgtk-3-dev libayatana-appindicator3-dev libxdo-dev` (older distros: `libappindicator3-dev` instead of the `ayatana` package). There's no autostart/packaging yet (no `.app` bundle, no Windows startup entry, no Linux `.desktop` autostart) — for now it's just "run the binary".

### Auto-launched by `sift watch start`, best-effort

Once `sift-tray` is installed (downloaded from a release or built from source, per above) and it's next to `sift` in the same directory, or anywhere on `PATH`, `sift watch start`/`sift watch resume` try to launch it automatically, detached, in the background — no separate step needed after that.

This is entirely best-effort and silent either way: if the `sift-tray` binary isn't installed (the common case, since it's still a manual download even now that it's released), if there's no display server (a headless server, a container, a CI run), or if a `sift-tray` instance is already running, `watch start` still succeeds exactly the same and never prints a warning about it. Only one `sift-tray` instance is ever running at a time (a singleton lock, same mechanism as the watch daemon's own), so starting several watches in a row never opens several tray icons.

---

# Quick start

```bash
# Preview organization of the current directory
sift

# Preview another directory
sift ~/Downloads

# Inspect entries without planning mutations
sift scan ~/Downloads

# Preview organization
sift organize ~/Downloads

# Apply the organization plan
sift organize ~/Downloads --apply

# Inspect nested directories too
sift organize ~/Downloads --recursive

# Diagnose a directory
sift doctor ~/Downloads

# Preview junk cleanup
sift clean ~/Downloads

# Send approved junk candidates to the system trash
sift clean ~/Downloads --apply

# Show operation history
sift history
```

---

# Command overview

| Command | Purpose | Mutates? |
|---|---|---:|
| `sift [path]` | Safe organize preview | No |
| `sift scan [path]` | Inspect directory entries | No |
| `sift organize [path]` | Build an organization plan | Only with `--apply` |
| `sift clean [path]` | Build a cleanup plan | Only with `--apply` |
| `sift doctor [path]` | Report filesystem findings | No |
| `sift history` | Show recorded operations | No |
| `sift undo <id>` | Reverse successful recorded moves | Yes |
| `sift init [path]` | Create a starter `.sift.toml` | Yes |
| `sift watch ...` | Manage continuous organization | Watch execution is explicitly authorized with `--auto-apply` |

---

# Complete CLI reference

This section documents the current CLI surface in detail.

## Global commands

```bash
sift --help
sift --version
```

### Bare invocation

```bash
sift [PATH]
```

`PATH` defaults to the current directory (`.`).

The bare command is equivalent to a safe, non-recursive organize preview.

Examples:

```bash
sift
sift .
sift ~/Downloads
```

It never applies changes automatically.

---

## `sift scan`

Inspect filesystem entries without changing anything.

```bash
sift scan [PATH] [--json] [--recursive]
```

`PATH` defaults to `.`.

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `[PATH]` | No | Directory to inspect. Defaults to `.` |
| `--json` | No | Print machine-readable JSON instead of human-readable output |
| `--recursive` | No | Descend into eligible subdirectories |

### Examples

```bash
sift scan
sift scan ~/Downloads
sift scan ~/Downloads --recursive
sift scan ~/Downloads --json
sift scan ~/Downloads --recursive --json
```

Recursive scanning does **not** descend through protected traversal boundaries such as hidden directories, symlinks, software projects, build output directories, or Sift's own category directories.

---

## `sift organize`

Classify loose files and build a plan that organizes them into category directories.

```bash
sift organize [PATH] [--apply] [--json] [--verbose] [--recursive]
```

`PATH` defaults to `.`.

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `[PATH]` | No | Directory to organize. Defaults to `.` |
| `--apply` | No | Execute the plan. Without it, Sift only previews |
| `--json` | No | Print the plan as JSON |
| `--verbose` | No | Show skipped entries individually instead of collapsing large groups |
| `--recursive` | No | Organize every eligible nested directory in its own local context |

### Examples

Preview:

```bash
sift organize ~/Downloads
```

Apply:

```bash
sift organize ~/Downloads --apply
```

Inspect the full plan:

```bash
sift organize ~/Downloads --verbose
```

Use from a script:

```bash
sift organize ~/Downloads --json
```

Organize nested directories in place:

```bash
sift organize ~/Downloads --recursive
```

Apply recursive organization:

```bash
sift organize ~/Downloads --recursive --apply
```

### Important behavior

Recursive mode preserves directory context.

Given:

```text
Downloads/
├── invoice.pdf
└── Client A/
    ├── proposal.pdf
    └── logo.png
```

Running:

```bash
sift organize Downloads --recursive --apply
```

produces the equivalent of:

```text
Downloads/
├── Documents/
│   └── invoice.pdf
└── Client A/
    ├── Documents/
    │   └── proposal.pdf
    └── Images/
        └── logo.png
```

Sift does **not** flatten `Client A/proposal.pdf` into `Downloads/Documents/`.

### Junk during organize

High-confidence junk extensions currently recognized by Sift:

```text
.tmp
.swp
.swo
```

These can appear as **Trash** actions in an organize plan rather than being moved to a category directory.

Nothing is sent to trash until `--apply` is explicitly used.

---

## `sift clean`

Find high-confidence cleanup candidates and send them to the operating system trash when explicitly applied.

```bash
sift clean [PATH] [--apply] [--json] [--verbose]
```

`PATH` defaults to `.`.

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `[PATH]` | No | Directory to clean. Defaults to `.` |
| `--apply` | No | Send planned trash candidates to the system trash |
| `--json` | No | Print the cleanup plan as JSON |
| `--verbose` | No | Show every skipped entry individually |

### Examples

Preview:

```bash
sift clean ~/Downloads
```

Apply:

```bash
sift clean ~/Downloads --apply
```

Verbose preview:

```bash
sift clean ~/Downloads --verbose
```

JSON:

```bash
sift clean ~/Downloads --json
```

### Why there is no `clean --recursive`

Recursive cleanup is intentionally not part of the current CLI.

Cleaning has a higher destructive risk than organization, so Sift keeps that surface deliberately narrower.

### Built-in cleanup candidates

The built-in high-confidence junk set is currently:

```text
*.tmp
*.swp
*.swo
```

Other files are not automatically treated as trash simply because they look old or unnecessary.

> Trash operations are not currently restored by `sift undo`. `undo` is for successful recorded file moves. The operating system trash remains responsible for trash recovery.

---

## `sift doctor`

Inspect a directory for potentially interesting or risky filesystem entries.

`doctor` is report-only and never reads file contents to make these decisions.

```bash
sift doctor [PATH] [--json] [--recursive]
```

`PATH` defaults to `.`.

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `[PATH]` | No | Directory to inspect. Defaults to `.` |
| `--json` | No | Print findings as JSON |
| `--recursive` | No | Inspect eligible nested directories using the same traversal boundaries as recursive organization |

### Examples

```bash
sift doctor ~/Downloads
sift doctor ~/Downloads --recursive
sift doctor ~/Downloads --json
```

### Current findings

`doctor` can report:

- software projects;
- protected directories;
- symlinks;
- hidden entries;
- sensitive-looking filenames;
- files larger than 100 MB;
- archives older than one year;
- known build/dependency output directories.

Current build/dependency directory names include:

```text
node_modules
target
.venv
```

The sensitive-filename check is filename-based. Sift does not need to open the file to flag it.

---

## `sift history`

List operations Sift has recorded.

```bash
sift history
```

Example:

```bash
sift history
```

History makes applied organization auditable and provides the operation IDs used by `sift undo`.

---

## `sift undo`

Reverse successful file moves from a recorded operation.

```bash
sift undo <OPERATION_ID>
```

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `<OPERATION_ID>` | Yes | History ID shown by `sift history`, such as `hist-...` |

Example:

```bash
sift undo hist-1699999999999999999
```

### Undo safety

Undo does not blindly move files back.

Immediately before restoring a file, Sift verifies that:

- the original location is still free;
- the moved destination still exists;
- the destination is a regular file;
- the destination has not become a symlink;
- the destination has not become a directory.

If those assumptions no longer hold, Sift refuses that undo rather than overwrite or move something unsafe.

A separate history entry records the undo attempt.

### What undo does not restore

Trash actions are not restored by Sift's history system.

If a file was sent to the operating system trash, recovery belongs to the trash implementation of your OS.

---

## `sift init`

Create a starter `.sift.toml` configuration file.

```bash
sift init [PATH] [--force]
```

`PATH` defaults to `.`.

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `[PATH]` | No | Directory in which `.sift.toml` should be created. Defaults to `.` |
| `--force` | No | Overwrite an existing `.sift.toml` |

Examples:

```bash
sift init
sift init ~/Downloads
sift init ~/Downloads --force
```

Without `--force`, Sift refuses to overwrite an existing `.sift.toml`.

---

# Sift Watch

Sift Watch turns selected folders into continuously organized inboxes.

The important distinction is that automatic mutation requires an explicit, persistent authorization:

```bash
--auto-apply
```

Registering a watch does **not** immediately start monitoring it.

A newly registered watch starts in the `stopped` state.

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

Pre-existing files are not automatically backfilled when a watch starts.

---

## `sift watch add`

Register a directory for automatic organization.

```bash
sift watch add <PATH> --auto-apply [--recursive]
```

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `<PATH>` | Yes | Existing directory to register |
| `--auto-apply` | **Yes** | Explicit persistent authorization allowing Watch to organize new eligible files automatically |
| `--recursive` | No | Also monitor eligible nested paths using recursive organization boundaries |

Examples:

```bash
sift watch add ~/Downloads --auto-apply
sift watch add ~/Desktop/Inbox --auto-apply --recursive
```

This only registers the watch.

Start it separately:

```bash
sift watch start ~/Downloads
```

---

## `sift watch list`

List registered watches.

```bash
sift watch list [--json]
```

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `--json` | No | Print the registry information as JSON |

Examples:

```bash
sift watch list
sift watch list --json
```

Watch records include useful operational state such as:

- path;
- state;
- recursive mode;
- auto-apply authorization;
- organized file count;
- error count;
- last activity;
- last error.

---

## `sift watch status`

Show watch status.

```bash
sift watch status [PATH] [--json]
```

### Parameters

| Parameter | Required | Description |
|---|---:|---|
| `[PATH]` | No | Show one registered watch. If omitted, show overall status/list information |
| `--json` | No | Print watch information as JSON |

Examples:

```bash
sift watch status
sift watch status ~/Downloads
sift watch status ~/Downloads --json
```

---

## `sift watch start`

Start a registered stopped watch.

```bash
sift watch start <PATH>
```

Example:

```bash
sift watch start ~/Downloads
```

Starting a watch does not organize files that were already present before the watch became active.

If you want to organize existing files first:

```bash
sift organize ~/Downloads --apply
sift watch start ~/Downloads
```

---

## `sift watch pause`

Pause a running watch while keeping it registered.

```bash
sift watch pause <PATH>
```

Example:

```bash
sift watch pause ~/Downloads
```

Pending candidates that have not yet become stable are discarded rather than queued for a later catch-up.

---

## `sift watch resume`

Resume a paused watch.

```bash
sift watch resume <PATH>
```

Example:

```bash
sift watch resume ~/Downloads
```

Resume processes new events from that point forward. It does not backfill files created while the watch was paused.

---

## `sift watch stop`

Stop processing a watch while keeping the registration.

```bash
sift watch stop <PATH>
```

Example:

```bash
sift watch stop ~/Downloads
```

You can later start it again.

---

## `sift watch remove`

Remove a watch registration entirely.

```bash
sift watch remove <PATH>
```

Example:

```bash
sift watch remove ~/Downloads
```

---

# Watch daemon commands

Watch uses one background daemon for registered running watches.

## Daemon status

```bash
sift watch daemon status
```

Shows whether the daemon is running and, when available, its PID/start information.

## Stop the daemon

```bash
sift watch daemon stop
```

Requests a safe daemon shutdown.

## Run daemon in foreground

```bash
sift watch daemon run
```

This is an internal/advanced command used by Sift when launching the daemon and is normally not something users need to invoke manually.

### Platform support

The detached background Watch daemon is currently implemented for Unix-like platforms.

On unsupported platforms, Sift reports an explicit error instead of pretending that background Watch is running.

#### Windows

There's no official Windows build (`install.sh` and the GitHub release only cover Linux and macOS). That said, everything except `sift watch` is written against portable `std::fs` APIs and the cross-platform `trash` crate, and the whole workspace does cross-compile cleanly for `x86_64-pc-windows-gnu` — `scan`/`organize`/`clean`/`doctor`/`history`/`undo`/`init`/`folders`/`config`/`explain` are expected to work if you build from source, though this has never been run on real Windows and isn't covered by CI. `sift watch` specifically won't: the daemon spawn in `watch::platform` is intentionally Unix-only (see above), so a native Windows build of the daemon needs real process-detachment work (`CREATE_NEW_PROCESS_GROUP`/`DETACHED_PROCESS`) that hasn't been done or tested on a Windows machine yet.

---

# Why Watch waits before moving a new file

A filesystem event does not necessarily mean a file has finished arriving.

For example, a browser may create a download and continue writing to it for several seconds.

Sift therefore uses a stability window before a candidate becomes eligible for organization.

The current default is approximately:

```text
2.5 seconds unchanged
```

Sift observes size and modification time. If either changes, the stability timer is restarted.

Common temporary download names are ignored while they remain transient:

```text
.crdownload
.part
.download
.tmp
```

When an application renames the file to its final name, the final filesystem event can become a fresh candidate.

---

# File classification

Built-in classification is deterministic and case-insensitive by extension.

Sift does not inspect file contents to decide the category.

| Category | Destination | Extensions |
|---|---|---|
| Documents | `Documents/` | `pdf`, `txt`, `md`, `markdown`, `rtf`, `doc`, `docx`, `odt`, `xls`, `xlsx`, `ods`, `ppt`, `pptx`, `odp`, `epub`, `mobi` |
| Images | `Images/` | `jpg`, `jpeg`, `png`, `gif`, `webp`, `svg`, `bmp`, `tiff`, `tif`, `heic`, `heif`, `avif`, `ico` |
| Audio | `Audio/` | `mp3`, `wav`, `flac`, `aac`, `m4a`, `ogg`, `opus`, `wma` |
| Video | `Video/` | `mp4`, `mov`, `mkv`, `avi`, `webm`, `m4v`, `mpg`, `mpeg`, `wmv` |
| Archives | `Archives/` | `zip`, `rar`, `7z`, `tar`, `gz`, `bz2`, `xz`, `tgz`, `tbz2`, `txz` |
| 3D | `3D/` | `stl`, `obj`, `3mf`, `step`, `stp`, `blend`, `blend1`, `fbx`, `glb`, `gltf`, `dae`, `ply` |
| Code | `Code/` | `js`, `jsx`, `mjs`, `cjs`, `ts`, `tsx`, `py`, `rs`, `go`, `php`, `dart`, `sh`, `bash`, `zsh`, `fish`, `lua`, `java`, `c`, `h`, `cc`, `cpp`, `cxx`, `hpp`, `cs`, `rb`, `swift`, `kt`, `kts`, `scala`, `vue`, `svelte` |
| Data | `Data/` | `json`, `jsonl`, `yaml`, `yml`, `toml`, `csv`, `tsv`, `xml`, `sql`, `sqlite`, `sqlite3`, `db`, `parquet`, `ndjson` |
| Other | `Other/` | Any other ordinary unprotected file |
| Junk | Trash candidate | `tmp`, `swp`, `swo` |

Unknown ordinary files are intentionally moved to `Other/` rather than being left as ambiguous unclassified files.

---

# Project and traversal protection

Sift recognizes common software project markers.

A directory containing any of these is considered a software project root:

```text
.git
Cargo.toml
package.json
pyproject.toml
pubspec.yaml
```

Recursive traversal also stops at known build/dependency output directories:

```text
node_modules
target
.venv
```

Sift also avoids descending into its own category directories:

```text
Documents
Images
Audio
Video
Archives
3D
Code
Data
Other
```

This prevents repeated nesting such as:

```text
Documents/Documents/file.pdf
```

---

# Configuration with `.sift.toml`

Sift can override built-in classification with explicit rules.

Generate a starter file:

```bash
sift init ~/Downloads
```

Example:

```toml
[[rules]]
name = "keep database exports here"
pattern = "*.db"
action = "Move"
destination = "Database"
priority = 100
enabled = true
description = "Keep database files in a dedicated folder"

[[rules]]
name = "never touch notes"
pattern = "notes.md"
action = "Skip"
priority = 200
enabled = true

[[rules]]
name = "temporary editor files"
pattern = "*.swp"
action = "Trash"
priority = 300
enabled = true
```

## Rule parameters

Every `[[rules]]` block supports:

| Field | Type | Required | Description |
|---|---|---:|---|
| `name` | string | Yes | Human-readable rule name |
| `pattern` | string | Yes | Filename matching pattern |
| `action` | string | Yes | `Move`, `Trash`, or `Skip` |
| `destination` | string | For `Move` | Relative destination directory under the target |
| `priority` | integer | Yes | Higher values run first |
| `enabled` | boolean | No | Whether the rule participates. Defaults to `false` when omitted |
| `description` | string | No | Human-readable reason shown for the decision |

### Rule precedence

Enabled rules are evaluated from highest `priority` to lowest.

The first matching rule wins.

```text
explicit enabled rule
        ↓
built-in classification
```

For example, although JSON normally maps to `Data/`, this rule leaves JSON files untouched:

```toml
[[rules]]
name = "keep json here"
pattern = "*.json"
action = "Skip"
priority = 1000
enabled = true
```

### Pattern matching in v0.1

Rule matching is filename-based and intentionally simple in the current version.

Patterns such as these are suitable:

```text
*.zip
*.json
notes.md
```

It is not currently a full filesystem glob engine. Avoid relying on advanced glob semantics.

### `Move`

```toml
[[rules]]
name = "sqlite files"
pattern = "*.sqlite"
action = "Move"
destination = "Databases"
priority = 100
enabled = true
```

Destinations must remain beneath the selected target directory.

Unsafe destinations are rejected/skipped. In particular, destinations must not use:

```text
absolute paths
..
.
root components
symlink escapes
```

### `Skip`

```toml
[[rules]]
name = "leave markdown notes alone"
pattern = "*.md"
action = "Skip"
priority = 100
enabled = true
```

### `Trash`

```toml
[[rules]]
name = "editor temp"
pattern = "*.swp"
action = "Trash"
priority = 100
enabled = true
```

In `organize`, `Move`, `Trash`, and `Skip` rules are meaningful.

In `clean`, `Trash` and `Skip` are meaningful; a `Move` rule is not applied as a move by the cleanup planner.

---

# Metadata-driven organize strategies

Besides `strategy = "type"` (classification by extension), `.sift.toml` supports five strategies that render a destination from a `template` string instead:

| Strategy | Metadata source | Example template |
|---|---|---|
| `date` | filesystem modification time | `"{year}/{month}"` |
| `audio` | tag metadata (ID3v2, Vorbis comments, MP4 atoms, ...) | `"{artist}/{album}"` |
| `video` | MP4/MOV container info | `"{resolution}/{year}"` |
| `photos` | EXIF metadata | `"{camera}/{year}"` |
| `documents` | PDF `/Info` or Office `docProps/core.xml` | `"{author}/{year}"` |

`[[rules]]` always wins over any of these, exactly as with `type`.

## `date`

```toml
[organize]
strategy = "date"
template = "{year}/{month}"
```

Supported placeholders: `{year}`, `{month}`, `{day}`. `organize.date_source` defaults to (and currently only supports) `"modified"`.

## `audio`

```toml
[organize]
strategy = "audio"
template = "{artist}/{album}"
```

Reads tag metadata via [`lofty`](https://crates.io/crates/lofty) — mp3, flac, m4a, ogg, opus, wav, wma, aiff, and more. Supported placeholders: `{artist}`, `{album}`, `{album_artist}`, `{genre}`, `{track}`, `{title}`, `{year}`.

## `video`

```toml
[organize]
strategy = "video"
template = "{resolution}/{year}"
```

Reads container-level metadata via a pure-Rust parser (MP4/MOV only, no dependency on any external tool) by default. If `ffprobe` is installed and on `PATH`, Sift uses it automatically instead — same `{width}`/`{height}`/`{resolution}`/`{codec}`/`{year}` values either way (`{codec}` always renders the raw fourcc, e.g. `avc1`, never `ffprobe`'s friendlier codec name, so a template you wrote before installing `ffmpeg` never silently points somewhere new afterward), plus broader container support and two extra placeholders:

| Placeholder | Source | Requires `ffprobe`? |
|---|---|---|
| `{width}`, `{height}`, `{resolution}` (`WIDTHxHEIGHT`) | video track dimensions | No |
| `{codec}` | raw fourcc, e.g. `avc1` | No |
| `{year}` | the container's creation time, when set | No |
| `{duration}` | length in whole seconds | Yes |
| `{fps}` | rounded frame rate | Yes |

There is never a shell-injection risk: the file path is always passed as a separate process argument, never interpolated into a shell command. If `ffprobe` isn't installed, `{duration}`/`{fps}` are simply unavailable — see below.

## `photos`

```toml
[organize]
strategy = "photos"
template = "{camera}/{year}/{month}"
```

Reads EXIF metadata via [`kamadak-exif`](https://crates.io/crates/kamadak-exif) (pure Rust, no external tool) — JPEG, TIFF, HEIF/HEIC, PNG, and WebP are all auto-detected. Supported placeholders: `{camera}` (Make+Model, deduplicated when the model already repeats the make, e.g. `"Canon EOS R5"` rather than `"Canon Canon EOS R5"`), `{year}`, `{month}`, `{day}` (from `DateTimeOriginal` — the capture date, never the file's mtime).

There is deliberately no `{gps}`/location placeholder: embedding capture coordinates in a folder name is an easy way to leak where a photo was taken without meaning to.

## `documents`

```toml
[organize]
strategy = "documents"
template = "{author}/{year}"
```

Reads metadata from PDF and Office files (docx/xlsx/pptx), normalized into the same shape regardless of format:

| Format | Source |
|---|---|
| PDF | the `/Info` dictionary, via the pure-Rust [`lopdf`](https://crates.io/crates/lopdf) crate |
| docx/xlsx/pptx | `docProps/core.xml` inside the zip, via the pure-Rust [`zip`](https://crates.io/crates/zip) + [`roxmltree`](https://crates.io/crates/roxmltree) crates |

Supported placeholders: `{author}`, `{title}`, `{year}`, `{month}`, `{day}` (the document's own recorded creation date — PDF's `CreationDate`, Office's `dcterms:created` — never the file's mtime).

## Missing metadata is a skip, never a guess

If a file is missing a tag/field the template references (an untagged mp3, a photo with no EXIF data, a PDF with no `/Info` dictionary), that file is **skipped** with a clear reason — Sift never invents a fallback bucket like `Unknown Artist/`. Run `sift explain <file>` to see exactly which field was unavailable.

## Walkthrough: a different strategy per folder

`.sift.toml` lives inside the directory it governs (`PATH/.sift.toml` — see [Configuration lookup](#configuration-lookup) below), not in one central file. That means each top-level folder you organize can run its own, independent strategy — your music library by tags, your photo library by camera/date, your PDFs by author, all at the same time:

```text
~/Music/.sift.toml       strategy = "audio"      → {artist}/{album}
~/Pictures/.sift.toml    strategy = "photos"     → {camera}/{year}/{month}
~/Documents/.sift.toml   strategy = "documents"  → {author}/{year}
~/Downloads/.sift.toml   strategy = "type"       → built-in Documents/Images/Audio/... (or no file at all)
```

Set one up end to end. Generate a starter file:

```bash
sift init ~/Music
```

Replace the generated `[[rules]]` example in `~/Music/.sift.toml` with:

```toml
[organize]
strategy = "audio"
template = "{artist}/{album}"
```

Preview the plan, then apply it:

```bash
sift organize ~/Music
sift organize ~/Music --apply
```

Before:

```text
~/Music/
├── 01 Track One.mp3
├── 02 Track Two.mp3
└── live_bootleg.flac
```

After — destinations come from each file's own tags, never the filename:

```text
~/Music/
├── Daft Punk/
│   └── Discovery/
│       ├── 01 Track One.mp3
│       └── 02 Track Two.mp3
└── Radiohead/
    └── I Might Be Wrong/
        └── live_bootleg.flac
```

A file missing the `{artist}`/`{album}` tag is skipped, not dropped into a guessed folder — run `sift explain ~/Music/live_bootleg.flac` beforehand to see exactly which tag would be used or missing.

The same `sift init <dir>` → edit `.sift.toml` → `sift organize <dir> --apply` pattern applies to `~/Pictures` (`strategy = "photos"`, e.g. `template = "{camera}/{year}/{month}"`) and `~/Documents` (`strategy = "documents"`, e.g. `template = "{author}/{year}"`). Each directory's policy only ever affects files inside that directory.

## `audio`/`video`/`photos`/`documents` don't support `--recursive` yet

Unlike `date`'s fixed-width `{year}`/`{month}`/`{day}`, an `{artist}`, `{camera}`, or `{author}` value is free text — structurally indistinguishable from any other folder name. That means there's currently no safe way to detect "this directory was generated by this same policy" and avoid re-entering it on a second run. `sift organize --recursive` and `sift watch add --recursive` both refuse `strategy = "audio"`/`"video"`/`"photos"`/`"documents"` with a clear error instead of guessing; a plain (non-recursive) `sift organize` and non-recursive `sift watch` work normally.

---

# Configuration lookup

For a command targeting `PATH`, Sift currently looks for configuration in this order:

```text
1. PATH/.sift.toml
2. platform config directory / sift / config.toml
3. built-in defaults
```

On a typical Linux system, the global path corresponds to something like:

```text
~/.config/sift/config.toml
```

Sift does **not** currently walk up parent directories looking for additional `.sift.toml` files.

For a recursive organize operation, the configuration selected for the command root is used for that operation.

Watch resolves configuration from the registered watch root while processing candidates.

---

# Safety model

Safety is part of Sift's architecture, not an optional mode.

## Dry-run first

```bash
sift organize ~/Downloads
```

shows the plan.

```bash
sift organize ~/Downloads --apply
```

executes it.

The same pattern applies to cleanup.

## Symlinks are not followed for organization decisions

Sift uses no-follow metadata for safety-sensitive filesystem checks.

A symlink, including a broken symlink, is treated as an occupied/protected filesystem entry rather than an empty destination.

## No silent overwrite

If the destination already exists, the move is skipped.

That includes an existing:

- regular file;
- directory;
- symlink;
- broken symlink.

## No automatic directory merge

Sift does not merge directory trees simply because destination names happen to match.

## Project roots are protected

Running organization against a recognized project root causes the target contents to be protected instead of dismantled into `Code/`, `Data/`, and other categories.

## Live revalidation before mutation

Planning-time assumptions are not blindly trusted during execution.

Sift re-checks filesystem state before mutating so that a destination created between preview and execution can block the move safely.

## No copy-delete fallback

If a rename cannot be performed safely, Sift does not silently turn it into a copy-then-delete operation.

## Trash instead of permanent deletion

Cleanup uses the operating system trash instead of issuing a permanent `rm` on your behalf.

---

# JSON output and scripting

The following command groups currently expose `--json`:

```text
scan
organize
clean
doctor
watch list
watch status
```

Examples:

```bash
sift scan ~/Downloads --json
sift organize ~/Downloads --json
sift doctor ~/Downloads --recursive --json
sift watch list --json
```

This makes Sift useful as both an interactive CLI and a building block for scripts and other tools.

Example with `jq`:

```bash
sift scan ~/Downloads --json | jq .
```

---

# Suggested workflows

## Keep Downloads manageable manually

```bash
sift organize ~/Downloads
sift organize ~/Downloads --apply
sift history
```

## Inspect before organizing

```bash
sift scan ~/Desktop --recursive
sift doctor ~/Desktop --recursive
sift organize ~/Desktop --recursive
```

## Create a custom Downloads policy

```bash
sift init ~/Downloads
$EDITOR ~/Downloads/.sift.toml
sift organize ~/Downloads
```

## Organize a library by its own metadata (music, photos, documents...)

Each folder's `.sift.toml` is independent, so `~/Music`, `~/Pictures`, and `~/Documents` can each run a different metadata-based `strategy` (tags, EXIF, document properties) at the same time — see [Walkthrough: a different strategy per folder](#walkthrough-a-different-strategy-per-folder) for the full example.

## Turn an Inbox into a Smart Inbox

```bash
mkdir -p ~/Inbox
sift watch add ~/Inbox --auto-apply
sift watch start ~/Inbox
sift watch status ~/Inbox
```

From then on, new stable eligible files can be organized automatically.

## Organize existing files before enabling Watch

```bash
sift organize ~/Inbox
sift organize ~/Inbox --apply
sift watch add ~/Inbox --auto-apply
sift watch start ~/Inbox
```

This keeps the distinction clear:

```text
manual organize → existing content
watch           → future filesystem events
```

---

# Architecture

The core flow is intentionally separated into stages:

```text
Filesystem
   │
   ▼
Scanner
   │
   ▼
Classifier
   │
   ▼
Rules / policy
   │
   ▼
Planner
   │
   ▼
Review
   │
   ▼
Executor
   │
   ▼
History
   │
   ▼
Undo
```

The scanner does not mutate the filesystem.

The planner decides what should happen.

The executor is responsible for applying an explicitly authorized plan and revalidating safety assumptions.

Watch reuses the same organization authority instead of inventing a separate, weaker organizer.

---

# Project layout

```text
src/
├── classifier.rs   deterministic extension → category classification
├── cli.rs          command-line interface and parameters
├── config.rs       .sift.toml rules and configuration lookup
├── domain.rs       shared domain types
├── executor.rs     filesystem mutation + execution safeguards
├── fs.rs           filesystem helpers
├── history.rs      history persistence and undo
├── planner.rs      organization and cleanup planning
├── render.rs       human-readable output
├── scanner.rs      scanning, diagnosis, recursive traversal boundaries
├── utils.rs        shared utility helpers
└── watch/
    ├── daemon.rs
    ├── eligibility.rs
    ├── engine.rs
    ├── platform.rs
    ├── registry.rs
    └── stability.rs
```

---

# Development

Run formatting:

```bash
cargo fmt --check
```

Run Clippy:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Run tests:

```bash
cargo test
```

Build release binary:

```bash
cargo build --release
```

---

# Contributing

Contributions are welcome.

Before opening a pull request, please make sure the project is formatted, lint-clean, and passing tests.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for project contribution guidance and [`SECURITY.md`](SECURITY.md) for security reports.

---

# License

Sift is licensed under the [Apache License 2.0](LICENSE).

Copyright 2026 Sérgio Cardoso.