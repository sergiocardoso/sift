---
title: Rules
description: Move, Trash, and Skip rules — fields, matching, and destination safety.
---

Explicit `[[rules]]` in a `.sift.toml` always win over the active strategy for a matching file.

```text
explicit rule
     ↓
organize strategy
     ↓
built-in behavior
```

## Fields

```toml
[[rules]]
name = "Trash tmp files"      # optional — falls back to `pattern` for display
pattern = "*.tmp"
action = "Trash"               # Move, Trash, or Skip
destination = "Archives"       # required for Move, invalid for Trash/Skip
priority = 100                 # higher runs first
enabled = true
description = "Trash all .tmp files"  # optional, for humans
```

| Field | Required | Notes |
|---|---|---|
| `pattern` | yes | Glob matched against the filename. |
| `action` | yes | `"Move"`, `"Trash"`, or `"Skip"` — any other value is a config error at load time. |
| `destination` | only for `Move` | A relative path beneath the target directory. |
| `priority` | yes | Integer. Higher runs first; ties keep file order. See [Rule Priority](/configuration/rule-priority/). |
| `enabled` | no (defaults `false`) | A rule must be explicitly enabled to ever match. |
| `name` | no | Display label; falls back to `pattern` if blank. |
| `description` | no | Free text, shown in `explain`/`config check` output. |

## Actions

| Action | Meaning |
|---|---|
| `Move` | Move to a relative destination beneath the target directory. |
| `Trash` | Send the matched file to the OS trash when applied. |
| `Skip` | Explicitly leave the file untouched. |

## Destination safety

A `Move` rule's `destination` cannot escape the target directory: absolute paths, `..`, unsafe root components, and symlink escapes are all rejected — both when the config is loaded and again, independently, by the executor immediately before any mutation.

## Same-name collisions

A rule's `Move` goes through the exact same collision handling as strategy-based moves (see [Safety Model](/core-concepts/safety-model/#no-silent-overwrite)):

- an existing directory, symlink, or broken symlink at the destination always blocks the move outright;
- an existing regular file with **byte-identical** content makes the source a redundant duplicate — trashed, not moved;
- an existing regular file with **different** content still gets the source moved, just under a disambiguated name (`"name (1).ext"`).

## Related

- [Rule Priority](/configuration/rule-priority/) — how multiple enabled rules matching the same file are resolved.
- [.sift.toml](/configuration/sift-toml/) — the full file shape rules live inside.
