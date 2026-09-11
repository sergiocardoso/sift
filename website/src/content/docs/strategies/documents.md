---
title: Documents
description: Organizing PDFs and Office files by author and creation date.
---

```toml
[organize]
strategy = "documents"
template = "{author}/{year}"
```

## Placeholders

```text
{author}
{title}
{year}
{month}
{day}
```

Same placeholder names regardless of whether the file is a PDF or an Office document — Sift normalizes both into the same shape before the template ever sees it.

## Two formats, one strategy

- **PDF** — reads the document's `/Info` dictionary (`Author`, `Title`, `CreationDate`) directly.
- **Office (docx/xlsx/pptx)** — reads `docProps/core.xml` (the same part in any modern Office file) for `dc:creator`, `dc:title`, and `dcterms:created`.

Neither requires an external tool (no LibreOffice, no Office install).

## Missing metadata is a skip, never a guess

A PDF with no `/Info`, or an Office file missing `docProps/core.xml`, is valid but has no metadata — treated the same as any file missing the specific field its template needs: skipped with a clear reason, never given an invented value. A genuinely corrupted/unreadable file is a real error, not a "missing metadata" skip.

## Recursion

`documents` does not support `--recursive` — see [Recursive Organization](/organizing/recursive-organization/) for why (shared by all four metadata strategies).

## Related

- [Audio](/strategies/audio/), [Video](/strategies/video/), [Photos](/strategies/photos/) — the other metadata-driven strategies.
