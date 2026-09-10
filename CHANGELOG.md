# Changelog

## [v0.2.0] - 2026-09-10
- Relicensed from MIT to Apache License, Version 2.0.
- `sift watch start`/`resume` now try (best-effort, silently) to auto-launch the optional `sift-tray` app if it's installed and not already running.
- Colorized `--help` output and grouped the `EXAMPLES` section.
- `sift-tray` is now published as a prebuilt binary in GitHub Releases for Linux (x86_64) and macOS (x86_64/aarch64).
- Fixed a spurious `curl: (23) Failure writing output to destination` message in `install.sh`.

## [v0.1.0] - 2026-09-07
- Initial public MVP

