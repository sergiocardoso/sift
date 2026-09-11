---
title: Recursive Watch
description: Watching eligible subdirectories continuously, with the same boundaries as recursive organize.
---

```bash
sift watch add ~/Inbox --auto-apply --recursive
```

Recursive Watch shares the same eligibility rules and traversal boundaries as [`organize --recursive`](/organizing/recursive-organization/): each eligible subdirectory is organized in its own local context, and Sift never enters hidden entries, symlinks, protected software projects, known build/dependency directories, or its own category directories.

## A subfolder's own `.sift.toml` takes over live

If a subfolder has its own local `.sift.toml`, that config governs its own subtree — and this is hot-reloaded the same way the root's own config is: editing a nested `.sift.toml` while the watch is running takes effect without restarting it. See [Configuration Lookup](/configuration/configuration-lookup/).

## Strategies that can't go recursive

`audio`, `video`, `photos`, and `documents` can't be used with `--recursive` — registering one is refused outright at `watch add` time. If a running watch's `.sift.toml` is *edited* afterward to switch from a recursive-capable strategy (`type`/`date`) to one of these four while still set to recursive, Sift detects it and **fails closed**: the watch is suspended rather than left running with an unsupported combination. See [Recursive Organization](/organizing/recursive-organization/) for why these four can't safely support recursion at all.

## Related

- [Register and Start](/watch/register-and-start/) — the `--recursive` flag in the full `add` command.
- [Strategies](/strategies/type/) — which strategies support recursion.
