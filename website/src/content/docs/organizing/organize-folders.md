---
title: Organize Folders
description: Moving whole immediate child folders based on what they contain.
---

`sift folders` reasons about immediate child folders from their *contents*, never from the folder name alone, and moves whole folders intact — it never dismantles one file-by-file. That makes it distinct from `organize --recursive`, which organizes files *inside* a folder but never moves the folder itself.

```bash
sift folders ~/Downloads
sift folders ~/Downloads --apply
```

```text
Plan
  3 folders to move
  1 suggestion
  1 protected
  1 uncertain

Move
  client-files/ → Documents/  100%
  print-jobs/   → 3D/         100%
  vacation/     → Images/     100%

Suggestions
  artwork/      → Images/      67% medium confidence

Protected
  my-app/       software project

Uncertain
  misc/         mixed content
```

## Confidence buckets

```text
high confidence   → planned, moved with --apply
medium confidence → only ever suggested, never auto-moved
protected         → software project, always left alone
uncertain / mixed → always left alone
```

Only high-confidence folders are ever actually moved. Candidate selection is always exactly one level of immediate children — there is no `--recursive` flag for this command.

## Removing duplicate files across similarly-named folders

```bash
sift folders ~/Downloads --remove-duplicates --apply
```

Additionally compares files inside folders whose *names* already look like duplicates of each other, and removes a file from every folder in the group except the alphabetically-first one — but only when that file is byte-for-byte identical to the one being kept. Removal always means sending to the system Trash (recoverable there, but **not** undoable via `sift undo`).

## Related

- [Organize Files](/organizing/organize-files/) — the file-level counterpart.
- [Protected Paths](/core-concepts/protected-paths/) — why `my-app/` above was left alone.
