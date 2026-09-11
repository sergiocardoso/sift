---
title: Dry-run and --apply
description: Why every mutating command previews first, and how --apply works.
---

Every Sift command that can change the filesystem — `organize`, `clean`, `folders` — runs as a dry-run by default:

```text
no --apply
→ no filesystem mutation
```

Running the bare command only builds and prints a plan. Nothing moves, nothing gets trashed, and nothing is created.

```bash
sift organize ~/Downloads        # preview only
sift organize ~/Downloads --apply # actually move files
```

The same shape applies to `clean` and `folders`:

```bash
sift clean ~/Downloads
sift clean ~/Downloads --apply

sift folders ~/Downloads
sift folders ~/Downloads --apply
```

## Why this matters

- You can inspect a plan before committing to it, on directories you don't fully trust or haven't looked at in a while.
- Scripts and CI can safely call the dry-run form to *check* what Sift would do, without any risk of it doing it.
- It removes an entire class of "I didn't expect that to move" surprises: if a command didn't print `Applied successfully`, nothing happened.

## Read-only commands

Some commands are read-only regardless of flags — there's no `--apply` for them because they never mutate:

- `sift scan`
- `sift doctor`
- `sift explain`
- `sift config check`
- `sift history`

`sift undo` and `sift init` do mutate (that's their entire purpose), but each has its own explicit confirmation shape rather than a dry-run/`--apply` pair — see [History and Undo](/core-concepts/history-and-undo/).

## Next

[Safety Model](/core-concepts/safety-model/) covers everything else that has to hold true even after you do pass `--apply`.
