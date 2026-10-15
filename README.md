# tern-projects

A [Tern](https://stencil.so/tern) sidebar for curated projects and agent chat history across **OMP, Claude Code, Codex, Pi, OpenCode, and Cursor**.

- **Projects → My Projects**: deliberately created, named projects with an optional icon and multiple source folders. The first folder is **Primary**, where new chats start. Nothing is imported automatically.
- **Projects → Discovered**: the original repository/folder grouping from indexed history.
- **Pinned**: global pinned chats, also retained in their projects and Recents.
- **Recents**: the latest 15 chats across all projects and harnesses.
- **Search**: full-text search with Workspace / Repository / All scopes, a separate named-project filter, relevance or updated-time ordering, and 50-result pages.
- **Settings**: default harness, Color / Monochrome harness icons, and Archived chats.

## Requirements and installation

- Current desktop Tern with plugin blocks, native overlays, and context menus.
- Rust/cargo (current stable recommended).
- At least one supported harness CLI for starting chats; existing history can still be indexed without its CLI.

```sh
./scripts/build.sh
tern plugin types .     # optional: regenerate local Luau API stubs
tern plugin link .
tern plugin list --json
```

The build installs `bin/tern-projects` and the `~/.cargo/bin/tern-projects` fallback. The plugin uses the bundled binary first. No legacy binary or route aliases are registered.

Native application verification is on **macOS**. Field-editing scenarios exercise macOS, Windows, and Linux runtime modifier values; that is not a claim of native Windows/Linux application testing. Launch planning includes Windows PowerShell argument encoding, but native Windows/Linux delivery remains unverified.

## Projects and chat organization

Click **+** to create a project. Enter a name, choose an icon, and add directories through the path field or the direct-child folder browser. Use **Make primary** to change the launch directory. Duplicate folders are canonicalized and deduplicated; projects may share folders intentionally. Empty projects remain visible. Removing a project never removes chats or source folders.

Automatic membership uses each chat's original working directory and folder ancestry, not a string-prefix match or its Git repository root. A chat can automatically belong to overlapping projects. **Change project** selects an exclusive named project, **Automatic (use source folders)**, or **No project**. Organization never moves transcripts, changes working directories, or rewrites resume commands.

Click a project to expand it; the first five chats are shown, **Show more chats** expands to 25, and **Older history — search** opens the appropriate project/repository filter. Recents includes chats already shown in a project or Pinned. Long lists use bounded pages so native row menus remain available.

**Refresh** sits beside **New chat** in the Projects toolbar; it refreshes the cached view and checks source histories in the background.

Right-click a project for **New chat**, **New chat with…**, **Edit project**, or **Remove project**. Discovered folders instead offer **Create project from folder**. Right-click a chat for **Pin/Unpin**, **Change project**, **Archive/Restore**, and **Permanently delete**. The detail dock's **Actions**, also available with Shift+F10, exposes the same choices. Double-click a chat or press Enter to resume it; Copy command and Copy ID remain in the dock.

## New chats and harness settings

**New chat** uses the saved default harness, or asks when no installed default is available. **New chat with…** always asks and does not silently change the default. A harness is a CLI application, not an API/model provider; the plugin does not manage credentials, models, or approval policy.

Executables are discovered from PATH and narrow conventional user-bin directories, then identity-checked using bounded help probes. An unrelated executable named `agent` is not treated as Cursor. The Cursor IDE alone is not a fresh-chat CLI.

OMP, Claude Code, and Codex receive additional source folders through their detected startup options. OMP also receives a generated `autoResume: false` overlay so New chat does not inherit automatic resumption. Unsupported versions/harnesses present the omitted paths and require **Open primary folder only** confirmation; missing extras with supported remaining folders require **Open with remaining folders** confirmation. Missing Primary blocks launch. No in-chat directory command is suggested unless verified for that installed harness.

Normal trust and approval prompts are unchanged. For example, Codex can reject `--add-dir` when its configured permissions are read-only; the plugin does not override that policy. Commands and paths are quoted for shell transport, and launches are restricted to the originating local Tern window.

Color mode applies semantic theme tones to harness icons only; names, titles, and snippets retain normal text colors. Monochrome removes the harness-specific icon tone. Both settings persist immediately.

## Search and keyboard controls

**Workspace** means the pane's Git worktree; **Repository** includes that repository's worktrees; **All** is global. Selecting a named project starts at All scope, after which an explicit Workspace or Repository choice intersects that project. An empty intersection is not silently broadened.

| Shortcut | macOS | Windows/Linux |
|---|---|---|
| Open/focus/close Projects | Cmd+Alt+S | Ctrl+Alt+S |
| Open/focus/close Search | Ctrl+Alt+Cmd+S | Ctrl+Alt+Shift+S |

Settings is available in the palette without a default chord. If a preset suppresses plugin defaults, bind `plugin.tern-projects.projects`, `plugin.tern-projects.search`, or `plugin.tern-projects.settings` explicitly in Tern settings. Exact old Projects/Search action overrides are migrated when the new plugin starts; unrelated keybinds are preserved.

| Inside the panel | Action |
|---|---|
| Tab / Shift+Tab | Cycle Projects, Search, Settings; inside a modal, move between its controls |
| Up / Down | Navigate rows or modal choices |
| Page Up / Page Down | Move selection by ten rows |
| Left / Right in Projects | Collapse / expand the selected project |
| Enter | Activate selected row/control; destructive confirmations initially select Cancel |
| Shift+F10 | Selected row's Actions sheet |
| Ctrl+R | Refresh cached view and index in the background |
| Ctrl+Y | Copy selected chat's resume command |
| `/` in Projects | Open Search |
| Escape | Clear Search, or cancel/back out of a modal |
| Cmd+A on macOS / Ctrl+A elsewhere | Select all in Search and project name/path fields |
| Cmd+Backspace on macOS / Ctrl+U | Delete the field prefix before the cursor |
| Ctrl+Backspace / Alt+Backspace | Delete the previous whitespace-delimited word |

Typing and pasting replace selected text. Native edits translate UTF-16 selections to UTF-8 boundaries and reject stale or invalid offsets.

## Archive versus permanent deletion

**Archive** is reversible and local to this integration. It hides a chat from normal lists, counts, and search without changing its original history. Restore is available under Settings → Archived chats. Assignment and pin state are retained.

**Permanently delete** removes the original saved chat after a target-specific preview and explicit confirmation. It is not an index-only hide:

- Codex and OpenCode use their identity-verified native per-chat deletion commands.
- OMP removes its exact transcript, owned sibling artifacts/rewrite backups, and matching title/recap rows.
- Pi unlinks only its exact transcript; it is not treated as OMP storage.
- Claude removes exact session-owned history/artifacts and filters only matching `history.jsonl` session IDs. Unverified optional cache layouts are refused, not guessed.
- Cursor requires the relevant application/store to be idle and removes only exact composer-owned records, explicit descendants, and header references. Shared blobs and working trees are not deleted.

Direct-file/database deletion requires the relevant harness store to be stopped; ambiguous process ownership, changed targets, symlinks, unsupported schemas, or missing native capabilities cause refusal. The plugin never stops applications automatically and never removes project working files/worktrees. Failures after partial filesystem cleanup are reported explicitly. Filesystem secure erasure, external backups, shared caches, and remote/cloud copies are not promised.

## Storage, refresh, and CLI

On macOS the data directory is:

```text
~/Library/Application Support/Tern/plugin-data/tern-projects/
```

- `index.sqlite`: disposable history/FTS cache, schema version 1.
- `library.sqlite`: durable projects, ordered folders, pins, archive/assignment state, and settings; independently versioned and never reset with the cache.
- `omp-new.yml`: constant fresh-chat overlay.

The panel queries cached rows immediately, indexes changed sources in the background, and refreshes every two minutes. First use has no chats until indexing completes. Source pruning does not discard durable annotations or create phantom chats.

```sh
bin/tern-projects --db /path/to/index.sqlite index
bin/tern-projects --db /path/to/index.sqlite projects  # Discovered grouping
bin/tern-projects --db /path/to/index.sqlite query --scope all --q 'search terms'
bin/tern-projects --db /path/to/index.sqlite query --scope all --project-id 1
```

`library` reads one operation JSON object from stdin and returns one JSON object; errors exit nonzero with stderr detail. For isolated fixture homes use `TERN_PROJECTS_HOME`, never the obsolete home override.

## Migration from the former plugin name

Close only old plugin panel panes, not running agent panes. Build first, then unlink the old plugin and copy its databases before opening the new panel:

```sh
tern plugin unlink tern-sessions
bin/tern-projects \
  --db "$HOME/Library/Application Support/Tern/plugin-data/tern-projects/index.sqlite" \
  migrate-data --from "$HOME/Library/Application Support/Tern/plugin-data/tern-sessions"
tern plugin link .
tern plugin reload
tern plugin list --json
```

Migration uses SQLite backup, including live WAL contents, and leaves the legacy source untouched. A destination index or library conflict causes refusal rather than overwrite or merge. Do not delete an existing destination library to force migration; retain it and resolve the conflict deliberately. Transient pane state and lock files are not transferred. This product rename does not move the checkout directory or change its remote repository.

## Verification

```sh
cargo test --manifest-path indexer/Cargo.toml
./scripts/build.sh
bin/tern-projects --help
tern plugin list --json
```

Tests use isolated homes and the real CLI. Native source-deletion smoke uses synthetic sessions in authentic temporary Codex/OpenCode stores; no real conversations are deletion fixtures.

## License

MIT
