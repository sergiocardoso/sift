---
title: Security
description: Reporting vulnerabilities, and Sift's own security-relevant defaults.
---

## Reporting a vulnerability

Report vulnerabilities or security issues to the maintainer, via a [GitHub Issue](https://github.com/sergiocardoso/sift/issues) or a private [GitHub Security Advisory](https://github.com/sergiocardoso/sift/security/advisories) on the repository, rather than broadcasting details publicly ahead of a fix.

## Do not run on sensitive or untrusted folders

Sift's stated security guidance is direct about this: **do not run it on sensitive or untrusted folders.** Sift organizes and can move or trash files based on its classification and rule logic — treat it the way you would any tool with filesystem write access, not as a sandboxed or content-aware security boundary.

## Security-relevant defaults already in place

These are architectural guarantees documented elsewhere, repeated here because they're directly relevant to running Sift safely:

- **No `--apply`, no mutation** — every organize/clean/folders command previews first. See [Dry-run and --apply](/core-concepts/dry-run-and-apply/).
- **No silent overwrite, no automatic directory merge** — see [Safety Model](/core-concepts/safety-model/).
- **`install.sh` never uses `sudo`**, installs only to `~/.local/bin/sift`, and verifies a SHA256 checksum against the downloaded release before installing anything.
- **No cloud service and no AI are required** for core organization — the only network call anywhere in Sift is the opt-out update-availability check, which never sends any file or path data, only checks GitHub's public releases API. See [CLI Commands → Update notices](/reference/cli-commands/#update-notices).
- **Destination paths are validated against traversal** — a rule's `Move` destination can't escape the target directory via `..`, an absolute path, or a symlink. See [Rules](/configuration/rules/#destination-safety).

## Related

- [Contributing](/project/contributing/) — the general pull request path, for non-security issues.
