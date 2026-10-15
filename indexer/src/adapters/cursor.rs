use std::path::Path;
use anyhow::Context;
use rusqlite::{params, OpenFlags};
use crate::model::{Candidate, Parsed, Resume};
use crate::text::{one_line, cap_body};
use super::Adapter;

pub struct CursorAdapter;

impl Adapter for CursorAdapter {
    fn agent(&self) -> &'static str { "cursor" }

    fn candidates(&mut self, home: &Path) -> anyhow::Result<Vec<Candidate>> {
        let db_path = cursor_db(home);
        if !db_path.exists() {
            return Ok(vec![]);
        }
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("opening cursor db {}", db_path.display()))?;
        conn.execute_batch("PRAGMA busy_timeout=2000;")?;

        let mut stmt = conn.prepare(
            "SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .flatten()
            .collect();

        let mut out = Vec::new();
        for (key, value) in rows {
            let v: serde_json::Value = match serde_json::from_str(&value) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // Skip archived.
            if v.get("isArchived").and_then(|a| a.as_bool()).unwrap_or(false) {
                continue;
            }
            // Must have messages.
            let has_full = v.get("fullConversationHeadersOnly")
                .and_then(|a| a.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            let has_conv = v.get("conversation")
                .and_then(|a| a.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if !has_full && !has_conv {
                continue;
            }
            let version = v.get("lastUpdatedAt")
                .and_then(|t| t.as_i64())
                .map(|t| t.to_string())
                .unwrap_or_else(|| "0".to_string());
            out.push(Candidate {
                source: key,
                version,
            });
        }
        Ok(out)
    }

    fn parse(&mut self, home: &Path, c: &Candidate) -> anyhow::Result<Option<Parsed>> {
        let db_path = cursor_db(home);
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.execute_batch("PRAGMA busy_timeout=2000;")?;

        let value: Option<String> = conn.query_row(
            "SELECT value FROM cursorDiskKV WHERE key=?1",
            params![c.source],
            |r| r.get(0),
        ).ok();

        let Some(value) = value else { return Ok(None); };
        let v: serde_json::Value = serde_json::from_str(&value)?;

        let id = c.source.strip_prefix("composerData:").unwrap_or(&c.source);
        let created_ms = v.get("createdAt").and_then(|t| t.as_i64()).unwrap_or(0);
        let updated_ms = v.get("lastUpdatedAt").and_then(|t| t.as_i64()).unwrap_or(created_ms);
        let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let model = v.get("modelConfig")
            .and_then(|m| m.get("modelName"))
            .and_then(|n| n.as_str())
            .filter(|s| *s != "default" && !s.is_empty())
            .map(String::from);

        // Messages: old format (conversation[]) or new format (fullConversationHeadersOnly).
        let mut messages_list: Vec<(i64, String)> = Vec::new(); // (type, text): 1=user, 2=assistant

        if let Some(conv) = v.get("conversation").and_then(|c| c.as_array()) {
            // Old v1 format.
            for bubble in conv {
                let btype = bubble.get("type").and_then(|t| t.as_i64()).unwrap_or(0);
                let text = bubble.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if text.is_empty() { continue; }
                messages_list.push((btype, text.to_string()));
            }
        } else if let Some(headers) = v.get("fullConversationHeadersOnly").and_then(|h| h.as_array()) {
            // New format: fetch each bubble from cursorDiskKV.
            let composer_id = id;
            for header in headers {
                let bubble_id = header.get("bubbleId")
                    .and_then(|b| b.as_str())
                    .unwrap_or("");
                if bubble_id.is_empty() { continue; }
                let bubble_key = format!("bubbleId:{composer_id}:{bubble_id}");
                let bubble_val: Option<String> = conn.query_row(
                    "SELECT value FROM cursorDiskKV WHERE key=?1",
                    params![bubble_key],
                    |r| r.get(0),
                ).ok();
                let Some(bv_str) = bubble_val else { continue; };
                let bv: serde_json::Value = match serde_json::from_str(&bv_str) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let btype = bv.get("type").and_then(|t| t.as_i64()).unwrap_or(0);
                let text = bv.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if text.is_empty() { continue; }
                messages_list.push((btype, text.to_string()));
            }
        }

        // Extract cwd from first user bubble.
        let mut cwd = String::new();
        for (btype, _) in &messages_list {
            if *btype == 1 {
                // Try workspaceProjectDir from the raw value.
                cwd = v.get("workspaceProjectDir")
                    .and_then(|d| d.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        v.get("workspaceUris")
                            .and_then(|u| u.as_array())
                            .and_then(|a| a.first())
                            .and_then(|u| u.as_str())
                            .map(|u| {
                                // strip file:// and percent-decode
                                let stripped = u.strip_prefix("file://").unwrap_or(u);
                                percent_decode(stripped)
                            })
                            .unwrap_or_default()
                    });
                break;
            }
        }

        let mut first_user: Option<String> = None;
        let mut last_assistant = String::new();
        let mut body_parts: Vec<String> = Vec::new();
        let mut msg_count: i64 = 0;

        for (btype, text) in &messages_list {
            if *btype == 1 {
                if first_user.is_none() { first_user = Some(text.clone()); }
                body_parts.push(text.clone());
                msg_count += 1;
            } else if *btype == 2 {
                last_assistant = one_line(text, 300);
                body_parts.push(text.clone());
                msg_count += 1;
            }
        }

        if first_user.is_none() { return Ok(None); }
        let first_user = first_user.unwrap();

        let raw_title = if name.is_empty() { one_line(&first_user, 80) } else { name };
        let title = one_line(&raw_title, 120);

        let mut body = format!("{}\n{}", title, body_parts.join("\n"));
        cap_body(&mut body, 1_000_000);

        // Resume: open Cursor with the cwd.
        let resume = if cwd.is_empty() {
            Resume::spawn(String::new(), vec!["open".to_string(), "-a".to_string(), "Cursor".to_string()])
        } else {
            Resume::spawn(cwd.clone(), vec!["open".to_string(), "-a".to_string(), "Cursor".to_string(), cwd.clone()])
        };

        Ok(Some(Parsed {
            id: id.to_string(),
            title,
            cwd,
            branch: None,
            model,
            created_ms,
            updated_ms,
            messages: msg_count,
            first_user,
            last_assistant,
            body,
            resume,
        }))
    }
}

fn cursor_db(home: &Path) -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    return home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb");
    #[cfg(not(target_os = "macos"))]
    return home.join(".config/Cursor/User/globalStorage/state.vscdb");
}

fn percent_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h1), Some(h2)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h1 << 4) | h2);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
