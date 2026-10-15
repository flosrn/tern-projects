use std::path::Path;
use crate::model::{Candidate, Parsed, Resume};
use crate::text::{clean_user, one_line, cap_body};
use super::Adapter;
use super::omp::candidates_at;

pub struct PiAdapter;

impl Adapter for PiAdapter {
    fn agent(&self) -> &'static str { "pi" }

    fn candidates(&mut self, home: &Path) -> anyhow::Result<Vec<Candidate>> {
        candidates_at(home, ".pi/agent/sessions", "pi")
    }

    fn parse(&mut self, _home: &Path, c: &Candidate) -> anyhow::Result<Option<Parsed>> {
        let path_str = c.source.strip_prefix("pi:").unwrap_or(&c.source);
        let path = Path::new(path_str);
        let content = std::fs::read_to_string(path)?;

        let mut id = String::new();
        let mut cwd = String::new();
        let mut session_name: Option<String> = None;
        let mut model: Option<String> = None;
        let mut created_ms: Option<i64> = None;
        let mut updated_ms: Option<i64> = None;
        let mut first_user: Option<String> = None;
        let mut last_assistant = String::new();
        let mut body_parts: Vec<String> = Vec::new();
        let mut messages: i64 = 0;

        for line in content.lines() {
            if line.trim().is_empty() { continue; }
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let ts_ms = get_ts(&v);
            if let Some(ms) = ts_ms {
                if created_ms.map(|c| ms < c).unwrap_or(true) { created_ms = Some(ms); }
                if updated_ms.map(|u| ms > u).unwrap_or(true) { updated_ms = Some(ms); }
            }

            let rec_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

            if rec_type == "session" {
                id = v.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                cwd = v.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string();
            } else if rec_type == "session_info" {
                if let Some(n) = v.get("name").and_then(|v| v.as_str()) {
                    if !n.is_empty() { session_name = Some(n.to_string()); }
                }
            } else if rec_type == "model_change" {
                // Pi uses modelId.
                if let Some(m) = v.get("modelId").and_then(|v| v.as_str())
                    .or_else(|| v.get("model").and_then(|v| v.as_str()))
                {
                    if !m.is_empty() { model = Some(m.to_string()); }
                }
            } else if rec_type == "message" {
                // Pi nests role+content under a "message" sub-object (same as OMP).
                let msg = v.get("message").unwrap_or(&v);
                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "user" {
                    let text = extract_pi_user(msg);
                    if let Some(t) = text.and_then(|t| clean_user(&t)) {
                        if first_user.is_none() { first_user = Some(t.clone()); }
                        body_parts.push(t);
                        messages += 1;
                    }
                } else if role == "assistant" {
                    let text = extract_pi_assistant(msg);
                    if !text.is_empty() {
                        last_assistant = one_line(&text, 300);
                        body_parts.push(text);
                        messages += 1;
                    }
                }
            }
        }

        if first_user.is_none() { return Ok(None); }
        let first_user = first_user.unwrap();

        let raw_title = session_name.unwrap_or_else(|| one_line(&first_user, 80));
        let title = one_line(&raw_title, 120);

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
        let upd = updated_ms.unwrap_or_else(|| {
            path.metadata().ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(now_ms)
        });

        let mut body = format!("{}\n{}", title, body_parts.join("\n"));
        cap_body(&mut body, 1_000_000);

        if id.is_empty() {
            id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        }

        Ok(Some(Parsed {
            id,
            title,
            cwd,
            branch: None,
            model,
            created_ms: created_ms.unwrap_or(now_ms),
            updated_ms: upd,
            messages,
            first_user,
            last_assistant,
            body,
            resume: Resume::tab(
                String::new(),
                vec!["pi".to_string(), "--session".to_string(), path_str.to_string()],
            ),
        }))
    }
}

fn get_ts(v: &serde_json::Value) -> Option<i64> {
    if let Some(n) = v.get("timestamp").and_then(|t| t.as_i64()) { return Some(n); }
    if let Some(s) = v.get("timestamp").and_then(|t| t.as_str()) { return crate::text::iso_ms(s); }
    None
}

fn extract_pi_user(v: &serde_json::Value) -> Option<String> {
    let content = v.get("content")?;
    if let Some(s) = content.as_str() { return Some(s.to_string()); }
    if let Some(arr) = content.as_array() {
        let parts: Vec<&str> = arr.iter()
            .filter(|i| i.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect();
        if !parts.is_empty() { return Some(parts.join("\n")); }
    }
    None
}

fn extract_pi_assistant(v: &serde_json::Value) -> String {
    let content = match v.get("content") { Some(c) => c, None => return String::new() };
    if let Some(arr) = content.as_array() {
        let parts: Vec<&str> = arr.iter()
            .filter(|i| i.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect();
        return parts.join("\n");
    }
    if let Some(s) = content.as_str() { return s.to_string(); }
    String::new()
}
