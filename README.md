# Sift

Sift is a local-first CLI for safely organizing and cleaning directories/files.

- **Safe by default**: every command is a dry-run unless you pass `--apply`.
- **Deterministic**: files are classified by extension and optional config rules, not AI.
- **Conservative**: hidden files, symlinks, and software project directories
  (anything containing `.git`, `Cargo.toml`, `package.json`, `pyproject.toml`,
  or `pubspec.yaml`) are never mutated.
- **Undoable**: successful moves can be reversed with `sift undo`.
- **No silent deletes**: cleanup sends files to the system trash, never `rm`.

## Usage

```sh
sift                     # dry-run organize preview of the current directory
sift scan [path]         # list what's in a directory (default path: .)
sift organize [path]     # preview an organize plan
sift organize [path] --apply
sift clean [path]        # preview a cleanup plan (high-confidence junk only)
sift clean [path] --apply
sift doctor [path]       # report-only: large files, stale archives, etc.
sift history             # list past operations
sift undo <operation-id> # reverse a successful move
sift init [path]         # write a starter .sift.toml
```

Add `--json` to `scan`, `organize`, `clean`, or `doctor` for machine-readable output.

## Configuration

Drop a `.sift.toml` in the target directory to add custom rules (see `sift init`
for a starter file). Rules are evaluated by descending `priority`; the first
matching rule wins. `Move` rule destinations must be a plain relative path
under the target directory — absolute paths, `..`, and symlink escapes are
rejected.

See CONTRIBUTING.md, SECURITY.md, CHANGELOG.md.

## License

MIT License. Copyright (c) 2026 Sérgio Cardoso
