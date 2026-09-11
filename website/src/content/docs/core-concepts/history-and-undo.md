---
title: History and Undo
description: How applied operations are recorded, and what undo verifies before restoring anything.
---

Every applied `organize`/`folders` operation is recorded.

```bash
sift history
```

lists past operations, each with an id (`hist-...`).

## Reversing an operation

```bash
sift undo hist-<operation-id>
```

## What undo checks before restoring

Undo is intentionally defensive. Before restoring a single file, Sift verifies that:

- the original path is still free;
- the moved destination still exists;
- the destination is still a regular file;
- the destination has not become a symlink;
- the destination has not become a directory.

If the live filesystem no longer matches those assumptions — something else moved into the original spot, or the file at the destination was itself replaced — Sift refuses that specific restore rather than risking an unsafe overwrite. This is checked per-file, so a partially-safe undo still restores everything it can.

## What undo does not cover

- **Trash actions are not restored by `sift undo`.** [Clean](/organizing/clean/) and content-aware duplicate handling (see [Rules](/configuration/rules/)) send files to the operating system trash — recovery for those is the OS trash's job, not Sift's.
- Undo reverses a specific recorded *operation*, not an arbitrary point in time. There's no "undo everything since yesterday."

## Next

[Explainability](/core-concepts/explainability/) covers `sift explain`, which answers "why" for a decision before or after the fact — undo answers "can this be safely reversed" for one that already happened.
