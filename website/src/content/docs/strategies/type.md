---
title: Type
description: The default strategy — classification by file extension.
---

`type` is the default `strategy` for any directory without a `.sift.toml`, and never reads file contents — only the extension, case-insensitively.

```toml
[organize]
strategy = "type"
unknown = "other"   # or "skip"
```

See [Classification](/core-concepts/classification/) for the full category table (Documents, Images, Audio, Video, Archives, 3D, Code, Data, Other, Junk).

## `unknown`

Controls what happens to a file with an extension `type` doesn't recognize:

- `"other"` (default) — organized into `Other/`.
- `"skip"` — left in place, untouched.

## Recursion

`type` fully supports `--recursive` — see [Recursive Organization](/organizing/recursive-organization/). It's one of only two strategies that do (the other is [`date`](/strategies/date/)); the four metadata strategies below don't.

## Related strategies

Reading metadata instead of just the extension:

- [Date](/strategies/date/) — filesystem modification time.
- [Audio](/strategies/audio/) — artist/album/genre tags.
- [Video](/strategies/video/) — resolution, codec, optionally duration/fps.
- [Photos](/strategies/photos/) — EXIF camera and capture date.
- [Documents](/strategies/documents/) — PDF/Office author and creation date.
