use anyhow::Context;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 1;

/// Open (or create) the index DB, running migrations as needed.
pub fn open(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating db parent dir {}", parent.display()))?;
    }
    let mut conn = Connection::open(path)
        .with_context(|| format!("opening db at {}", path.display()))?;

    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    if user_version != SCHEMA_VERSION {
        migrate(&mut conn)?;
    }

    Ok(conn)
}

/// Open read-only for a helper DB (e.g. agent stores).
pub fn open_readonly(path: &Path) -> anyhow::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening read-only db at {}", path.display()))
}

fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // An opener may have migrated while we waited. Never recreate its live cache.
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == SCHEMA_VERSION {
        tx.commit()?;
        return Ok(());
    }
    tx.execute_batch("
        DROP TABLE IF EXISTS sessions_fts;
        DROP TABLE IF EXISTS sessions;
        DROP TABLE IF EXISTS sources;
        CREATE TABLE sources(source TEXT PRIMARY KEY, agent TEXT NOT NULL, version TEXT NOT NULL);
        CREATE TABLE sessions(
            rowid INTEGER PRIMARY KEY,
            uid TEXT NOT NULL UNIQUE,
            agent TEXT NOT NULL,
            id TEXT NOT NULL,
            source TEXT NOT NULL,
            title TEXT NOT NULL,
            cwd TEXT NOT NULL,
            project_root TEXT NOT NULL,
            worktree_root TEXT NOT NULL,
            branch TEXT,
            model TEXT,
            created_ms INTEGER NOT NULL,
            updated_ms INTEGER NOT NULL,
            messages INTEGER NOT NULL,
            last_assistant TEXT NOT NULL,
            resume TEXT NOT NULL
        );
        CREATE INDEX sessions_updated ON sessions(updated_ms DESC);
        CREATE INDEX sessions_project ON sessions(project_root, updated_ms DESC);
        CREATE INDEX sessions_source ON sessions(source);
        CREATE VIRTUAL TABLE sessions_fts USING fts5(
            title, body,
            tokenize='unicode61 remove_diacritics 2'
        );
        PRAGMA user_version=1;
    ")?;
    tx.commit()?;
    Ok(())
}

/// Check whether `uid` already exists in sessions.
pub fn get_source_version(conn: &Connection, source: &str) -> Option<String> {
    conn.query_row(
        "SELECT version FROM sources WHERE source = ?1",
        params![source],
        |r| r.get(0),
    )
    .ok()
}

pub struct UpsertData<'a> {
    pub uid: &'a str,
    pub agent: &'a str,
    pub id: &'a str,
    pub source: &'a str,
    pub title: &'a str,
    pub cwd: &'a str,
    pub project_root: &'a str,
    pub worktree_root: &'a str,
    pub branch: Option<&'a str>,
    pub model: Option<&'a str>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub messages: i64,
    pub last_assistant: &'a str,
    pub resume_json: &'a str,
    pub body: &'a str,
    pub source_version: &'a str,
}

pub fn upsert_session(conn: &Connection, d: &UpsertData<'_>) -> anyhow::Result<()> {
    // Delete old FTS row if the session already exists.
    let old_rowid: Option<i64> = conn
        .query_row(
            "SELECT rowid FROM sessions WHERE uid = ?1",
            params![d.uid],
            |r| r.get(0),
        )
        .ok();
    if let Some(rowid) = old_rowid {
        conn.execute("DELETE FROM sessions_fts WHERE rowid = ?1", params![rowid])?;
    }

    conn.execute(
        "INSERT INTO sessions(uid,agent,id,source,title,cwd,project_root,worktree_root,branch,model,
            created_ms,updated_ms,messages,last_assistant,resume)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
         ON CONFLICT(uid) DO UPDATE SET
            agent=excluded.agent, id=excluded.id, source=excluded.source,
            title=excluded.title, cwd=excluded.cwd, project_root=excluded.project_root,
            worktree_root=excluded.worktree_root, branch=excluded.branch, model=excluded.model,
            created_ms=excluded.created_ms, updated_ms=excluded.updated_ms,
            messages=excluded.messages, last_assistant=excluded.last_assistant,
            resume=excluded.resume",
        params![
            d.uid, d.agent, d.id, d.source, d.title, d.cwd,
            d.project_root, d.worktree_root, d.branch, d.model,
            d.created_ms, d.updated_ms, d.messages, d.last_assistant, d.resume_json
        ],
    )?;

    let new_rowid: i64 = conn.query_row(
        "SELECT rowid FROM sessions WHERE uid = ?1",
        params![d.uid],
        |r| r.get(0),
    )?;

    conn.execute(
        "INSERT INTO sessions_fts(rowid, title, body) VALUES(?1, ?2, ?3)",
        params![new_rowid, d.title, d.body],
    )?;

    conn.execute(
        "INSERT INTO sources(source, agent, version) VALUES(?1,?2,?3)
         ON CONFLICT(source) DO UPDATE SET agent=excluded.agent, version=excluded.version",
        params![d.source, d.agent, d.source_version],
    )?;

    Ok(())
}

