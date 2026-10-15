use std::path::Path;
use anyhow::Context;
use rusqlite::{params, OpenFlags};
use crate::model::{Candidate, Parsed, Resume};
use crate::text::{clean_user, one_line, cap_body};
use super::Adapter;

pub struct OpenCodeAdapter;

impl Adapter for OpenCodeAdapter {
    fn agent(&self) -> &'static str { "opencode" }

    fn candidates(&mut self, home: &Path) -> anyhow::Result<Vec<Candidate>> {
        let db_path = opencode_db(home);
        if !db_path.exists() {
            return Ok(vec![]);
        }
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("opening opencode db {}", db_path.display()))?;
        conn.execute_batch("PRAGMA busy_timeout=2000;")?;

        let mut stmt = conn.prepare(
            "SELECT id, time_updated FROM session \
             WHERE parent_id IS NULL AND time_archived IS NULL",
        )?;
        let out = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .flatten()
            .map(|(id, updated_ms)| Candidate {
                source: format!("opencode:{id}"),
                version: updated_ms.to_string(),
            })
            .collect();
        Ok(out)
    }

    fn parse(&mut self, home: &Path, c: &Candidate) -> anyhow::Result<Option<Parsed>> {
        let id = c.source.strip_prefix("opencode:").unwrap_or(&c.source);
        let db_path = opencode_db(home);
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.execute_batch("PRAGMA busy_timeout=2000;")?;

        let row: Option<(String, Option<String>, Option<String>, i64, i64)> = conn.query_row(
            "SELECT COALESCE(title,''), directory, \
             json_extract(model,'$.id'), \
             COALESCE(time_created,0), COALESCE(time_updated,0) \
             FROM session WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        ).ok();

        let Some((db_title, cwd_opt, model_opt, created_ms, updated_ms)) = row else {
            return Ok(None);
        };
        let cwd = cwd_opt.unwrap_or_default();

        // Fetch text parts.
        let mut stmt = conn.prepare(
            "SELECT json_extract(m.data,'$.role'), json_extract(p.data,'$.text') \
             FROM part p \
             JOIN message m ON m.id = p.message_id \
             WHERE p.session_id = ?1 \
             AND json_extract(p.data,'$.type') = 'text' \
             AND COALESCE(json_extract(p.data,'$.synthetic'),0) = 0 \
             ORDER BY m.time_created, p.id",
        )?;

        let rows: Vec<(String, String)> = stmt
            .query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .flatten()
            .collect();

        let mut messages: i64 = 0;
        let mut first_user: Option<String> = None;
        let mut last_assistant = String::new();
        let mut body_parts: Vec<String> = Vec::new();
        let mut last_role = String::new();

        for (role, text) in &rows {
            if role == "user" {
                if let Some(t) = clean_user(text) {
                    if first_user.is_none() { first_user = Some(t.clone()); }
                    // Count consecutive parts of one message as one message.
                    if last_role != "user" { messages += 1; }
                    body_parts.push(t);
                }
            } else if role == "assistant" {
                if !text.is_empty() {
                    last_assistant = one_line(text, 300);
                    if last_role != "assistant" { messages += 1; }
                    body_parts.push(text.clone());
                }
            }
            last_role = role.clone();
        }

        if first_user.is_none() { return Ok(None); }
        let first_user = first_user.unwrap();

        // Title: use db_title unless it starts with "New session - ".
        let raw_title = if db_title.starts_with("New session - ") || db_title.is_empty() {
            one_line(&first_user, 80)
        } else {
            db_title
        };
        let title = one_line(&raw_title, 120);

        let mut body = format!("{}\n{}", title, body_parts.join("\n"));
        cap_body(&mut body, 1_000_000);

        Ok(Some(Parsed {
            id: id.to_string(),
            title,
            cwd,
            branch: None,
            model: model_opt.filter(|s| !s.is_empty()),
            created_ms,
            updated_ms,
            messages,
            first_user,
            last_assistant,
            body,
            resume: Resume::tab(
                String::new(),
                vec!["opencode".to_string(), "--session".to_string(), id.to_string()],
            ),
        }))
    }
}

fn opencode_db(home: &Path) -> std::path::PathBuf {
    home.join(".local/share/opencode/opencode.db")
}
