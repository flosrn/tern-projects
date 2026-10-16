# Contributing to tern-projects

Thank you for your interest in contributing to `tern-projects`!

`tern-projects` is a plugin for the [Tern terminal](https://stencil.so/tern). It consists of two coordinated halves:
1. **Frontend (Luau UI)**: `host.luau`, `window.luau`, `blocks/panel.luau`, and `lib/` modules driving Tern's native layout, elements, and styles.
2. **Backend (Rust CLI)**: `indexer/`, a high-performance companion CLI (`bin/tern-projects`) that manages SQLite FTS5 search indexing (`index.sqlite`), durable project metadata (`library.sqlite`), executable discovery, and safe session operations.

---

## Local Development Workflow

### Prerequisites
- [Tern](https://stencil.so/tern) (desktop release)
- Rust toolchain (`rustc`, `cargo` — current stable)

### Setup & Build

```sh
# Clone the repository
git clone https://github.com/thegeneralist01/tern-projects.git
cd tern-projects

# Build the release Rust binary and copy to bin/tern-projects
./scripts/build.sh

# Link the plugin into Tern
tern plugin link .

# Verify the plugin is recognized
tern plugin list --json
```

Whenever you modify Rust code in `indexer/`, run `./scripts/build.sh` (or `cargo test --manifest-path indexer/Cargo.toml`) to compile and test. Tern automatically reloads Luau and CSS changes when saved.

---

## Adding Support for a New AI Coding Agent

We welcome adapters for new AI coding assistants! Adding an agent requires implementing discovery, indexing, and launching:

### 1. Create the Rust Adapter
Create `indexer/src/adapters/<agent>.rs` implementing the `Adapter` trait:
- `candidates(&self, home: &Path) -> Result<Vec<Candidate>>`: Find session files, SQLite databases, or transcript stores on disk.
- `parse(&self, candidate: &Candidate) -> Result<Parsed>`: Extract conversation title, working directory (`cwd`), timestamp, message count, model name, and resume invocation.

Register the new module in:
- `indexer/src/adapters/mod.rs`
- `all_adapters()` in `indexer/src/main.rs`

### 2. Configure Launch & Resume
In `indexer/src/harnesses.rs`:
- Add executable candidate names and search paths in `find_executable`.
- Implement `plan_launch`: configure argv, working directory, and `--add-dir` multi-directory flags if the tool supports them.
- If the tool only supports a single working directory, flag omitted folders so the UI can prompt the user safely.

### 3. Add Safe Deletion (Optional)
If the tool provides a verified per-chat deletion command or unlinked transcript file:
- Add a handler in `indexer/src/delete.rs`.
- Deletion must always verify target identity, check for running processes before mutating disk state, and never touch unrelated files or worktrees.

### 4. UI Icons and Formatting
In `lib/fmt.luau`:
- Add display label in `fmt.agent_label(agent)`.
- Add Tern icon name in `fmt.agent_icon(agent)`.
- Add color tone in `fmt.agent_tone(agent, style)` (e.g., `accent`, `warning`, `success`, `info`, `user`).

### 5. Integration Tests
Add synthetic, sanitized test fixtures under:
- `indexer/tests/fixtures/home/<agent>/...`
- Add assertions in `indexer/tests/index.rs` verifying that candidate parsing, indexing, and search snippets work end-to-end.

Run the test suite:
```sh
cargo test --manifest-path indexer/Cargo.toml
```

---

## Code Style & Conventions

- **Luau**: `--!strict` at the top of every `.luau` file. Follow existing patterns and avoid global state where possible.
- **Rust**: Standard Rust 2021 edition formatting (`cargo fmt`). Prefer direct error context with `anyhow::Context`.
- **CSS**: Scoped to `[data-surface="plugin.tern-projects.panel"]` using Tern design tokens (`var(--sans)`, `var(--card)`, `var(--t1)`, `var(--accent)`). Never hardcode light/dark colors when a theme token exists.
- **Safety**: Never run destructive git operations or mutate user repositories. Chat deletions must fail closed if process ownership or store identity is ambiguous.

---

## Submitting a Pull Request

1. Fork the repository and create your branch from `main`.
2. Ensure all tests pass (`cargo test --manifest-path indexer/Cargo.toml`).
3. Commit with a clear, conventional commit message (e.g. `feat: add goose agent adapter`).
4. Open a Pull Request on GitHub describing your changes and testing evidence.