/// Delete a session by source (used when parse returns None or source is gone).
pub fn delete_by_source(conn: &Connection, source: &str) -> anyhow::Result<()> {
    let rowid: Option<i64> = conn
        .query_row(
            "SELECT rowid FROM sessions WHERE source = ?1",
            params![source],
            |r| r.get(0),
        )
        .ok();
    if let Some(rowid) = rowid {
        conn.execute("DELETE FROM sessions_fts WHERE rowid = ?1", params![rowid])?;
        conn.execute("DELETE FROM sessions WHERE rowid = ?1", params![rowid])?;
    }
    Ok(())
}

/// Upsert a source record without a session (parse returned None).
pub fn upsert_source_only(conn: &Connection, source: &str, agent: &str, version: &str) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO sources(source, agent, version) VALUES(?1,?2,?3)
         ON CONFLICT(source) DO UPDATE SET agent=excluded.agent, version=excluded.version",
        params![source, agent, version],
    )?;
    Ok(())
}

/// Remove all sources (and their sessions) for `agent` whose source is not in `live`.
pub fn remove_stale(conn: &Connection, agent: &str, live: &[String]) -> anyhow::Result<usize> {
    // Get all sources for this agent.
    let mut stmt = conn.prepare("SELECT source FROM sources WHERE agent = ?1")?;
    let all_sources: Vec<String> = stmt
        .query_map(params![agent], |r| r.get(0))?
        .flatten()
        .collect();

    let live_set: std::collections::HashSet<&str> = live.iter().map(String::as_str).collect();
    let mut removed = 0usize;
    for src in &all_sources {
        if !live_set.contains(src.as_str()) {
            delete_by_source(conn, src)?;
            conn.execute("DELETE FROM sources WHERE source = ?1", params![src])?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_migration_preserves_cache_committed_by_another_opener() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.sqlite");
        let mut waiting = Connection::open(&path).unwrap();
        let initial_version: i64 = waiting.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(initial_version, 0);

        // The first opener finishes initialization and indexes a chat before the
        // stale opener enters migrate(). Its source and search rows must survive.
        let first = open(&path).unwrap();
        upsert_source_only(&first, "chat.jsonl", "omp", "v1").unwrap();
        first.execute("INSERT INTO sessions_fts(title, body) VALUES('First chat', 'migration sentinel')", []).unwrap();
        migrate(&mut waiting).unwrap();

        assert_eq!(get_source_version(&waiting, "chat.jsonl").as_deref(), Some("v1"));
        let title: String = waiting.query_row(
            "SELECT title FROM sessions_fts WHERE sessions_fts MATCH 'sentinel'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(title, "First chat");
    }

    #[test]
    fn failed_migration_rolls_back_partial_schema_changes() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("
            CREATE VIRTUAL TABLE sessions_fts USING fts5(title, body);
            INSERT INTO sessions_fts VALUES('Existing chat', 'rollback sentinel');
            CREATE VIEW sources AS SELECT 1;
        ").unwrap();

        // DROP TABLE sources cannot drop a view. Earlier DDL must roll back too.
        assert!(migrate(&mut conn).is_err());
        let title: String = conn.query_row(
            "SELECT title FROM sessions_fts WHERE sessions_fts MATCH 'sentinel'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(title, "Existing chat");
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 0);
    }
}
