---
title: Installation
description: Install the sift CLI via the install script, or build it from source.
---

## Install script

```bash
curl -fsSL https://raw.githubusercontent.com/sergiocardoso/sift/main/install.sh | sh
```

This downloads the latest prebuilt release for your OS/architecture, verifies its SHA256 checksum, and installs `sift` to:

```text
~/.local/bin/sift
```

It never uses `sudo`. Prebuilt CLI releases currently target Linux and macOS on x86_64/aarch64 — see [Platform Support](/reference/platform-support/).

The installer also checks whether `ffprobe` is on your `PATH`. It's entirely optional (the [`video`](/strategies/video/) strategy works without it), but if it's missing, the installer *tells* you the right command for your system and offers to run it — only in an interactive terminal, only after you confirm. It never installs anything silently or non-interactively.

## Build from source

```bash
git clone https://github.com/sergiocardoso/sift.git
cd sift
cargo build --release
./target/release/sift --help
```

Or install it onto your `PATH` directly:

```bash
cargo install --path .
sift --help
```

## Optional: the tray app

`sift-tray` is a separate, optional binary — a small desktop UI for Linux and macOS that lists watched folders. It isn't installed by `install.sh` automatically yet. See [Tray App → Installation](/tray-app/installation/) for how to get it.

## Verifying the install

```bash
sift --help
```

If you'd rather not have Sift check GitHub for newer releases in the background, set:

```bash
export SIFT_NO_UPDATE_CHECK=1
```

See [Update notices](/reference/cli-commands/#update-notices) for exactly what that check does and doesn't do.

## Next

[Quick Start](/getting-started/quick-start/) walks through the core commands against a real directory.
