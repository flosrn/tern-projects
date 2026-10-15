use serde::Serialize;

/// A source record identifying one indexable unit (file or DB row).
#[derive(Debug, Clone)]
pub struct Candidate {
    pub source: String,
    pub version: String,
}

/// A fully-parsed session ready for insertion.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub messages: i64,
    pub first_user: String,
    pub last_assistant: String,
    pub body: String,
    pub resume: Resume,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resume {
    pub kind: &'static str, // "tab" | "spawn"
    pub cwd: String,
    pub argv: Vec<String>,
    pub command: String,
}

impl Resume {
    pub fn tab(cwd: impl Into<String>, argv: Vec<String>) -> Self {
        let command = crate::text::shell_join(&argv);
        Resume {
            kind: "tab",
            cwd: cwd.into(),
            argv,
            command,
        }
    }

    pub fn spawn(cwd: impl Into<String>, argv: Vec<String>) -> Self {
        let command = crate::text::shell_join(&argv);
        Resume {
            kind: "spawn",
            cwd: cwd.into(),
            argv,
            command,
        }
    }
}

/// Session as returned in JSON responses.
#[derive(Debug, Serialize)]
pub struct SessionJson {
    pub uid: String,
    pub agent: String,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub project_root: String,
    pub project_label: String,
    pub worktree_root: String,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub messages: i64,
    pub last_assistant: String,
    pub snippet: Option<String>,
    pub resume: serde_json::Value,
    pub pinned: bool,
    pub archived: bool,
    pub assignment: Assignment,
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

/// Compute a human-readable project label from a project_root path and a home dir.
pub fn project_label(root: &str, home: &str) -> String {
    if root.is_empty() {
        return "No project".to_string();
    }
    if root == home {
        return "~".to_string();
    }
    // basename
    root.rsplit('/').next().unwrap_or(root).to_string()
}
