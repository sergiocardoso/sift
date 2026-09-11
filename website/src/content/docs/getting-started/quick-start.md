---
title: Quick Start
description: A copy-pasteable tour of Sift's core commands.
---

```bash
# Safe preview of the current directory
sift

# Safe preview of another directory
sift ~/Downloads

# Inspect entries (read-only, never mutates)
sift scan ~/Downloads

# Preview an organization plan
sift organize ~/Downloads

# Apply it
sift organize ~/Downloads --apply

# Organize eligible subdirectories too, each in its own local context
sift organize ~/Downloads --recursive

# Classify and move whole immediate child folders
sift folders ~/Downloads

# Report-only diagnostics: large files, stale archives, sensitive names
sift doctor ~/Downloads

# Preview junk cleanup (.tmp/.swp/.swo)
sift clean ~/Downloads

# Apply cleanup
sift clean ~/Downloads --apply

# Explain exactly what organize would do to one file, and why
sift explain ~/Downloads/invoice.pdf

# Validate the effective .sift.toml policy for a directory
sift config check ~/Downloads

# List past operations
sift history

# Reverse a successful move
sift undo hist-<operation-id>
```

Every command above that can mutate the filesystem (`organize`, `clean`, `folders`) is a dry-run unless you add `--apply`. Nothing needs `sudo`, nothing touches the network, and nothing runs in the background unless you explicitly start a [Watch](/watch/overview/).

For the full command surface with every flag, see the [CLI Commands reference](/reference/cli-commands/).

## Next

[Your First Organize](/getting-started/first-organize/) walks through one complete preview → apply → undo cycle against a real, messy directory.
