---
title: Platform Support
description: What's supported on Linux, macOS, and Windows today.
---

## Linux

Supported by prebuilt CLI releases for x86_64 and aarch64.

`sift-tray` is currently released as a prebuilt binary for x86_64 only — there's no prebuilt Linux aarch64 tray archive yet, since it requires GTK/AppIndicator development headers for the target architecture that aren't available in the cross-compilation environment used for that release job. Building `sift-tray` from source on aarch64 Linux directly (not cross-compiling) is unaffected by this.

## macOS

Supported by prebuilt CLI releases for x86_64 and Apple Silicon (aarch64). `sift-tray` is released as a prebuilt binary for both architectures.

## Windows

There is currently no official Windows release, for either binary.

Most non-Watch functionality (`scan`, `organize`, `clean`, `doctor`, `history`, `undo`, `init`, `folders`, `config`, `explain`) is built on portable `std::fs` APIs and the cross-platform `trash` crate, and the whole workspace does cross-compile cleanly for `x86_64-pc-windows-gnu` — those commands are expected to work if built from source, though this hasn't been run on a real Windows machine and isn't covered by CI.

`sift watch` specifically does not work on Windows: the daemon's detached-process spawn (`watch::platform`) is intentionally Unix-only, and a native Windows build of it needs real process-detachment work (`CREATE_NEW_PROCESS_GROUP`/`DETACHED_PROCESS`) that hasn't been implemented or tested.

## Install script coverage

```bash
curl -fsSL https://raw.githubusercontent.com/sergiocardoso/sift/main/install.sh | sh
```

Covers Linux and macOS on x86_64/aarch64. It does not install `sift-tray` automatically yet — see [Tray App → Installation](/tray-app/installation/).
