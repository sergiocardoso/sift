# Changelog

## [v0.2.2] - 2026-09-10
- Fixed `release.yml`: the Linux `sift-tray` build was also missing `libxdo-dev` (needed by the native folder picker), so it failed to link even after the v0.2.1 package fix. Documented the full Linux build dependency list. No other changes.

## [v0.2.1] - 2026-09-10
- Fixed `release.yml`: the new `sift-tray` release job was building against the wrong package (root `sift` instead of the `sift-tray` workspace member), so no `sift-tray` archives were actually published on v0.2.0. No other changes.

## [v0.2.0] - 2026-09-10
- Relicensed from MIT to Apache License, Version 2.0.
- `sift watch start`/`resume` now try (best-effort, silently) to auto-launch the optional `sift-tray` app if it's installed and not already running.
- Colorized `--help` output and grouped the `EXAMPLES` section.
- `sift-tray` is now published as a prebuilt binary in GitHub Releases for Linux (x86_64) and macOS (x86_64/aarch64).
- Fixed a spurious `curl: (23) Failure writing output to destination` message in `install.sh`.

## [v0.1.0] - 2026-09-07
- Initial public MVP

