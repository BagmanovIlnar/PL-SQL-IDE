# ADR 0002: Oracle Client distribution

## Status

Accepted for Phase 0

## Context

The Rust `oracle` crate depends on ODPI-C and a local Oracle Client. Bundling Instant Client into a commercial installer has licensing constraints and packaging complexity, especially on macOS arm64.

## Decision

1. Default builds enable **both** PostgreSQL and Oracle (`default = ["postgres", "oracle"]`).
   Plain `cargo build` / `cargo install plsqlide` / CI release packages need **no** `--features`.
2. The `oracle` crate (ODPI-C) links at build time without Instant Client; the client is loaded at **runtime**.
3. The application discovers a user-installed Oracle Client and shows a diagnostic screen when it is missing.
4. Bundling Instant Client is deferred until legal review is completed.
5. Opt-out remains possible: `cargo build --no-default-features --features postgres`.

## Consequences

- One binary supports all wired databases by default
- Oracle users must still install a compatible Instant Client to connect
- Packaging docs must include Client setup for each OS
- A later ADR may allow optional bundling after legal approval
