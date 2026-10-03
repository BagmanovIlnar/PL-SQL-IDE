# ADR 0002: Oracle Client distribution

## Status

Accepted for Phase 0

## Context

The Rust `oracle` crate depends on ODPI-C and a local Oracle Client. Bundling Instant Client into a commercial installer has licensing constraints and packaging complexity, especially on macOS arm64.

## Decision

1. Default builds enable PostgreSQL only (`--features postgres`).
2. Oracle support is compiled with `--features oracle`.
3. The application discovers a user-installed Oracle Client and shows a diagnostic screen when it is missing.
4. Bundling Instant Client is deferred until legal review is completed.

## Consequences

- CI can validate the common path without Instant Client
- Oracle users must install a compatible client once
- Packaging docs must include Client setup for each OS
- A later ADR may allow optional bundling after legal approval
