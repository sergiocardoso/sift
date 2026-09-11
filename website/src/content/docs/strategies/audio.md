---
title: Audio
description: Organizing by audio tags — artist, album, genre, and more.
---

```toml
[organize]
strategy = "audio"
template = "{artist}/{album}"
```

## Placeholders

```text
{artist}
{album}
{album_artist}
{genre}
{track}
{title}
{year}
```

## How it reads metadata

Reads embedded audio tags directly (ID3 and equivalents) across mp3, flac, m4a, ogg, opus, wav, wma, aiff, and more — no external tool required.

## Missing metadata is a skip, never a guess

If a file lacks a tag the template references (an mp3 with no artist tag, say), that file is **skipped** with a clear reason — Sift never invents a fallback folder like `Unknown Artist/`. A file with no tags at all is treated the same as any other file missing the specific field it needs, not as an error.

## Recursion

`audio` does not support `--recursive` — free-text values like an artist name can't be structurally distinguished from a same-named folder that already existed for another reason, so recursive descent is refused outright rather than risking unbounded nesting on a second run. See [Recursive Organization](/organizing/recursive-organization/) for the full explanation (shared by all four metadata strategies).

## Related

- [Video](/strategies/video/), [Photos](/strategies/photos/), [Documents](/strategies/documents/) — the other metadata-driven strategies.
