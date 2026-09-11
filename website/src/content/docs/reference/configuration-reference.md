---
title: Configuration Reference
description: Every .sift.toml field, in one place.
---

Narrative documentation lives under [Configuration](/configuration/overview/) and [Strategies](/strategies/type/) — this page is the terse, field-by-field lookup.

## Top level

| Field | Type | Default | Notes |
|---|---|---|---|
| `version` | integer | `1` | Only `1` is supported by this build. |

## `[organize]`

| Field | Type | Default | Valid with |
|---|---|---|---|
| `strategy` | `"type"` \| `"date"` \| `"audio"` \| `"video"` \| `"photos"` \| `"documents"` | `"type"` | — |
| `unknown` | `"other"` \| `"skip"` | `"other"` | `type` only |
| `template` | string | — | `date`, `audio`, `video`, `photos`, `documents` (required); invalid for `type` |
| `date_source` | `"modified"` | `"modified"` | `date` only |

## `[watch]`

| Field | Type | Default when omitted | Valid range if set |
|---|---|---|---|
| `stability_seconds` | integer | 2.5 seconds | `1`–`300` |

## `[[rules]]` (repeatable)

| Field | Type | Required | Notes |
|---|---|---|---|
| `pattern` | string (glob) | yes | Matched against the filename. |
| `action` | `"Move"` \| `"Trash"` \| `"Skip"` | yes | Any other value is a load-time error. |
| `destination` | string (relative path) | only for `Move` | Cannot escape the target directory (`..`, absolute paths, and symlink escapes are rejected). |
| `priority` | integer | yes | Higher runs first; ties keep file order. |
| `enabled` | boolean | no (default `false`) | A disabled rule is never evaluated. |
| `name` | string | no | Display label; falls back to `pattern`. |
| `description` | string | no | Free text, shown in `explain`/`config check`. |

## Template placeholders by strategy

| Strategy | Placeholders |
|---|---|
| `date` | `{year}` `{month}` `{day}` (fixed-width digits) |
| `audio` | `{artist}` `{album}` `{album_artist}` `{genre}` `{track}` `{title}` `{year}` |
| `video` | `{resolution}` `{width}` `{height}` `{codec}` `{year}` `{duration}`\* `{fps}`\* |
| `photos` | `{camera}` `{year}` `{month}` `{day}` |
| `documents` | `{author}` `{title}` `{year}` `{month}` `{day}` |

\* `{duration}`/`{fps}` are only ever populated via the optional `ffprobe` backend — see [Video](/strategies/video/).

A file missing a field its template references is always **skipped** with a clear reason, never assigned a guessed/fallback value.

## Resolution order

```text
1. <directory>/.sift.toml
2. global config (~/.config/sift/config.toml on Linux)
3. built-in defaults
```

See [Configuration Lookup](/configuration/configuration-lookup/) for the recursive-subfolder exception.

## Unknown fields

Every table (`RawConfig`, `[organize]`, `[watch]`, each rule) rejects unrecognized keys as a hard error at load time — a typo in a file that can drive automatic mutation is never a silent no-op.
