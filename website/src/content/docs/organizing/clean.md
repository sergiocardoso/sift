---
title: Clean
description: Finding and trashing high-confidence junk files.
---

`sift clean` finds high-confidence junk and sends it to the operating system trash — only when explicitly applied.

```bash
# Preview
sift clean ~/Downloads

# Apply
sift clean ~/Downloads --apply
```

## Built-in junk candidates

```text
*.tmp
*.swp
*.swo
```

## Why there's no `--recursive`

This is intentional. Cleanup carries a higher destructive risk than moving files, so Sift deliberately keeps its surface narrower than `organize`'s — there is no recursive variant of `clean` today.

:::caution
Trash actions are **not** restored by `sift undo`. Recovery belongs to the operating system trash, not Sift's history. See [History and Undo](/core-concepts/history-and-undo/).
:::

## Listing what would be skipped

```bash
sift clean ~/Downloads --verbose
```

Lists every skipped entry individually instead of a single collapsed count.

## Related

- [Doctor](/organizing/doctor/) — read-only diagnostics (large files, stale archives) that `clean` does not act on.
- [Rules](/configuration/rules/) — a `.sift.toml` rule can also `Trash` a custom pattern, independent of `clean`'s built-in candidates.
