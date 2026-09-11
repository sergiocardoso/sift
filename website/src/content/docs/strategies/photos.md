---
title: Photos
description: Organizing by EXIF camera and capture date metadata.
---

```toml
[organize]
strategy = "photos"
template = "{camera}/{year}/{month}"
```

## Placeholders

```text
{camera}
{year}
{month}
{day}
```

`{year}`/`{month}`/`{day}` come from the EXIF capture date (`DateTimeOriginal`) — never the file's own mtime.

`{camera}` combines the EXIF `Make` and `Model` fields, with deduplication: if `Model` already contains `Make` (common on several brands), only `Model` is used, avoiding a folder like `"Canon Canon EOS R5"`.

## Reading EXIF

Auto-detects JPEG, TIFF, HEIF/HEIC, PNG, and WebP directly from file bytes — no external tool required.

## No location placeholder

There is deliberately no `{gps}` or location placeholder. Embedding capture coordinates in a folder name is an easy way to leak where a photo was taken without noticing.

## Missing metadata is a skip, never a guess

A file with no EXIF at all (common for PNG/WebP, or a JPEG with metadata stripped) is treated the same as any file missing the specific field its template needs — skipped with a clear reason, never given an invented value.

## Recursion

`photos` does not support `--recursive` — see [Recursive Organization](/organizing/recursive-organization/) for why (shared by all four metadata strategies).

## Related

- [Audio](/strategies/audio/), [Video](/strategies/video/), [Documents](/strategies/documents/) — the other metadata-driven strategies.
