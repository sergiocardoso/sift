---
title: Explainability
description: Asking Sift why it would make, or made, a specific decision.
---

`sift explain` answers exactly one question — why would Sift do *this* to *this one file* — without touching anything.

```bash
sift explain ~/Downloads/invoice.pdf
```

```text
Policy
  built-in defaults

Strategy
  type

Classification
  .pdf → Document

Destination
  ~/Downloads/Documents/invoice.pdf

Decision
  MOVE
  Document

Safety
  ✓ regular file
  ✓ not protected
  ✓ destination available
  ✓ no symlink ancestor
  ✓ no collision

No changes were made.
```

## What it shows

- **Policy**: whether a local `.sift.toml`, a global config, or built-in defaults are in effect for this file.
- **Strategy**: `type`, `date`, `audio`, `video`, `photos`, or `documents`.
- **Classification**: the category (or metadata match) the file resolved to.
- **Destination**: exactly where it would land.
- **Decision**: `MOVE`, `TRASH`, or `SKIP`, and why.
- **Safety**: every safety check that passed (or the one that didn't, if the decision is `SKIP`).

## With a custom rule

If a `.sift.toml` rule matches the file, `explain` shows the matching rule (pattern, priority, action) and the reason instead of the strategy's classification — rules always take precedence. See [Rules](/configuration/rules/) and [Rule Priority](/configuration/rule-priority/).

## Choosing which policy applies

```bash
sift explain ~/Downloads/invoice.pdf --root ~/Downloads
```

`--root` selects which `.sift.toml` (or global/default policy) applies. It defaults to the file's own parent directory, and is never discovered by walking further up — see [Configuration Lookup](/configuration/configuration-lookup/).

## Scripting

```bash
sift explain ~/Downloads/invoice.pdf --json
```

prints the same explanation as machine-readable JSON. See [JSON and Scripting](/organizing/json-and-scripting/).
