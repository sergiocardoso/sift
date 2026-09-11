---
title: Recursive Organization
description: Organizing eligible nested directories, each in its own local context.
---

```bash
sift organize ~/Downloads --recursive
sift organize ~/Downloads --recursive --apply
```

Sift organizes eligible nested directories **in their own local context** — it never flattens a subfolder's files into the root's category folders.

Given:

```text
Downloads/
├── invoice.pdf
└── Client A/
    ├── proposal.pdf
    └── logo.png
```

Recursive organization produces:

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

`Client A/proposal.pdf` becomes `Client A/Documents/proposal.pdf` — it is **not** flattened into the root's `Documents/`.

## A subfolder's own `.sift.toml` takes over

If a subfolder has its own local `.sift.toml`, that config governs its own subtree entirely — strategy, rules, and `unknown` policy — instead of deferring to the recursion root's policy. See [Configuration Lookup](/configuration/configuration-lookup/).

## What stops the descent

Recursive traversal never enters:

- hidden entries;
- symlinks;
- [protected software projects](/core-concepts/protected-paths/);
- known build/dependency output directories (`node_modules`, `target`, `.venv`);
- Sift's own category directories, or a custom rule's `destination` — preventing re-entering a folder Sift already organized.

## Strategies that don't support recursion

`audio`, `video`, `photos`, and `documents` currently do not support `--recursive` — Sift refuses outright with a clear error rather than risking infinite nesting from free-text metadata values (an artist or camera name can't be structurally distinguished from a folder that already existed for another reason). Non-recursive organize and non-recursive Watch work normally with all six strategies. See [Strategies](/strategies/type/).

## Related

- [Watch → Recursive Watch](/watch/recursive-watch/) — the same boundaries, applied continuously.
