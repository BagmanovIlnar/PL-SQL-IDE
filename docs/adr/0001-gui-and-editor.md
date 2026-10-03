# ADR 0001: GUI shell and editor

## Status

Accepted for Phase 0 / MVP

## Context

The product must be a native cross-platform desktop IDE on Windows, Linux and macOS. The original product decision is a pure Rust GUI on `iced`, not a webview shell.

`iced::widget::text_editor` is a basic multiline editor. Features such as line-number gutter, search/replace UI, autocomplete overlays, folding and large-file virtualization will be implemented incrementally on top of iced.

## Decision

Use:

- `iced` 0.14 as the desktop GUI toolkit
- `iced_aw` for additional widgets when needed (tabs, menus)
- `iced` `text_editor` + `highlighter` for the SQL/PLSQL editor foundation
- Rust core for sessions, metadata and execution

Tauri / Monaco are explicitly out of scope unless a future ADR overturns this decision.

## Consequences

- One language and one runtime for UI and DB core
- Faster native packaging without Node/webview stack
- Editor product features require more custom work than Monaco
- Phase 0 must validate editor performance on large documents early
