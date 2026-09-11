---
title: Stability Window
description: Why Watch waits before organizing a newly arrived file.
---

A filesystem event does not mean a file has finished arriving. Browsers, sync tools, and other applications may keep writing to a file for several seconds after it first appears. Sift waits for a stability window before treating a new file as a real candidate to organize.

## Default

```text
2.5 seconds unchanged
```

## Overriding it

```toml
[watch]
stability_seconds = 3
```

Accepted values are `1` through `300` seconds. See [.sift.toml](/configuration/sift-toml/).

## Transient download names are ignored while temporary

These suffixes are recognized as in-progress downloads and never treated as stable candidates while present:

```text
.crdownload
.part
.download
.tmp
```

Once the browser or tool renames the file to its final name, normal stability tracking applies from that point.

## Related

- [Lifecycle Guarantees](/watch/lifecycle-guarantees/) — how stability interacts with pause/resume (a file mid-window when paused is discarded, not queued).
