---
title: Organize Files
description: Building and applying a deterministic organization plan for a directory.
---

`sift organize` builds a deterministic plan for a directory's files and, with `--apply`, executes it.

```bash
# Current directory
sift

# Another directory
sift ~/Downloads

# Explicit command
sift organize ~/Downloads

# Apply after reviewing
sift organize ~/Downloads --apply
```

Note that bare `sift` and `sift <path>` are shorthand for a dry-run `organize` — there's no separate "default" command.

## Built-in categories

```text
Documents/
Images/
Audio/
Video/
Archives/
3D/
Code/
Data/
Other/
```

Unknown ordinary files go to `Other/` by default. See [Classification](/core-concepts/classification/) for the full extension table, and [Configuration → Overview](/configuration/overview/) to change `unknown` to `skip` instead.

## Verbose output

```bash
sift organize ~/Downloads --apply --verbose
```

Lists every skipped entry individually instead of collapsing large lists into a count.

## JSON output

```bash
sift organize ~/Downloads --json
```

See [JSON and Scripting](/organizing/json-and-scripting/).

## Related

- [Recursive Organization](/organizing/recursive-organization/) — organizing eligible subdirectories in place.
- [Organize Folders](/organizing/organize-folders/) — moving whole child folders instead of individual files.
- [Strategies](/strategies/type/) — classifying by date or metadata instead of extension.
