# PLSQL IDE

Cross-platform desktop IDE for Oracle PL/SQL and PostgreSQL.

Stack for MVP:

- Rust + iced 0.14
- `iced::widget::text_editor` + highlighter for SQL/PLSQL editing
- `iced_aw` будет добавлен позже для tabs/menus
- PostgreSQL via `sqlx`
- Oracle via `oracle` crate (enabled by default; Instant Client required at runtime)

Product plan: [`plan.md`](./plan.md)

## Phase 0 status

Phase 0 validates the technology gate:

- iced application window with ribbon shell
- SQL editor with completion and line numbers
- PostgreSQL connect / query / cancel spike
- Oracle + PostgreSQL in the default feature set
- CI matrix for Linux, Windows and macOS

## Release packages

GitHub Actions workflow [`.github/workflows/release.yml`](./.github/workflows/release.yml) builds installable artifacts:

| Platform | Artifacts |
| --- | --- |
| Windows x86_64 | `.exe`, `.zip` |
| Linux x86_64 | `.tar.gz`, `.deb`, `.rpm` |
| macOS Apple Silicon (M) | `.zip` (`.app`), `.dmg` |

Triggers:

- push a version tag: `git tag v0.1.0 && git push origin v0.1.0` (also creates a GitHub Release)
- **Actions → Release packages → Run workflow** (artifacts only)

Release binaries include PostgreSQL and Oracle support by default. Oracle Instant Client is **not** bundled — install it on the machine to connect to Oracle.

## Prerequisites

- Rust 1.90+
- PostgreSQL for the Postgres spike (optional for compile)
- Oracle Instant Client on the machine when connecting to Oracle (runtime)

### Linux system packages (Debian/Ubuntu)

```bash
sudo apt-get install -y \
  libasound2-dev libudev-dev libxkbcommon-dev \
  libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev
```

## Develop

```bash
cargo run
```

In the app:

1. Select a connection in **Connections**.
2. Edit SQL in the worksheet and use **Session → Execute**.
3. Change theme under **Settings** if needed.

## Build / install

Default features enable both databases — no `--features` needed:

```bash
cargo run
cargo build --release
cargo install --path .
```

Postgres-only (no Oracle crate):

```bash
cargo build --no-default-features --features postgres
```

## Tests

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Architecture notes

See ADRs:

- [`docs/adr/0001-gui-and-editor.md`](./docs/adr/0001-gui-and-editor.md)
- [`docs/adr/0002-oracle-client.md`](./docs/adr/0002-oracle-client.md)
