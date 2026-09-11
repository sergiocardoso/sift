---
title: Architecture
description: How Sift's pipeline separates observation, planning, execution, and history.
---

The core pipeline intentionally separates observation, planning, execution, and history:

```text
Filesystem
   │
   ▼
Scanner
   │
   ▼
Classifier / metadata strategy
   │
   ▼
Rules / policy
   │
   ▼
Planner
   │
   ▼
Review
   │
   ▼
Executor
   │
   ▼
History
   │
   ▼
Undo
```

- The **scanner** never mutates the filesystem.
- The **planner** decides what should happen, given the scan, the active strategy or metadata extraction, and any `.sift.toml` rules.
- The **executor** applies only explicitly authorized plans (`--apply`), and independently revalidates every safety assumption immediately before mutation — never trusting the plan's snapshot of the world to still be true.
- **Watch** reuses this exact same organization authority instead of implementing a second, weaker organizer — the daemon calls straight into the same planner/executor the CLI's `organize` uses.

## Project layout

```text
src/
├── classifier.rs      extension-based `type` classification
├── cli.rs             command/flag definitions and dispatch
├── config.rs          .sift.toml parsing, validation, resolution
├── domain.rs
├── executor.rs        applies an authorized plan, revalidates before mutation
├── explain.rs         `sift explain`
├── folders.rs         `sift folders` (whole-folder moves)
├── fs.rs
├── history.rs         recorded operations + undo
├── metadata.rs         audio/video/photo/document metadata extraction
├── planner.rs         builds the plan; owns collision handling
├── render.rs          terminal/table output
├── scanner.rs         read-only directory inspection
├── update_check.rs    background GitHub release check
├── utils.rs
└── watch/
    ├── daemon.rs      the single background process
    ├── eligibility.rs pure path-shape/transient-name decisions
    ├── engine.rs      stabilized candidate → plan + execution
    ├── platform.rs    Unix-only detached process spawning
    ├── registry.rs    persisted watch config, OS-file-lock concurrency
    ├── stability.rs   the debounce state machine
    └── tray.rs        best-effort + explicit sift-tray launch

sift-tray/
└── src/main.rs        thin GUI shell over the same sift library
```

## `sift-tray` is not a second implementation

`sift-tray` never reimplements watch state transitions or organization logic — it calls the exact same library functions (`watch::cmd_watch_pause`, `planner::cmd_organize`, ...) the CLI dispatches to. It also never talks to the watch daemon directly; the daemon polls the registry file on its own, so a tray-initiated change is picked up the same way a CLI-initiated one is. See [Tray App → Overview](/tray-app/overview/).

## Related

- [Safety Model](/core-concepts/safety-model/) — the guarantees the executor enforces.
- [Watch → Lifecycle Guarantees](/watch/lifecycle-guarantees/) — how the daemon confirms state transitions rather than firing and forgetting.
