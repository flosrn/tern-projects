use std::path::Path;
use anyhow::Context;
use crate::model::{Candidate, Parsed, Resume};
use crate::text::{clean_user, one_line, cap_body, iso_ms};
use super::Adapter;

pub struct ClaudeAdapter;

impl Adapter for ClaudeAdapter {
    fn agent(&self) -> &'static str { "claude" }

    fn candidates(&mut self, home: &Path) -> anyhow::Result<Vec<Candidate>> {
        let base = home.join(".claude/projects");
        if !base.exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        // Depth 2: ~/.claude/projects/<enc-dir>/<id>.jsonl
        // Skip subagent dirs: <id>/subagents/*.jsonl would be depth 3.
        for entry1 in std::fs::read_dir(&base).context("listing .claude/projects")? {
            let entry1 = entry1?;
            if entry1.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let p1 = entry1.path();
            if !p1.is_dir() {
                continue;
            }
            for entry2 in std::fs::read_dir(&p1)
                .with_context(|| format!("listing {}", p1.display()))?
            {
                let entry2 = entry2?;
                let name = entry2.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    continue;
                }
                let p2 = entry2.path();
                if !p2.is_file() {
                    continue;
                }
                if p2.extension().map(|e| e != "jsonl").unwrap_or(true) {
                    continue;
                }
                let meta = p2.metadata()?;
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let version = format!("{}:{}", mtime, meta.len());
                let source = format!("claude:{}", p2.to_string_lossy());
                out.push(Candidate { source, version });
            }
        }
        Ok(out)
    }

    fn parse(&mut self, _home: &Path, c: &Candidate) -> anyhow::Result<Option<Parsed>> {
        // source = "claude:<path>"
        let path_str = c.source.strip_prefix("claude:").unwrap_or(&c.source);
        let path = Path::new(path_str);
        let id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;

        let mut cwd = String::new();
        let mut branch: Option<String> = None;
        let mut model: Option<String> = None;
        let mut title: Option<String> = None;
        let mut ai_title: Option<String> = None;
        let mut created_ms: Option<i64> = None;
        let mut updated_ms: Option<i64> = None;
        let mut first_user: Option<String> = None;
        let mut last_assistant = String::new();
        let mut body_parts: Vec<String> = Vec::new();
        let mut messages: i64 = 0;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let record: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Skip sidechain records.
            if record.get("isSidechain").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }

            // Track cwd from any record that has it.
            if cwd.is_empty() {
                if let Some(c) = record.get("cwd").and_then(|v| v.as_str()) {
                    if !c.is_empty() {
                        cwd = c.to_string();
                    }
                }
            }

            // Track timestamps.
            let ts_ms = record
                .get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(iso_ms);
            if let Some(ms) = ts_ms {
                if created_ms.map(|c| ms < c).unwrap_or(true) {
                    created_ms = Some(ms);
                }
                if updated_ms.map(|u| ms > u).unwrap_or(true) {
                    updated_ms = Some(ms);
                }
            }

            // Title extraction.
            if let Some(t_type) = record.get("type").and_then(|v| v.as_str()) {
                if t_type == "custom-title" {
                    if let Some(t) = record
                        .pointer("/message/customTitle")
                        .or_else(|| record.get("customTitle"))
                        .and_then(|v| v.as_str())
                    {
                        if !t.is_empty() {
                            title = Some(t.to_string());
                        }
                    }
                } else if t_type == "ai-title" {
                    if let Some(t) = record
                        .pointer("/message/aiTitle")
                        .or_else(|| record.get("aiTitle"))
                        .and_then(|v| v.as_str())
                    {
                        if !t.is_empty() {
                            ai_title = Some(t.to_string());
                        }
                    }
                }
            }

            // Git branch.
            if let Some(b) = record.get("gitBranch").and_then(|v| v.as_str()) {
                if !b.is_empty() && b != "HEAD" {
                    branch = Some(b.to_string());
                }
            }

            // Messages.
            let msg_type = record.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if msg_type == "user" {
                if record.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
                    continue;
                }
                let text = extract_user_text(&record);
                if let Some(t) = text.and_then(|t| clean_user(&t)) {
                    if first_user.is_none() {
                        first_user = Some(t.clone());
                    }
                    body_parts.push(t);
                    messages += 1;
                }
            } else if msg_type == "assistant" {
                // model from message.model if not synthetic
                if let Some(m) = record.pointer("/message/model").and_then(|v| v.as_str()) {
                    if m != "<synthetic>" && !m.is_empty() {
                        model = Some(m.to_string());
                    }
                }
                let text = extract_assistant_text(&record);
                if !text.is_empty() {
                    last_assistant = one_line(&text, 300);
                    body_parts.push(text);
                    messages += 1;
                }
            }
        }

        // Must have at least one user message to be indexable.
        if first_user.is_none() {
            return Ok(None);
        }

        let first_user = first_user.unwrap();
        let final_title = title
            .or(ai_title)
            .unwrap_or_else(|| one_line(&first_user, 80));
        let final_title = one_line(&final_title, 120);

        let mut body = format!("{}\n{}", final_title, body_parts.join("\n"));
        cap_body(&mut body, 1_000_000);

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        Ok(Some(Parsed {
            id: id.clone(),
            title: final_title,
            cwd,
            branch,
            model,
            created_ms: created_ms.unwrap_or(now_ms),
            updated_ms: updated_ms.unwrap_or(now_ms),
            messages,
            first_user,
            last_assistant,
            body,
            resume: Resume::tab(
                String::new(), // cwd resolved at index time from the stored cwd
                vec!["claude".to_string(), "--resume".to_string(), id],
            ),
        }))
    }
}

fn extract_user_text(record: &serde_json::Value) -> Option<String> {
    let content = record.get("message").and_then(|m| m.get("content"))
        .or_else(|| record.get("content"))?;

    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            let t = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if t == "text" {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
            // Drop tool_result.
        }
        if !parts.is_empty() {
            return Some(parts.join("\n"));
        }
    }
    None
}

fn extract_assistant_text(record: &serde_json::Value) -> String {
    let content = record.get("message").and_then(|m| m.get("content"))
        .or_else(|| record.get("content"));
    let Some(content) = content else {
        return String::new();
    };
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            let t = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if t == "text" {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
            // Drop thinking, tool_use.
        }
        return parts.join("\n");
    }
    String::new()
}
