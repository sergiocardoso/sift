---
title: Protected Paths
description: Software projects, build output, and Sift's own category directories are never dismantled.
---

## Software project roots

A directory containing any of these markers is treated as a software project root and protected outright — organize (recursive or not) never descends into it or reclassifies its contents:

```text
.git
Cargo.toml
package.json
pyproject.toml
pubspec.yaml
```

`sift folders` reflects the same rule: a child folder that's a software project is reported under **Protected**, never moved or even suggested.

## Traversal boundaries

Recursive traversal (`organize --recursive`, recursive Watch) also stops at known build/dependency output directories, without needing a project marker at that exact level:

```text
node_modules
target
.venv
```

## Sift's own category directories

Recursive traversal also avoids descending into the categories Sift itself creates (`Documents`, `Images`, `Audio`, `Video`, `Archives`, `3D`, `Code`, `Data`, `Other`), which prevents repeated nesting like:

```text
Documents/Documents/file.pdf
```

A custom rule `destination` (e.g. `destination = "Invoices"`) is protected from re-entry the same way — Sift never reclassifies a file it (or a nested `.sift.toml`) already placed there.

## Hidden entries and symlinks

Hidden entries (dotfiles/dotdirs) are left alone entirely. Symlinks are never followed for organization decisions — see [Safety Model](/core-concepts/safety-model/) for the full no-follow guarantee.

## Checking what's protected without applying anything

```bash
sift doctor ~/Downloads --recursive
```

reports software projects, symlinks, and other findings without touching the filesystem. See [Doctor](/organizing/doctor/).
