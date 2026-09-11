---
title: Configuration Lookup
description: Exactly which .sift.toml (or global config) applies, and in what order.
---

For a command targeting a directory, Sift resolves the effective policy in this order:

```text
1. <directory>/.sift.toml, if present
2. the global config file, if present
3. built-in defaults
```

A missing file at a given level just means "try the next level" — it's not an error. A file that **exists but fails to parse or validate is always a hard error**, at every level: a broken local `.sift.toml` is never silently patched over with the global config or defaults, and a broken global config is never silently replaced by the built-in defaults either. Configuration that can drive automatic mutation fails closed, not open.

## The global config file

On a typical Linux system:

```text
~/.config/sift/config.toml
```

Same file shape as a local `.sift.toml` — it's the fallback policy for any directory that doesn't have its own.

## No arbitrary parent-directory walking for the command's own root

For the directory a command actually targets, Sift **never** walks upward looking for a `.sift.toml` in a parent directory — only that exact directory's own file, then the global config, then defaults.

## The recursive exception: subfolders can override

This "no walking up" rule is about the command's *own* root only. Inside a recursive operation (`organize --recursive`, recursive Watch), a **subfolder** strictly under that root is checked for its own local `.sift.toml` by walking upward *from the subfolder toward the root* (never past the root, and never re-reading the root's own policy) — the closest `.sift.toml` found wins for that subfolder's subtree. See [Recursive Organization](/organizing/recursive-organization/) for what this looks like in practice.

If no subfolder along that path has its own `.sift.toml`, the root's already-resolved policy governs it, exactly as before.

## Which policy Watch uses

A running Watch resolves its configuration from the registered watch's root, the same way manual `organize` does — see [Watch → Overview](/watch/overview/) for how a `.sift.toml` edit is picked up live.

## Inspecting the effective policy

```bash
sift config check ~/Downloads
sift config check ~/Downloads --json
```

Shows exactly which policy applies and validates it, without touching the filesystem.
