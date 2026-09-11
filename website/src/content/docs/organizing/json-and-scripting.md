---
title: JSON and Scripting
description: Machine-readable output for scripting against Sift.
---

Machine-readable JSON is available via `--json` on several command groups:

```text
scan
organize
folders
clean
doctor
explain
config check
watch list
watch status
```

```bash
sift organize ~/Downloads --json | jq .
```

## Update notices never corrupt JSON stdout

Sift's background update-availability notice (see [CLI Commands → Update notices](/reference/cli-commands/#update-notices)), when enabled, is always written to **stderr** — never stdout. Piping `--json` output to `jq` or a file is always safe, regardless of whether a new release happens to be available at that moment.

## Exit codes

A mutating command (`organize`, `clean`, `folders`) that completes with failures still exits non-zero, whether or not `--json` was passed — check the process exit code in scripts rather than parsing prose for "failed."

## Using dry-run output as a check

Because every mutating command is a dry-run unless `--apply` is passed (see [Dry-run and --apply](/core-concepts/dry-run-and-apply/)), the `--json` form without `--apply` is a safe way for a script to *inspect* what Sift would do without any risk of it doing it — useful for CI checks or pre-flight validation before a scheduled `--apply` run elsewhere.

## Related

- [CLI Commands reference](/reference/cli-commands/) — full flag list per command.
