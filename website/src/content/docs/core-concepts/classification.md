---
title: Classification
description: How Sift decides what category a file belongs to.
---

Sift's built-in `type` strategy classifies files deterministically, by extension, case-insensitively. It never reads file contents to do this — that's exclusive to the [metadata strategies](/strategies/type/) (`audio`, `video`, `photos`, `documents`).

## Categories

| Category | Destination | Examples |
|---|---|---|
| Documents | `Documents/` | pdf, txt, md, docx, xlsx, pptx, epub |
| Images | `Images/` | jpg, png, webp, svg, heic, avif |
| Audio | `Audio/` | mp3, wav, flac, m4a, ogg, opus |
| Video | `Video/` | mp4, mov, mkv, webm, m4v |
| Archives | `Archives/` | zip, rar, 7z, tar, gz, xz |
| 3D | `3D/` | stl, obj, 3mf, step, blend, fbx, glb |
| Code | `Code/` | js, ts, py, rs, go, dart, sh, java |
| Data | `Data/` | json, yaml, toml, csv, xml, sql, sqlite |
| Other | `Other/` | ordinary unmatched files |
| Junk | trash candidate | tmp, swp, swo (see [Clean](/organizing/clean/)) |

An ordinary file with an unrecognized extension falls back to `Other/` rather than being skipped — there's always a safe place to put it.

## What never gets classified this way

- Directories, symlinks, and already-protected entries are never classified into a category — they're left as-is (or handled separately, e.g. by [`sift folders`](/organizing/organize-folders/) for whole-folder moves).
- A file inside a [protected software project](/core-concepts/protected-paths/) is never reached at all.

## Overriding classification

Explicit [rules](/configuration/rules/) in a `.sift.toml` always win over the strategy's classification for a matching file. The strategy itself can also be changed per-directory — see [Strategies](/strategies/type/) for `date`, `audio`, `video`, `photos`, and `documents`.

## Checking a specific decision

```bash
sift explain ~/Downloads/invoice.pdf
```

shows exactly which category (or rule) applied, and why. See [Explainability](/core-concepts/explainability/).
