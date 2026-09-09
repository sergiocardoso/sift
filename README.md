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