---
title: Your First Organize
description: A full preview, apply, and undo cycle against a real directory.
---

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

## 1. Preview

Nothing is mutated by this — it only prints a plan:

```bash
sift organize ~/Downloads
```

```text
invoice.pdf      → Documents/
vacation.jpg     → Images/
backup.zip       → Archives/
data.json        → Data/
model.stl        → 3D/
experiment.rs    → Code/
notes.xyz        → Other/
my-project/      → protected: software project

No changes made.
Run with --apply to execute.
```

`my-project/` is left alone entirely — it contains `Cargo.toml`, which marks it as a software project. See [Protected Paths](/core-concepts/protected-paths/).

## 2. Apply

Happy with the plan? Run the same command with `--apply`:

```bash
sift organize ~/Downloads --apply
```

```text
✓ Applied successfully

7 files moved
7 directories created
1 entry skipped
0 failures

History
  hist-...
```

The `hist-...` id is what you'd use to undo this specific operation.

## 3. Undo

Changed your mind?

```bash
sift undo hist-...
```

```text
✓ 7 items restored
```

Undo re-verifies the live filesystem before restoring anything — see [History and Undo](/core-concepts/history-and-undo/) for exactly what it checks and when it refuses.

## What just happened

- Classification was by file extension (the default `type` strategy) — see [Classification](/core-concepts/classification/).
- Nothing was overwritten, merged, or force-deleted at any point — see [Safety Model](/core-concepts/safety-model/).
- You could have asked *why* a specific file would move before applying anything, with `sift explain ~/Downloads/invoice.pdf` — see [Explain](/organizing/explain/).

## Next

- Want this to happen automatically for new files? See [Watch → Overview](/watch/overview/).
- Want different rules for this folder? See [.sift.toml](/configuration/sift-toml/).
