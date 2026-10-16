# Adding a New AI Coding Harness

`tern-projects` supports sessions from multiple AI coding tools. This guide walks through adding support for a new assistant (such as Goose, Aider, Devin, or an internal agent).

Adding an assistant requires three primary capabilities:
1. **History indexing**: discovering and parsing past chat sessions.
2. **Launch & resume planning**: launching fresh sessions and resuming existing ones.
3. **Safe session cleanup (optional)**: unlinking files or issuing native CLI deletion commands.

---

## 1. Implement the Rust Adapter

Create `indexer/src/adapters/<agent>.rs` implementing the `Adapter` trait:

```rust
use crate::adapters::Adapter;
use crate::model::{Candidate, Parsed, Resume};
use anyhow::Result;
use std::path::Path;

pub struct NewAgentAdapter;

impl Adapter for NewAgentAdapter {
    fn name(&self) -> &'static str {
        "newagent"
    }

    /// Scan the user's home directory for transcript files or databases.
    fn candidates(&self, home: &Path) -> Result<Vec<Candidate>> {
        let store = home.join(".newagent/sessions");
        // Enumerate candidates...
        Ok(vec![])
    }

    /// Extract session metadata without loading the entire conversation body into memory.
    fn parse(&self, candidate: &Candidate) -> Result<Parsed> {
        Ok(Parsed {
            title: "Session Title".to_string(),
            cwd: "/path/to/project".to_string(),
            project_root: "/path/to/project".to_string(),
            worktree_root: "/path/to/project".to_string(),
            branch: Some("main".to_string()),
            model: Some("claude-3-5-sonnet".to_string()),
            created_ms: 1729000000000,
            updated_ms: 1729000000000,
            messages: 4,
            last_assistant: "Here is the code...".to_string(),
            resume: Resume {
                kind: "command".to_string(),
                argv: None,
                command: Some("newagent --resume <id>".to_string()),
            },
            first_user: "Help me fix this bug".to_string(),
        })
    }
}
```

Register the new adapter:
- Add `pub mod <agent>;` in `indexer/src/adapters/mod.rs`.
- Add `Box::new(<agent>::<Agent>Adapter)` to `all_adapters()` in `indexer/src/main.rs`.

---

## 2. Configure Launch & Resume Planning

In `indexer/src/harnesses.rs`:

1. **Executable Discovery**:
   Add the tool's binary name in `find_executable` (e.g. searching PATH and standard paths like `~/.local/bin`, `~/.cargo/bin`, `~/.npm-packages/bin`).
2. **Launch Planning**:
   Implement argument construction in `plan_launch`:
   - Set the working directory to the project's **Primary** folder.
   - If the tool supports multi-folder workspaces (like OMP, Claude Code, and Codex with repeated `--add-dir`), pass the additional project directories.
   - If the tool only supports a single working directory, flag omitted folders so the UI can prompt the user safely with an "Open primary folder only" confirmation.

---

## 3. Implement Safe Deletion (Optional)

If the harness provides an official per-chat deletion command or uses dedicated session files:

In `indexer/src/delete.rs`:
- For tools with a verified native CLI delete subcommand (like Codex or OpenCode), invoke the native CLI using strict identity checks.
- For file-backed tools (like OMP or Pi), unlink only the exact session transcript and its direct artifacts.
- **Critical safety rule**: Verify that no processes of that harness are running before deleting disk files. If store ownership is ambiguous, fail closed.

---

## 4. UI Icons and Styling

In `lib/fmt.luau`:
- `fmt.agent_label`: Return the human-readable display name (e.g. `"NewAgent"`).
- `fmt.agent_icon`: Return a valid Tern icon name (e.g. `"terminal"`, `"code"`, `"sparkle"`).
- `fmt.agent_tone`: Return the semantic theme color tone (`"accent"`, `"info"`, `"warning"`, `"success"`, `"user"`, or `"muted"`).

---

## 5. Integration Tests

Add a realistic, sanitized fixture under:
`indexer/tests/fixtures/home/<agent>/...`

Add test cases in `indexer/tests/index.rs`:
- Verify candidates are detected from disk.
- Verify session title, working directory, and message counts parse correctly.
- Verify full-text search indexing and snippet highlights function properly.

Run the test suite:
```sh
cargo test --manifest-path indexer/Cargo.toml
```
