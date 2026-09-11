---
title: Date
description: Organizing by filesystem modification time.
---

```toml
[organize]
strategy = "date"
template = "{year}/{month}"
```

## Source

Reads the file's filesystem modification time (`mtime`) via no-follow metadata — never the file's own internal metadata. `date_source = "modified"` is the only supported value today, and can be omitted (it's the default).

## Placeholders

```text
{year}
{month}
{day}
```

Each renders as fixed-width digits (`{year}` → 4 digits, `{month}`/`{day}` → 2 digits). That fixed width is what lets recursive organize safely recognize a folder it already generated (`2026/09/`) and avoid nesting into it again on a second run — see [Recursive Organization](/organizing/recursive-organization/).

## Recursion

`date` fully supports `--recursive`, same as [`type`](/strategies/type/) — the only two strategies that do.

## Related

- [.sift.toml](/configuration/sift-toml/) — the `template` field in context with the rest of the file.
- [Audio](/strategies/audio/), [Video](/strategies/video/), [Photos](/strategies/photos/), [Documents](/strategies/documents/) — free-text metadata strategies, which don't support `--recursive` for exactly the reason above (their values aren't fixed-width).
