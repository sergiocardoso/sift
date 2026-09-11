---
title: Explain
description: Read-only, per-file explanation of what organize would do, and why.
---

```bash
sift explain ~/Downloads/invoice.pdf
```

`sift explain` never mutates anything — it's the read-only counterpart to `organize`, scoped to exactly one file. See [Explainability](/core-concepts/explainability/) for the full output shape (policy, strategy, classification, destination, decision, and every safety check) and how it behaves with a matching rule.

## Choosing the policy root

```bash
sift explain ~/Downloads/invoice.pdf --root ~/Downloads
```

`--root` is the directory whose `.sift.toml` (or global/default policy) applies. It defaults to the file's own parent directory and is never discovered by walking further up the tree — see [Configuration Lookup](/configuration/configuration-lookup/).

## JSON

```bash
sift explain ~/Downloads/invoice.pdf --json
```

See [JSON and Scripting](/organizing/json-and-scripting/).

## Related

- [Doctor](/organizing/doctor/) — directory-wide, finding-focused diagnostics instead of one file's decision.
- [Configuration → Overview](/configuration/overview/) — validating a whole policy instead of one file's outcome (`sift config check`).
