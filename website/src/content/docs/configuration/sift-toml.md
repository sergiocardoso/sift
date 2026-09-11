---
title: .sift.toml
description: The full shape of Sift's per-folder policy file, field by field.
---

```toml
version = 1

[organize]
strategy = "type"
unknown = "other"

[watch]
stability_seconds = 3

[[rules]]
enabled = true
priority = 100
pattern = "*.tmp"
action = "Trash"
description = "Trash all .tmp files"

[[rules]]
enabled = true
priority = 90
pattern = "*.zip"
action = "Move"
destination = "Archives"
description = "Move .zip to Archives"
```

## Top level

| Field | Type | Notes |
|---|---|---|
| `version` | integer | Must be `1` — the only version this Sift build supports. A file that doesn't specify `version` is treated as `1`. |

An unrecognized top-level key in `.sift.toml` is a hard error, not a silently-ignored typo — a config file that can drive automatic mutation is never allowed to have a field that quietly does nothing.

## `[organize]`

| Field | Type | Notes |
|---|---|---|
| `strategy` | string | `"type"` (default), `"date"`, `"audio"`, `"video"`, `"photos"`, or `"documents"`. See [Strategies](/strategies/type/). |
| `unknown` | string | `"other"` (default) or `"skip"` — what happens to a `type`-classified file with no recognized extension. |
| `template` | string | Required for `date`/`audio`/`video`/`photos`/`documents`; invalid for `type`. The destination pattern, e.g. `"{year}/{month}"` or `"{artist}/{album}"`. |
| `date_source` | string | Only valid with `strategy = "date"`. |

## `[watch]`

| Field | Type | Notes |
|---|---|---|
| `stability_seconds` | integer | `1`–`300`. How long a candidate file must sit unchanged before Watch organizes it. Defaults to `2.5` seconds if omitted. See [Stability Window](/watch/stability-window/). |

## `[[rules]]`

See [Rules](/configuration/rules/) for the full field-by-field reference, action semantics, and destination-safety rules, and [Rule Priority](/configuration/rule-priority/) for how conflicts between multiple matching rules resolve.

## Where Sift looks for this file

See [Configuration Lookup](/configuration/configuration-lookup/) for the exact resolution order (`local .sift.toml` → global config → built-in defaults), and how recursive organize and Watch each pick which file governs which subtree.

## Generating one

```bash
sift init ~/Downloads
sift init ~/Downloads --force   # overwrite an existing .sift.toml
```

## Validating one

```bash
sift config check ~/Downloads
sift config check ~/Downloads --json
```
