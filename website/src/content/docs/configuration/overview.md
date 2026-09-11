---
title: Overview
description: How per-folder configuration fits together in Sift.
---

Every directory can have its own `.sift.toml`, giving it its own strategy, unknown-file handling, Watch stability window, and declarative rules — independent of every other directory.

```text
~/Music/.sift.toml
~/Pictures/.sift.toml
~/Documents/.sift.toml
~/Downloads/.sift.toml
```

## Generating a starter config

```bash
sift init ~/Downloads
```

Writes a starter, canonical `.sift.toml` with example rules. Use `--force` to overwrite an existing one.

## Validating a config

```bash
sift config check ~/Downloads
```

Validates the *effective* policy for a directory — local `.sift.toml`, global config, or built-in defaults, whichever applies — without touching the filesystem.

## What lives in `.sift.toml`

- **`strategy`** — how files are classified: `type` (default), `date`, `audio`, `video`, `photos`, or `documents`. See [Strategies](/strategies/type/).
- **`unknown`** — what happens to a file `type` doesn't recognize: `other` (default) or `skip`.
- **`template`** — the destination pattern for metadata-driven strategies.
- **`[watch].stability_seconds`** — how long a file must sit unchanged before Watch organizes it. See [Stability Window](/watch/stability-window/).
- **`[[rules]]`** — explicit, declarative overrides that always win over the strategy. See [Rules](/configuration/rules/).

## What `.sift.toml` never does

Saving a `.sift.toml` never itself authorizes or starts any mutation — it describes *how* a directory should be organized, not *whether* automatic organization is currently running. That's Watch's job, and Watch requires its own explicit `--auto-apply` and `sift watch start`. See [Watch → Overview](/watch/overview/).

## Pages in this section

- [.sift.toml](/configuration/sift-toml/) — the full file shape, field by field.
- [Rules](/configuration/rules/) — `Move` / `Trash` / `Skip`, and destination safety.
- [Rule Priority](/configuration/rule-priority/) — how conflicts between rules resolve.
- [Configuration Lookup](/configuration/configuration-lookup/) — exactly which file wins, and in what order.
