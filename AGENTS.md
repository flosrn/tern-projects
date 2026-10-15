# AGENTS.md — tern-projects

## Repo layout

```
plugin.toml          Tern plugin manifest
host.luau            Host half: registers the panel block
window.luau          Window half: keybind migration, toggles, guarded launch/resume routes
blocks/panel.luau    Projects (My Projects / Discovered), Search, Settings
lib/cli.luau         Binary discovery, CLI invocation, JSON decode
lib/fmt.luau         Age formatting, agent labels/icons, pct encode, snippet spans
lib/textfield.luau   UTF-8 text field (copied verbatim from tern-agent-scheduler)
lib/input.luau       Shared shortcuts and native UTF-16 edit handling
lib/project-editor.luau  Unsaved project draft and direct-child folder browser
indexer/             Rust crate `tern-projects` — the CLI binary
  src/main.rs        CLI: index, query, projects, library, migrate-data
  src/model.rs       Shared types: Candidate, Parsed, Resume, SessionJson
  src/text.rs        clean_user, one_line, cap_body, shell_join, iso_ms
  src/git.rs         Git worktree resolution (file-only, no git subprocess)
  src/db.rs          SQLite open/migrate/upsert/delete helpers
  src/library.rs     Durable metadata schema, CRUD, folder membership
  src/sessions.rs    Shared annotation projection, scopes, counts and paging
  src/harnesses.rs   Identity-verified discovery and fresh launch plans
  src/delete*.rs     Fail-closed per-harness source deletion
  src/adapters/      One file per agent (claude, codex, omp, pi, opencode, cursor)
  tests/             Real-binary integration tests with isolated homes
scripts/build.sh     Build the Rust binary and copy to bin/
bin/tern-projects    Compiled binary (gitignored)
tern.d.luau          Generated Tern type stubs (gitignored)
```

## VCS

This repo has `.jj` — prefer `jj` for all version-control operations. No mutating `git` commands unless explicitly asked.

## Building

```sh
./scripts/build.sh          # release binary → bin/tern-projects + cargo fallback
cargo test --manifest-path indexer/Cargo.toml  # run all tests
tern plugin types .         # regenerate tern.d.luau after Tern upgrades
tern plugin link .          # (re)link; only needed once after first clone
tern plugin list            # verify: both plugins show "ready"
```

## Key invariants

- **Binary path**: `cli.bin()` tries `<plugin.dir>/bin/tern-projects` first, then `~/.cargo/bin/tern-projects`. Both are populated by `scripts/build.sh`.
- **Data path**: `<plugin.data>/index.sqlite` and sibling durable `library.sqlite` — under `~/Library/Application Support/Tern/plugin-data/tern-projects` on macOS.
- **Indexer schema version** = 1 (in `db.rs`). Bump it when the schema changes; this forces a full reindex.
- **Library schema version** = 1, independent of the disposable index. Never drop durable project/chat/settings data during a cache rebuild.
- **Migration**: `migrate-data --from <legacy-data-directory>` uses SQLite backup, refuses destination conflicts, and retains the source. No old routes/binary aliases.
- **Native menus**: this Tern build displays `=value` in menu labels. Use plain native menu actions with the originating item ID; explicit Actions sheets carry percent-encoded targets.
- **Deletion verification**: synthetic fixture conversations only. Native Codex/OpenCode use their real per-chat commands; direct operations require proven source ownership and an idle store. Never delete project files/worktrees.
- **OMP/Pi message format**: role and content are nested under a `message` sub-object (`record.message.role`, `record.message.content`), not at the top level.
- **Codex source of truth**: the DB (`state_5.sqlite` / `thread_history_1.sqlite`). Rollout JSONL files are a fallback only — 469 of 473 visible threads have no rollout file.
- **FTS snippet markers**: `\x01` (open highlight) and `\x02` (close highlight), set via `char(1)` / `char(2)` in the SQLite snippet() call. `fmt.snippet_spans` in Luau converts these to `ui.span(…, "mark")`.
- **PaneInfo field**: `.pane` (not `.id`) for the pane ID — both in `tern.pane.list()` (host) and `cx.session:panes()` (window).

## Adding a new agent

1. Add `src/adapters/<agent>.rs` implementing `Adapter`.
2. Register it in `run_index` in `main.rs` (the `adapters` Vec).
3. Add fixture files under `tests/fixtures/home/` and assert on its uid in `tests/index.rs`.
4. Add label/icon entries to `fmt.agent_label` / `fmt.agent_icon` in `lib/fmt.luau`.
5. Add the resume argv to the plan doc and verify with a real session.

## Luau style

- `--!strict` in every file.
- Host-only APIs (`tern.fs.*`, `tern.pane.list`) are never called from `window.luau`.
- `(tern :: any).process.run(argv, opts, cb)` — use a local alias to avoid the leading `(` ambiguity that Luau rejects with a bare `;`.
- `textfield.key(f, k, false)` does **not** handle `enter`/`tab`/`escape` — the block's `key` handler must intercept those first.
