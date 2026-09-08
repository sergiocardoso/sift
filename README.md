# Sift

**A local-first CLI that actually organizes your files — safely, deterministically, and never without your permission.**

![License: Apache 2.0](https://img.shields.io/badge/license-Apache%202.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-stable-orange.svg)
![Status](https://img.shields.io/badge/status-v0.1-lightgrey.svg)

Point Sift at a messy folder — `Downloads`, `Desktop`, a client's Dropbox drop —
and it sorts loose files into `Documents/`, `Images/`, `Code/`, `Data/`, `3D/`,
and more, based on deterministic rules you can see and undo. No cloud, no AI,
no surprises: every mutation is planned, shown to you, and reversible.

```
$ sift organize ~/Downloads

Sift organize
/home/you/Downloads

Plan
  6 moves
  4 directories
  2 skipped

Moves
  invoice.pdf     → Documents/
  photo.jpg       → Images/
  data.json       → Data/
  script.js       → Code/
  model.blend     → 3D/
  archive.zip     → Archives/

Skipped
  my-project/     software project
  .config/        hidden directory

No changes made.
Run with --apply to execute.
```

---

## Table of contents

- [Why Sift](#why-sift)
- [Install](#install)
- [Quick start](#quick-start)
- [The pipeline](#the-pipeline)
- [Commands](#commands)
- [Classification](#classification)
- [Recursive mode](#recursive-mode)
- [Sift Watch — automatic organization](#sift-watch--automatic-organization)
- [Configuration (`.sift.toml`)](#configuration-sifttoml)
- [Safety guarantees](#safety-guarantees)
- [JSON output & scripting](#json-output--scripting)
- [Project layout](#project-layout)
- [Contributing](#contributing)
- [License](#license)

---

## Why Sift

Most "file organizer" tools fall into two camps: fragile shell scripts you
wrote once and are afraid to touch, or opaque AI tools that move things
around in ways you can't predict or reverse. Sift is neither.

- **Deterministic.** Classification is extension- and metadata-based, not a
  model's guess. The same folder produces the same plan every time.
- **Dry-run by default.** Every command prints a plan first. Nothing moves
  until you pass `--apply`, explicitly, every time.
- **Conservative about what it touches.** Hidden files, symlinks (including
  broken ones), and anything that looks like a software project (`.git`,
  `Cargo.toml`, `package.json`, `pyproject.toml`, `pubspec.yaml`,
  `node_modules`, `.venv`, `target`) are left alone — always.
- **Undoable.** Every successful move is recorded in history and can be
  reversed with one command.
- **No silent deletes.** Cleanup sends files to your system trash. `rm` is
  never called on your behalf.
- **Set it and forget it, on your terms.** [Sift Watch](#sift-watch--automatic-organization)
  turns a folder into a self-organizing inbox — but only after you grant it
  explicit, persistent authorization.

## Install

Sift is a single Rust binary with no runtime dependencies.

```sh
git clone <this-repository>
cd sift
cargo build --release
./target/release/sift --help
```

Requires a stable Rust toolchain (1.89+ — Sift uses the file-locking APIs
stabilized in `std::fs::File`).

## Quick start

```sh
sift .                        # bare invocation: dry-run preview of the current directory
sift scan ~/Downloads         # list what's there, read-only
sift organize ~/Downloads     # preview an organize plan
sift organize ~/Downloads --apply
sift history                  # see what happened
sift undo hist-1699999999999  # change your mind
```

## The pipeline

Every mutating command follows the same pipeline, and the scanner **never**
mutates anything:

```
scanner → classifier → planner → review (you) → executor → history
```

1. **Scan** the target directory (or, in `--recursive` mode, every eligible
   subdirectory) using no-follow (`symlink_metadata`) filesystem calls.
2. **Classify** each ordinary file deterministically by extension.
3. **Plan** a list of explicit actions (`Move`, `Trash`, `CreateDir`, `Skip`),
   each with a reason, checking collisions against the *live* filesystem.
4. **Show you the plan.** Dry-run is the default for every command.
5. **Execute**, only with `--apply`, re-validating every assumption
   immediately before each mutation (no TOCTOU windows, no overwrites, ever).
6. **Record history** — one auditable record per operation, undoable.

## Commands

| Command | What it does |
|---|---|
| `sift` / `sift <path>` | Bare invocation: safe organize dry-run of the current (or given) directory |
| `sift scan [path]` | List entries, read-only, never mutates |
| `sift organize [path]` | Preview or (`--apply`) execute an organize plan |
| `sift clean [path]` | Preview or (`--apply`) send high-confidence junk (`.tmp`/`.swp`/`.swo`) to the trash |
| `sift doctor [path]` | Report-only: large files, stale archives, sensitive-looking filenames, build output dirs |
| `sift history` | List past operations |
| `sift undo <id>` | Reverse a successful move |
| `sift init [path]` | Write a starter `.sift.toml` |
| `sift watch ...` | Register, start, pause, resume, stop, or remove a persistent watch — see [below](#sift-watch--automatic-organization) |

Common flags:

- `--apply` (`organize`, `clean`) — actually perform the mutation; omit for a dry-run.
- `--recursive` (`scan`, `organize`, `doctor` — **not** `clean`) — descend into eligible subdirectories. See [Recursive mode](#recursive-mode).
- `--json` (`scan`, `organize`, `clean`, `doctor`) — machine-readable output; the schema never changes based on `--recursive`.
- `--verbose` (`organize`, `clean`) — list every skipped entry individually instead of a collapsed summary.

## Classification

Files are classified by extension (case-insensitive), never by content.
Nothing is ever read to make a decision — only filenames and metadata.

| Category | Destination | Example extensions |
|---|---|---|
| Document | `Documents/` | pdf, txt, md, rtf, doc(x), odt, xls(x), ppt(x), epub, mobi |
| Image | `Images/` | jpg, png, gif, webp, svg, bmp, tiff, heic, avif, ico |
| Video | `Video/` | mp4, mov, mkv, avi, webm, m4v, mpg, wmv |
| Audio | `Audio/` | mp3, wav, flac, aac, m4a, ogg, opus, wma |
| Archive | `Archives/` | zip, rar, 7z, tar, gz, bz2, xz, tgz |
| 3D | `3D/` | stl, obj, 3mf, step, blend, blend1, fbx, glb, gltf, dae, ply |
| Code | `Code/` | js, ts, py, rs, go, java, c, cpp, cs, rb, swift, kt, sh, vue, svelte |
| Data | `Data/` | json, yaml, toml, csv, tsv, xml, sql, sqlite, parquet |
| *(anything else)* | `Other/` | An ordinary file with no more specific category is still organized — never silently skipped |

Only genuinely **protected** entries are left in place: directories,
symlinks (valid or broken), hidden files, collisions, and anything inside a
recognized software project. See [Safety guarantees](#safety-guarantees).

`Documents/`, `Images/`, … `Other/` — every category directory Sift itself
creates — is treated as terminal: Sift never reorganizes a file that's
already inside one, and recursive mode never descends into one. Run
`organize` twice in a row and the second run finds nothing left to do.

## Recursive mode

Without `--recursive`, only the immediate children of the target directory
are touched. With it, Sift organizes **every eligible subdirectory in
place** — each one is its own local context, never flattened into the root:

```
Downloads/
├── contrato.pdf
└── Cliente A/
    ├── foto.jpg
    └── proposta.pdf
```

```sh
sift organize ~/Downloads --recursive --apply
```

```
Downloads/
├── Documents/
│   └── contrato.pdf
└── Cliente A/
    ├── Documents/
    │   └── proposta.pdf
    └── Images/
        └── foto.jpg
```

Traversal stops — the entire subtree is protected — at hidden directories,
symlinks, software project roots, `.git`/`node_modules`/`.venv`/`target`,
and Sift's own category directories. A nested project only gets *more*
protected the deeper you look, never less.

## Sift Watch — automatic organization

Turn a folder into a persistent, self-organizing inbox. A single background
daemon watches every folder you register and organizes new files the moment
they've finished arriving — using the exact same planner and executor as
manual `organize`, so there is no separate, weaker code path.

```sh
sift watch add ~/Desktop/Inbox --auto-apply   # register (starts STOPPED)
sift watch start ~/Desktop/Inbox              # begin monitoring
sift watch list                               # see every watch and its state
sift watch pause ~/Desktop/Inbox              # temporarily stop processing
sift watch resume ~/Desktop/Inbox
sift watch stop ~/Desktop/Inbox
sift watch remove ~/Desktop/Inbox
sift watch daemon status
sift watch daemon stop
```

```
$ sift watch list
Sift watch

  ●  /home/you/Desktop/Inbox
     running · auto-apply · recursive off
     14 files organized · last activity 2026-09-08 21:19 UTC
```

**What makes this safe, not just convenient:**

- **`--auto-apply` is mandatory and explicit.** `sift watch add <path>`
  without it is rejected outright — there is no implicit way to grant a
  folder standing permission to mutate itself.
- **`add` never starts monitoring, and `start` never back-fills.** Only
  files that appear *after* a watch is actually running are ever touched.
  Pausing and resuming never triggers a catch-up sweep either.
- **File-stability debouncing.** A new file must sit unchanged (size +
  mtime) for ~2.5 seconds before Sift will touch it — a browser still
  writing `video.mp4` is never yanked out from under it. Files with
  transient names (`.crdownload`, `.part`, `.download`, `.tmp`) are ignored
  until they're renamed to their final name.
- **Live re-validation, every time.** Immediately before organizing a file,
  Sift re-checks every directory between the watch root and that file
  against the *current* filesystem — so if a `package.json` appears in a
  subfolder a moment after a `.js` file did, the whole subtree is
  protected before the executor ever runs, and Sift's own
  `Images/photo.jpg` move can never trigger another move.
- **One daemon, OS-level singleton lock.** Never two daemons, never a
  corrupted registry under concurrent CLI/daemon writes.
- **Fully auditable.** Every automatic move is a normal history entry
  (`sift history` shows its origin), undoable exactly like a manual one.

Watch is **organize-only** — it never trashes or cleans automatically, and
`clean --recursive`/`watch --auto-clean` don't exist. The background daemon
is currently Unix-only; other platforms get an explicit error instead of a
silent no-op.

## Configuration (`.sift.toml`)

```sh
sift init ~/Downloads
```

```toml
[[rules]]
name = "archive files"
pattern = "*.zip"
action = "Move"          # Move | Trash | Skip
destination = "Archives"
priority = 2
enabled = true
```

Rules are evaluated by descending `priority` (highest first); the first
match wins, and always **wins over built-in classification** — a config
`Skip` for `*.json` overrides the built-in `Data` category. `Move`
destinations must be a plain relative path under the target: absolute
paths, `..`, and symlink escapes are rejected outright. Config precedence
is identical for manual `organize`, `--recursive`, and `watch`.

## Safety guarantees

Things Sift will **never** do, by design and by test:

- Mutate anything without `--apply` (or, for watch, without
  `--auto-apply` granted at registration time).
- Follow a symlink when deciding what to scan, plan, or move — broken
  symlinks count as *occupied*, never as available space.
- Overwrite an existing destination — file, directory, symlink, or broken
  symlink all block a move identically.
- Extract a file out of a directory recognized as a software project.
- Re-enter one of its own category directories (no `Documents/Documents`).
- Fall back to a cross-filesystem copy+delete when a rename fails.
- Permanently delete anything — `clean` uses your system trash.
- Trust a planning-time decision at execution time: the executor
  independently re-validates every assumption (including the full
  ancestor chain, for symlink-escape protection) immediately before
  mutating.

## JSON output & scripting

```sh
sift scan ~/Downloads --json
sift organize ~/Downloads --json
```

`--json` output is stable and machine-oriented regardless of `--recursive`
or classification changes — it's the same `Plan`/`Entry` schema either way,
just spanning more directories or richer categories.

## Project layout

```
src/
  scanner.rs     read-only discovery & traversal-boundary rules
  classifier.rs  deterministic extension → category mapping
  planner.rs     turns entries into an Action plan (config precedence, collisions, ...)
  executor.rs    the only code that mutates the filesystem; re-validates everything
  history.rs     undo/history persistence
  config.rs      .sift.toml loading & rule validation
  render.rs      human-readable output (all --json bypasses this)
  watch/         registry, stability engine, daemon, platform-specific spawn
```

See `CONTRIBUTING.md` for how to propose changes, `SECURITY.md` for
reporting vulnerabilities, and `CHANGELOG.md` for release notes.

## Contributing

Contributions are welcome. Please run `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`,
and `cargo test` before opening a PR — see `CONTRIBUTING.md`.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).

```
Copyright 2026 Sérgio Cardoso

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
