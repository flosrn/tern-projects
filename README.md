# tern-projects

A curated project sidebar and unified agent history search for [Tern](https://stencil.so/tern).

Bring order to your AI coding sessions across **OMP, Claude Code, Codex, Pi, OpenCode, and Cursor**.

![tern-projects sidebar overview](docs/screenshots/showcase-overview.png)

---

## Quickstart

Run this inside Tern:

```sh
tern plugin install github.com/thegeneralist01/tern-projects
```

That's it. Tern fetches the plugin, and the background engine builds automatically on first load. No manual compile steps, no extra configuration. Open **Projects** from your command palette or press `⌘⌥S`.

To update later:
```sh
tern plugin install github.com/thegeneralist01/tern-projects --force
```

---

## What it does

If you use multiple AI coding agents, your sessions are scattered across hidden SQLite databases and JSONL files. `tern-projects` unifies them into a fast, native sidebar:

- 📁 **Curated "My Projects"** — Group folders into named projects (like an app, its backend, and documentation) with custom icons. Set a **Primary folder** so new chats always start in the right root.
- 🔍 **Instant Full-Text Search** — FTS5-powered search across all your past sessions. Filter by **Worktree**, **Repository**, or **All history** with live keyword highlighting.
- 🚀 **Fresh Chat Launching** — Press `⌘N` to launch a new chat in your preferred CLI (`omp`, `claude`, `codex`, `pi`, `opencode`, or `cursor`). Multi-folder projects pass `--add-dir` flags through automatically.
- 📌 **Global Pins & Recents** — Keep critical threads pinned to the top of your sidebar. Recents keeps your 15 most recent chats one click away.
- 🗄️ **Local Archiving & Safe Deletion** — Archive threads to hide them without losing data, or permanently delete sessions via verified native CLI calls.

![tern-projects details, menus, and search results](docs/screenshots/showcase-details.png)

---

## Keyboard-First Controls

Optimized for speed and one-handed navigation:

| Action | macOS | Windows / Linux |
|---|---|---|
| **Toggle Projects sidebar** | `⌘ ⌥ S` | `Ctrl + Alt + S` |
| **Search everywhere** | `⌃ ⌥ ⌘ S` | `Ctrl + Alt + Shift + S` |
| **Cycle views** (Projects ➔ Found ➔ Search ➔ Settings) | `Tab` / `Shift + Tab` | `Tab` / `Shift + Tab` |
| **Quick Search** from Projects or Settings | `/` | `/` |
| **New chat** (in current project/folder) | `⌘ N` | `Ctrl + N` |
| **New chat with…** (choose harness) | `⇧ ⌘ N` | `Shift + Ctrl + N` |
| **Resume chat** | `Enter` | `Enter` |
| **Fold / unfold project** | `←` / `→` | `←` / `→` |
| **Context menu / Actions** | `Shift + F10` | `Shift + F10` |
| **Copy resume command** | `⌃ Y` | `Ctrl + Y` |
| **Dismiss / Clear search / Close modal** | `Esc` | `Esc` |

Inside any text field, standard terminal editing shortcuts just work (`⌘A`/`Ctrl+A` select all, `⌘⌫`/`Ctrl+U` delete line prefix, `⌥⌫`/`Ctrl+⌫` delete word).

---

## Supported Coding Harnesses

| Agent | Fresh Launch Command | Native Resume | Deletion Support |
|---|---|---|---|
| **OMP** | `omp --cwd <primary> [--add-dir <extra>...]` | `omp --resume <path>` | Transcript + artifact unlinking |
| **Claude Code** | `claude [--add-dir <extra>...]` | `claude --resume <id>` | Exact session history filtering |
| **Codex** | `codex --cd <primary> [--add-dir <extra>...]` | `codex resume <id>` | `codex delete <uuid> --force` |
| **Pi** | `pi` | `pi --session <path>` | Exact file unlinking |
| **OpenCode** | `opencode <primary>` | `opencode --session <id>` | `opencode session delete <id>` |
| **Cursor** | `cursor-agent` / native CLI | Resumes in directory | Clean database record pruning |

---

## How It Works

- **Non-destructive**: Your original git worktrees, folders, and transcripts are never touched or rearranged.
- **Fast SQLite FTS**: Full-text indexing runs in a separate local SQLite database (`index.sqlite`), refreshing in the background every two minutes.
- **Durable Library**: Your custom projects, icons, pins, and preferences are stored in an independent `library.sqlite` so reindexing the cache never wipes your setup.

---

## Manual Build / Development

If you are developing the plugin or want to build from source:

```sh
git clone https://github.com/thegeneralist01/tern-projects.git
cd tern-projects
./scripts/build.sh
tern plugin link .
tern plugin list --json
```

All backend tests pass with:
```sh
cargo test --manifest-path indexer/Cargo.toml
```

---

## Changelog

### 0.2.3 — Sidebar & Interaction Polish

- **Clean Typography**: Title-case section headers (`Pinned`, `Projects`, `Folders`, `Recents`) with aligned margins and a quick `+` button to create projects.
- **Opaque Modals**: Overlay cards, dialogs, and menus now use solid backgrounds to prevent list items from bleeding through.
- **Smarter Path Presentation**: Long paths truncate cleanly in the middle; git branch names (`⎇ main`) stay pinned on the right without clipping.
- **One-Handed Navigation**: 4-stop `Tab` navigation across views, `Shift+F10` options menu, keyboard-navigable archived pages (`Ctrl+P`/`Ctrl+N`), and non-modal `Esc` hide for long operations.
- **Concurrency Fix**: Serialized database initialization lock prevents SQLite busy errors when Tern loads concurrent requests on startup.

### 0.2.2 — Concurrent First-Run Initialization

- Index schema migration runs in a single SQLite write transaction with lock verification to eliminate startup races.
- Automatic backend compilation on plugin load.

---

## License

MIT
