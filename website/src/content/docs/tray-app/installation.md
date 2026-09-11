---
title: Installation
description: Getting the sift-tray binary, from a release or from source.
---

## From a GitHub Release

Each [GitHub Release](https://github.com/sergiocardoso/sift/releases) includes a prebuilt `sift-tray` archive alongside `sift` itself, for:

```text
Linux x86_64
macOS x86_64
macOS aarch64
```

There's no prebuilt Linux aarch64 archive yet, and no Windows build at all — see [Platform Support](/reference/platform-support/).

Download and extract it next to your installed `sift` binary (typically `~/.local/bin`) — the same as any other release archive. `install.sh` doesn't fetch it automatically yet, so this is a manual step for now.

## Build from source

```bash
cargo build -p sift-tray --release
./target/release/sift-tray
```

On Debian/Ubuntu, the GTK/AppIndicator headers it links against:

```bash
sudo apt-get install libgtk-3-dev libayatana-appindicator3-dev libxdo-dev
```

## Running it

```bash
sift watch tray
```

Launches `sift-tray` explicitly and reports exactly what happened — already running, started, not installed, or spawned but never came up. See [Auto-launch Behavior](/tray-app/auto-launch-behavior/) for the difference between this and the automatic launch `watch start`/`resume` already attempt on their own.

## Related

- [Overview](/tray-app/overview/) — what the tray app does once it's running.
