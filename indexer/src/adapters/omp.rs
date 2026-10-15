use std::path::Path;
use crate::model::{Candidate, Parsed, Resume};
use crate::text::{clean_user, one_line, cap_body};
use super::Adapter;

pub struct OmpAdapter;

impl Adapter for OmpAdapter {
    fn agent(&self) -> &'static str { "omp" }

    fn candidates(&mut self, home: &Path) -> anyhow::Result<Vec<Candidate>> {
        candidates_at(home, ".omp/agent/sessions", "omp")
    }

    fn parse(&mut self, _home: &Path, c: &Candidate) -> anyhow::Result<Option<Parsed>> {
        parse_omp_file(c, "omp", |path| {
            Resume::tab(String::new(), vec!["omp".to_string(), "--resume".to_string(), path.to_string()])
        })
    }
}

/// Shared depth-2 candidate listing for OMP and Pi.
pub fn candidates_at(home: &Path, rel: &str, agent: &str) -> anyhow::Result<Vec<Candidate>> {
    let base = home.join(rel);
    if !base.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry1 in std::fs::read_dir(&base)? {
        let entry1 = entry1?;
        if entry1.file_name().to_string_lossy().starts_with('.') { continue; }
        let p1 = entry1.path();
        if !p1.is_dir() { continue; }
        for entry2 in std::fs::read_dir(&p1)? {
            let entry2 = entry2?;
            let name = entry2.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') { continue; }
            let p2 = entry2.path();
            if !p2.is_file() { continue; }
            if p2.extension().map(|e| e != "jsonl").unwrap_or(true) { continue; }
            let meta = p2.metadata()?;
            let mtime = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis()).unwrap_or(0);
            let version = format!("{}:{}", mtime, meta.len());
            let source = format!("{}:{}", agent, p2.to_string_lossy());
            out.push(Candidate { source, version });
        }
    }
    Ok(out)
}

/// Shared JSONL parser for OMP-style files.
pub fn parse_omp_file<F>(c: &Candidate, agent: &str, make_resume: F) -> anyhow::Result<Option<Parsed>>
where
    F: FnOnce(&str) -> Resume,
{
    let path_str = c.source.strip_prefix(&format!("{agent}:")).unwrap_or(&c.source);
    let path = Path::new(path_str);
    let content = std::fs::read_to_string(path)?;

    let mut id = String::new();
    let mut cwd = String::new();
    let mut session_title: Option<String> = None;
    let mut first_title: Option<String> = None;
    let mut last_title_change: Option<String> = None;
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

        // Timestamp tracking.
        let ts_ms = get_ts(&v);
        if let Some(ms) = ts_ms {
            if created_ms.map(|c| ms < c).unwrap_or(true) { created_ms = Some(ms); }
            if updated_ms.map(|u| ms > u).unwrap_or(true) { updated_ms = Some(ms); }
        }

        let rec_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if rec_type == "session" {
            id = v.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            cwd = v.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if let Some(t) = v.get("title").and_then(|v| v.as_str()) {
                if !t.is_empty() { session_title = Some(t.to_string()); }
            }
        } else if rec_type == "title" {
            // First `title` record wins.
            if first_title.is_none() {
                if let Some(t) = v.get("title").and_then(|v| v.as_str()) {
                    if !t.is_empty() { first_title = Some(t.to_string()); }
                }
            }
        } else if rec_type == "title_change" {
            if let Some(t) = v.get("title").and_then(|v| v.as_str()) {
                if !t.is_empty() { last_title_change = Some(t.to_string()); }
            }
        } else if rec_type == "model_change" {
            if let Some(m) = v.get("model").and_then(|v| v.as_str()) {
                if !m.is_empty() { model = Some(m.to_string()); }
            }
        } else if rec_type == "message" {
            // OMP nests role+content under a "message" sub-object.
            let msg = v.get("message").unwrap_or(&v);
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role == "user" {
                let text = extract_content_text(msg);
                if let Some(t) = text.and_then(|t| clean_user(&t)) {
                    if first_user.is_none() { first_user = Some(t.clone()); }
                    body_parts.push(t);
                    messages += 1;
                }
            } else if role == "assistant" {
                let text = extract_assistant_parts(msg);
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

    // Title priority: first_title > last_title_change > session_title > fallback
    let raw_title = first_title
        .or(last_title_change)
        .or(session_title)
        .unwrap_or_else(|| one_line(&first_user, 80));
    let title = one_line(&raw_title, 120);

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64;

    // updated_ms fallback: file mtime (already in the source version string).
    let upd = updated_ms.unwrap_or_else(|| {
        path.metadata().ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(now_ms)
    });

    let mut body = format!("{}\n{}", title, body_parts.join("\n"));
    cap_body(&mut body, 1_000_000);

    let resume = make_resume(path_str);

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
        resume,
    }))
}

fn get_ts(v: &serde_json::Value) -> Option<i64> {
    // Try numeric ms first, then ISO string.
    if let Some(n) = v.get("timestamp").and_then(|t| t.as_i64()) {
        return Some(n);
    }
    if let Some(s) = v.get("timestamp").and_then(|t| t.as_str()) {
        return crate::text::iso_ms(s);
    }
    None
}

fn extract_content_text(v: &serde_json::Value) -> Option<String> {
    let content = v.get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        let parts: Vec<&str> = arr.iter()
            .filter(|i| i.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect();
        if !parts.is_empty() {
            return Some(parts.join("\n"));
        }
    }
    None
}

fn extract_assistant_parts(v: &serde_json::Value) -> String {
    let content = match v.get("content") { Some(c) => c, None => return String::new() };
    if let Some(arr) = content.as_array() {
        let parts: Vec<&str> = arr.iter()
            .filter(|i| i.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect();
        return parts.join("\n");
    }
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    String::new()
}
