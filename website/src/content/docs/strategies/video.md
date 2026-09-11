---
title: Video
description: Organizing by video container metadata, with optional ffprobe support.
---

```toml
[organize]
strategy = "video"
template = "{resolution}/{year}"
```

## Two backends

**Always available**: a pure-Rust MP4/MOV container parser reads width, height, codec, and creation time directly — no external tool needed. Understands MP4/MOV only, and never populates `{duration}`/`{fps}`.

**Optional, tried first if present**: if `ffprobe` is on your `PATH`, Sift uses it for broader container/format support and richer metadata. If `ffprobe` is missing, or present but fails for any reason, Sift silently falls back to the built-in parser rather than erroring out.

The fields both backends share (`{width}`, `{height}`, `{resolution}`, `{codec}`, `{year}`) are identical for the same file regardless of which backend answered — a template you've already configured never points somewhere different just because you installed `ffmpeg` later.

## Placeholders

```text
{resolution}
{width}
{height}
{codec}
{year}
{duration}   ffprobe only
{fps}        ffprobe only
```

If a template uses `{duration}` or `{fps}` and `ffprobe` isn't installed, those fields are unavailable — the file is skipped with a clear reason, never given a guessed value.

## Installing ffprobe

`install.sh` checks for `ffprobe` and offers to install it — see [Installation](/getting-started/installation/). It's entirely optional; `video` works without it.

## Recursion

`video` does not support `--recursive` — see [Recursive Organization](/organizing/recursive-organization/) for why (shared by all four metadata strategies).

## Related

- [Audio](/strategies/audio/), [Photos](/strategies/photos/), [Documents](/strategies/documents/) — the other metadata-driven strategies.
