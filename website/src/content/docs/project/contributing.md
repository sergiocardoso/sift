---
title: Contributing
description: How to propose changes to Sift.
---

Contributions are welcome, via fork and pull request.

## Before opening a pull request

Run the full local quality gate:

```bash
cargo fmt --check
cargo check --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

This is the same gate the project's CI runs.

## Building

```bash
cargo build --release              # the sift CLI
cargo build -p sift-tray --release # the optional tray app
```

## Breaking changes and major proposals

Discuss breaking changes or major proposals in an Issue first, before investing in a pull request.

## Security

Security issues have a separate, narrower reporting path — see [Security](/project/security/) rather than a public Issue.
