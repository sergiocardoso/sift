---
title: Introduction
description: What Sift is, and the ideas it's built around.
---

Sift is an open-source CLI for organizing messy directories **safely, predictably, and locally**.

It can preview changes, classify files, organize nested folders, diagnose risky entries, apply per-folder rules, keep history, undo successful moves, and continuously organize new files with **Sift Watch**.

```text
preview → understand → apply → undo
                         ↓
                       watch
```

No cloud required. No AI required. No silent filesystem changes.

## What makes Sift different

- **Predictable.** Classification is deterministic — filenames, extensions, metadata, and explicit rules. The same input always produces the same plan.
- **Safe by default.** Every command is a dry-run until you add `--apply`. Existing destinations are never silently overwritten, and directory trees are never automatically merged.
- **Explainable.** `sift explain <file>` shows exactly why a file was (or would be) classified, skipped, protected, or moved.
- **Reversible where possible.** Successful moves are recorded in history and can be undone with `sift undo`. (Trash-based cleanup isn't — see [Clean](/organizing/clean/).)
- **Local-first.** Core organization happens entirely on your machine.
- **Explicit authorization for automation.** Watch only mutates a folder automatically after you pass `--auto-apply`, and only for files that arrive from that point on.

## Deliberately conservative

Sift is careful around important data by design:

- software projects (anything with `.git`, `Cargo.toml`, `package.json`, `pyproject.toml`, or `pubspec.yaml`) are protected;
- hidden entries are left alone;
- symlinks are not followed for organization decisions;
- existing destinations are never silently overwritten;
- directory trees are not automatically merged;
- unsafe destination paths are rejected;
- cleanup uses the operating system trash, never permanent deletion;
- live filesystem assumptions are revalidated immediately before mutation.

## Where to go next

- [Installation](/getting-started/installation/) to get the `sift` binary.
- [Quick Start](/getting-started/quick-start/) for a copy-pasteable tour of the core commands.
- [Your First Organize](/getting-started/first-organize/) for a walkthrough of one real dry-run → apply → undo cycle.
- [Safety Model](/core-concepts/safety-model/) if you want the full list of guarantees before you point Sift at anything important.
