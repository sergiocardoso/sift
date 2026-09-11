---
title: Safety Model
description: Sift's guarantees around mutation, overwrite, and protected paths.
---

Safety is part of Sift's architecture, not an optional mode.

## Dry-run first

```bash
sift organize ~/Downloads
```

shows the plan. Nothing moves until:

```bash
sift organize ~/Downloads --apply
```

This applies uniformly across `organize`, `clean`, and `folders`.

## No silent overwrite

Sift never overwrites an existing destination. What happens next depends on what's occupying it and, for a regular file, whether its content is actually the same as the file being organized:

- **destination is a directory, a symlink, or a broken symlink** — the move is always skipped outright, no exceptions;
- **destination is an existing regular file with byte-identical content** — the source is treated as a redundant duplicate and sent to the trash, rather than left cluttering the folder;
- **destination is an existing regular file with different content** — the source is still organized, just under a disambiguated name (`"name (1).ext"`) instead of being skipped or silently overwriting the other file.

Every one of those outcomes is always shown explicitly in `organize`'s output (dry-run and `--apply`) — never folded into a plain skip count. See [Rules](/configuration/rules/#same-name-collisions) for the same behavior applied to a rule's `Move` action.

## Symlink safety

Sift uses no-follow metadata for every safety-sensitive filesystem check. Symlinks are treated as occupied/protected entries, never followed as if they were the real target.

## No automatic directory merge

Sift never merges directory trees just because two directory names match.

## Protected software projects

Running organization against a recognized software project protects its contents instead of dismantling the project into categories. See [Protected Paths](/core-concepts/protected-paths/).

## Live revalidation

Planning-time assumptions are checked again immediately before mutation. If something changed on disk between the plan and the apply (a new file appeared, a destination became a symlink), Sift refuses rather than acting on stale information.

## No copy-delete fallback

If a rename can't be performed safely, Sift does not silently fall back to copy-then-delete.

## Trash, not permanent deletion

[Clean](/organizing/clean/) sends files to the operating system trash. It never deletes permanently, and it is intentionally the only command with no `--recursive` option — cleanup carries more destructive risk than moving files, so Sift keeps that surface narrower.

:::note
Trash actions are **not** restored by `sift undo` — recovery for those belongs to the operating system trash. Undo only reverses recorded `organize`/`folders` moves. See [History and Undo](/core-concepts/history-and-undo/).
:::
