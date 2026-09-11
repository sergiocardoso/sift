---
title: File Classification
description: The complete extension-to-category table used by the type strategy.
---

The built-in `type` strategy classifies by extension only, case-insensitively, and never reads file contents.

| Category | Destination | Extensions |
|---|---|---|
| Images | `Images/` | jpg, jpeg, png, gif, webp, svg, bmp, tiff, tif, heic, heif, avif, ico |
| 3D | `3D/` | stl, obj, 3mf, step, stp, blend, blend1, fbx, glb, gltf, dae, ply |
| Code | `Code/` | js, jsx, mjs, cjs, ts, tsx, py, rs, go, php, dart, sh, bash, zsh, fish, lua, java, c, h, cc, cpp, cxx, hpp, cs, rb, swift, kt, kts, scala, vue, svelte |
| Data | `Data/` | json, jsonl, yaml, yml, toml, csv, tsv, xml, sql, sqlite, sqlite3, db, parquet, ndjson |
| Documents | `Documents/` | pdf, txt, md, markdown, rtf, doc, docx, odt, xls, xlsx, ods, ppt, pptx, odp, epub, mobi |
| Audio | `Audio/` | mp3, wav, flac, aac, m4a, ogg, opus, wma |
| Video | `Video/` | mp4, mov, mkv, avi, webm, m4v, mpg, mpeg, wmv |
| Archives | `Archives/` | zip, rar, 7z, tar, gz, bz2, xz, tgz, tbz2, txz |
| Junk | trash candidate (`sift clean`) | tmp, swp, swo |
| Other | `Other/` | any file with no recognized extension |

## What never gets classified by extension

- A directory, a symlink, or an already-protected entry is never assigned one of the categories above.
- A file inside a [protected software project](/core-concepts/protected-paths/) is never reached by classification at all.

## Changing the fallback for unrecognized extensions

```toml
[organize]
unknown = "skip"   # default is "other"
```

See [Strategies → Type](/strategies/type/) and [.sift.toml](/configuration/sift-toml/).

## Metadata-based classification

The `date`, `audio`, `video`, `photos`, and `documents` strategies classify by metadata instead of extension — see [Strategies](/strategies/type/).
