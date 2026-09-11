---
title: Rule Priority
description: How Sift resolves multiple rules matching the same file.
---

Rules are evaluated in descending `priority` order — highest first. **The first enabled matching rule wins**, and no other rule is considered after that for the same file.

```toml
[[rules]]
enabled = true
priority = 100
pattern = "*.tmp"
action = "Trash"

[[rules]]
enabled = true
priority = 90
pattern = "*.zip"
action = "Move"
destination = "Archives"
```

Given both rules above, a file matching `*.tmp` is always trashed by the first rule — its higher `priority` (100 > 90) means it's checked first, and once a rule matches, evaluation stops for that file.

## Ties

Rules with equal `priority` keep their original file order — the one written first in `.sift.toml` is checked first.

## Disabled rules are invisible

A rule with `enabled = false` (the default if omitted) is never evaluated at all — not even as a lower-priority fallback. It's as if the rule weren't in the file.

## Rules beat the strategy, always

Regardless of priority values between rules, **any** enabled matching rule takes precedence over the directory's `strategy` (`type`, `date`, `audio`, ...) for that file. The strategy only ever applies to a file that no enabled rule matches.

## Checking what would actually match

```bash
sift explain ~/Downloads/archive.zip
```

Shows the exact matching rule (if any) instead of the strategy's classification — see [Explainability](/core-concepts/explainability/).

## Related

- [Rules](/configuration/rules/) — full field reference and destination safety.
