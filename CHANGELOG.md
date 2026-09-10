# Changelog

## [v0.6.0] - 2026-09-10
- Recursive organize/watch now lets a subfolder's own local `.sift.toml` take over its own subtree (strategy, rules, `unknown` policy — everything) instead of always deferring to the recursion root's resolved policy for every file underneath it, no matter how deep. Applies to `sift organize --recursive`, `sift-tray`'s "Reapply now", and a running recursive watch (a subfolder's own config now hot-reloads the same way the root's already did). A subfolder governed by a strategy that doesn't support recursion on its own (`audio`/`video`/`photos`/`documents`) still organizes its own direct files under that strategy; it just never descends into its own children.
- Fixed recursive organize/watch re-entering and reclassifying a file it (or a nested config) already placed in a custom `[[rules]]` destination directory (e.g. `destination = "Invoices"`) a second time — previously only the nine built-in category names (`Documents`, `Images`, ...) were protected from this; custom destinations now are too.

## [v0.5.0] - 2026-09-10
- `sift-tray` gained two new per-watch controls: a "Recursive" checkbox (toggles `--recursive` scope after a watch is already registered, taking effect live on the daemon's next reconcile — no pause/resume needed) and "Reapply now" (runs one `sift organize --apply` pass on demand). Fixed the daemon's `reconcile` to actually rebuild a root's monitor when `recursive` changes while running (it previously kept the stale value silently). Both controls are new library functions (`watch::cmd_watch_set_recursive`, reused `planner::cmd_organize`) — the CLI doesn't expose a subcommand for either yet.
- `install.sh` now offers to install `sift-tray` too (when a prebuilt archive exists for the detected platform), interactively and opt-in only — same "never silently, never non-interactively" rule as the existing ffmpeg offer. Doesn't affect a non-interactive `curl | sh` install, which still installs only `sift`.

## [v0.4.0] - 2026-09-10
- `sift-tray`'s "Add folder…" now runs one `sift organize --apply` pass on the folder's pre-existing files before starting the watch, instead of leaving them untouched until something new lands (which is still exactly what `sift watch add` on the CLI does — this only changes the tray's one-click flow).

## [v0.3.0] - 2026-09-10
- Any command may now print a one-line, best-effort notice (to stderr, never stdout/`--json`) when a newer `sift` release is available on GitHub. Checked in a fully detached background process, at most once every 24 hours, via `curl` (never blocks or slows down the command that triggered it). Set `SIFT_NO_UPDATE_CHECK` to disable entirely.

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

