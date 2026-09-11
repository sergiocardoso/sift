---
title: Doctor
description: Read-only filesystem diagnostics for a directory.
---

`sift doctor` is a read-only filesystem diagnostic. It never mutates anything, regardless of flags.

```bash
sift doctor ~/Downloads
sift doctor ~/Downloads --recursive
sift doctor ~/Downloads --json
```

## What it reports

- software projects;
- protected directories;
- symlinks;
- hidden entries;
- sensitive-looking filenames;
- files larger than 100 MB;
- archives older than one year;
- known build/dependency output directories (`node_modules`, `target`, `.venv`).

```text
Issues found

⚠ SLACK_TOKEN.txt
  Sensitive-looking filename

⚠ my-project/
  Software project detected — protected

⚠ some-link
  Symlink

3 findings
```

The sensitive-filename check is filename-based only — Sift does not read file contents to produce it.

## Recursive

```bash
sift doctor ~/Downloads --recursive
```

Uses the same traversal boundaries as `organize --recursive` (see [Recursive Organization](/organizing/recursive-organization/)) — inspects eligible subdirectories, but reads no file contents either way.

## Related

- [Explain](/organizing/explain/) — a decision-focused read-only check for one specific file, instead of a directory-wide report.
