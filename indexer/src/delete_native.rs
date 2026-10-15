use super::*;
use std::process::{Command, Stdio};

pub(super) struct Plan {
    agent: String,
    executable: PathBuf,
    id: String,
    ids: Vec<String>,
    databases: Vec<(PathBuf, String, String)>,
    rollouts: Vec<PathBuf>,
    pub(super) state: Vec<Value>,
}

pub(super) fn prepare(home: &Path, chat: &Chat) -> Result<super::Plan> {
    valid_id(&chat.id)?;
    let executable = crate::harnesses::native_delete_executable(home, &chat.agent)?;
    inspect(home, chat, executable)
}

fn inspect(home: &Path, chat: &Chat, executable: PathBuf) -> Result<super::Plan> {
    let mut stamps = BTreeSet::new();
    stamps.insert(executable.canonicalize()?);
    let mut p = Plan {
        agent: chat.agent.clone(),
        executable,
        id: chat.id.clone(),
        ids: vec![chat.id.clone()],
        databases: vec![],
        rollouts: vec![],
        state: vec![],
    };
    let mut targets = vec![];
    if chat.agent == "codex" {
        ensure!(
            is_uuid(&chat.id),
            "Codex permanent deletion requires an exact UUID, never a name or path"
        );
        let root = home.join(".codex");
        safe_path(home, &root, true)?;
        let mut paths = std::fs::read_dir(&root)?
            .map(|entry| Ok(entry?.path()))
            .collect::<Result<Vec<_>>>()?;
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .context("Missing Codex database name")?
                .to_string_lossy();
            let table = if versioned(&name, "state_") {
                Some(("threads", "id"))
            } else if versioned(&name, "thread_history_") {
                Some(("thread_items", "thread_id"))
            } else {
                None
            };
            if let Some((table, column)) = table {
                validate_database(&root, &path)?;
                let conn = crate::db::open_readonly(&path)?;
                let conn = conn.unchecked_transaction()?;
                snapshot_rows(&conn, &path, table, column, &p.ids, &mut p.state)?;
                let count: i64 = conn.query_row(
                    &format!("SELECT count(*) FROM {table} WHERE {column}=?1"),
                    [&chat.id],
                    |r| r.get(0),
                )?;
                if count > 0 {
                    targets.push(format!(
                        "{}: {table}[{column}={}] (native cleanup)",
                        path.display(),
                        chat.id
                    ));
                }
                if table == "threads" && count > 0 {
                    let rollout: Option<String> = conn.query_row(
                        "SELECT rollout_path FROM threads WHERE id=?1",
                        [&chat.id],
                        |r| r.get(0),
                    )?;
                    if let Some(rollout) = rollout.filter(|s| !s.is_empty()) {
                        let path = if Path::new(&rollout).is_absolute() {
                            PathBuf::from(rollout)
                        } else {
                            home.join(rollout)
                        };
                        if path.try_exists()? {
                            safe_path(&root, &path, false)?;
                            stamps.insert(path.clone());
                            p.rollouts.push(path);
                        }
                    }
                }
                p.databases.push((path, table.into(), column.into()));
            }
        }
        if let Some(path) = chat
            .source
            .strip_prefix("codex:")
            .filter(|s| Path::new(s).is_absolute())
        {
            let path = PathBuf::from(path);
            safe_path(&root, &path, false)?;
            stamps.insert(path.clone());
            p.rollouts.push(path);
        }
        // List every matching rollout, including archived copies: native code must remove them all.
        for directory in ["sessions", "archived_sessions"] {
            let path = root.join(directory);
            if path.try_exists()? {
                collect_rollouts(&root, &path, &chat.id, &mut p.rollouts, &mut stamps)?;
            }
        }
        p.rollouts.sort();
        p.rollouts.dedup();
        targets.extend(p.rollouts.iter().map(|p| p.display().to_string()));
        ensure!(
            !targets.is_empty(),
            "No verifiable Codex source rows or rollouts remain"
        );
    } else {
        ensure!(
            chat.source == format!("opencode:{}", chat.id),
            "OpenCode source ID mismatch"
        );
        let root = home.join(".local/share/opencode");
        safe_path(home, &root, true)?;
        let path = root.join("opencode.db");
        validate_database(&root, &path)?;
        let conn = crate::db::open_readonly(&path)?;
        let conn = conn.unchecked_transaction()?;
        let mut queue = vec![chat.id.clone()];
        let mut seen = BTreeSet::new();
        while let Some(id) = queue.pop() {
            ensure!(
                seen.insert(id.clone()),
                "OpenCode session hierarchy contains a cycle or duplicate child"
            );
            let mut stmt = conn.prepare("SELECT id FROM session WHERE parent_id=?1 ORDER BY id")?;
            let children = stmt
                .query_map([&id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for child in children {
                valid_id(&child)?;
                queue.push(child);
            }
        }
        p.ids = seen.into_iter().collect();
        for id in &p.ids {
            targets.push(format!(
                "{}: session[id={id}] and native-owned messages/parts",
                path.display()
            ));
        }
        // Required source schemas are checked before invoking the native mutation.
        for (table, column) in [
            ("session", "id"),
            ("message", "session_id"),
            ("part", "session_id"),
        ] {
            conn.prepare(&format!("SELECT {column} FROM {table} LIMIT 0"))?;
            snapshot_rows(&conn, &path, table, column, &p.ids, &mut p.state)?;
            p.databases
                .push((path.clone(), table.into(), column.into()));
        }
    }
    Ok(super::Plan {
        ids: p.ids.clone(),
        operation: Operation::Native(p),
        targets,
        stamps,
    })
}

// Preserve SQLite value types and every column, including ownership/reference fields.
// Sorting exact encoded rows makes the token independent of query plans and insertion order.
fn snapshot_rows(
    conn: &Connection,
    path: &Path,
    table: &str,
    column: &str,
    ids: &[String],
    state: &mut Vec<Value>,
) -> Result<()> {
    use rusqlite::types::ValueRef;
    let mut stmt = conn.prepare(&format!("SELECT * FROM {table} WHERE {column}=?1"))?;
    let columns = stmt
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for id in ids {
        let mut query = stmt.query([id])?;
        while let Some(row) = query.next()? {
            let mut cells = Vec::with_capacity(columns.len());
            for i in 0..columns.len() {
                cells.push(match row.get_ref(i)? {
                    ValueRef::Null => json!(["null"]),
                    ValueRef::Integer(v) => json!(["integer", v]),
                    ValueRef::Real(v) => json!(["real", v.to_bits()]),
                    ValueRef::Text(v) => json!(["text", v]),
                    ValueRef::Blob(v) => json!(["blob", v]),
                });
            }
            rows.push(serde_json::to_string(&cells)?);
        }
    }
    if !rows.is_empty() {
        rows.sort();
        state.push(
            json!({"path":path,"table":table,"owner_column":column,"columns":columns,"rows":rows}),
        );
    }
    Ok(())
}

fn validate_database(root: &Path, path: &Path) -> Result<()> {
    safe_path(root, path, false)?;
    for suffix in ["-wal", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.to_string_lossy()));
        if sidecar.try_exists()? {
            safe_path(root, &sidecar, false)?;
        }
    }
    Ok(())
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn versioned(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(".sqlite"))
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
fn collect_rollouts(
    root: &Path,
    path: &Path,
    id: &str,
    out: &mut Vec<PathBuf>,
    stamps: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    safe_path(root, path, true)?;
    for e in std::fs::read_dir(path)? {
        let e = e?;
        let p = e.path();
        let m = std::fs::symlink_metadata(&p)?;
        ensure!(
            !m.file_type().is_symlink(),
            "Codex session store contains a symlink: {}",
            p.display()
        );
        if m.is_dir() {
            collect_rollouts(root, &p, id, out, stamps)?;
        } else if p.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
            s.ends_with(&format!("{id}.jsonl")) || s.ends_with(&format!("{id}.jsonl.zst"))
        }) {
            safe_path(root, &p, false)?;
            stamps.insert(p.clone());
            out.push(p);
        }
    }
    Ok(())
}

pub(super) fn execute(home: &Path, p: &Plan) -> Result<()> {
    // Re-run identity/capability discovery at the point of mutation; no raw-SQL fallback exists.
    let executable = crate::harnesses::native_delete_executable(home, &p.agent)?;
    ensure!(
        executable == p.executable,
        "Harness executable changed; obtain a fresh deletion preview"
    );
    let mut command = Command::new(executable);
    command
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CODEX_HOME", home.join(".codex"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("OPENCODE_CONFIG_DIR", home.join(".config/opencode"));
    for key in [
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_CONTENT",
        "OPENCODE_SERVER_URL",
        "OPENCODE_SERVER_USERNAME",
        "OPENCODE_SERVER_PASSWORD",
        "CODEX_REMOTE_URL",
    ] {
        command.env_remove(key);
    }
    if p.agent == "codex" {
        command.args(["delete", &p.id, "--force"]);
    } else {
        command.args(["session", "delete", &p.id]);
    }
    let output = command
        .output()
        .with_context(|| format!("Running {} native permanent deletion", p.agent))?;
    ensure!(output.status.success(),"{} native deletion failed ({}): {}. No cache rows were removed; obtain a new preview before another attempt",p.agent,output.status,String::from_utf8_lossy(&output.stderr).trim());
    Ok(())
}

pub(super) fn verify(_home: &Path, p: &Plan) -> Result<()> {
    for (path, table, column) in &p.databases {
        let conn = crate::db::open_readonly(path)?;
        for id in &p.ids {
            let count: i64 = conn.query_row(
                &format!("SELECT count(*) FROM {table} WHERE {column}=?1"),
                [id],
                |r| r.get(0),
            )?;
            ensure!(
                count == 0,
                "Native deletion left {table} rows for {id} in {}; source cleanup is incomplete",
                path.display()
            );
        }
    }
    for path in &p.rollouts {
        ensure!(
            std::fs::symlink_metadata(path)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "Native deletion left rollout {}; source cleanup is incomplete",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "735b37da-cf2f-44b5-8d25-e3a768f5eac0";

    fn chat(agent: &str, id: &str) -> Chat {
        Chat {
            uid: format!("{agent}:{id}"),
            agent: agent.into(),
            id: id.into(),
            source: format!("{agent}:{id}"),
            title: "Synthetic fixture".into(),
            version: "1".into(),
        }
    }
    fn inspected(home: &Path, chat: &Chat) -> super::super::Plan {
        // Inventory tests never discover or invoke a harness CLI.
        inspect(home, chat, std::env::current_exe().unwrap()).unwrap()
    }
    fn version(home: &Path, chat: &Chat) -> String {
        super::super::token(chat, &inspected(home, chat)).unwrap()
    }

    #[test]
    fn codex_confirmation_tracks_only_owned_rows_and_exact_rollouts() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".codex");
        let directory = root.join("sessions/2026/10/15");
        std::fs::create_dir_all(&directory).unwrap();
        let rollout = directory.join(format!("rollout-{UUID}.jsonl"));
        std::fs::write(&rollout, "synthetic rollout").unwrap();
        let state = Connection::open(root.join("state_5.sqlite")).unwrap();
        state.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE threads(id TEXT PRIMARY KEY,rollout_path TEXT,title TEXT,updated_at_ms INTEGER);").unwrap();
        state
            .execute(
                "INSERT INTO threads VALUES(?1,?2,'Target',1)",
                params![UUID, rollout.to_str().unwrap()],
            )
            .unwrap();
        state
            .execute(
                "INSERT INTO threads VALUES('neighbor',NULL,'Neighbor',1)",
                [],
            )
            .unwrap();
        let history = Connection::open(root.join("thread_history_1.sqlite")).unwrap();
        history.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE thread_items(thread_id TEXT,item_type TEXT,item_json TEXT,rollout_ordinal INTEGER);").unwrap();
        history
            .execute(
                "INSERT INTO thread_items VALUES(?1,'userMessage','target content',0)",
                [UUID],
            )
            .unwrap();
        let target = chat("codex", UUID);
        let original = version(home.path(), &target);
        assert!(
            !original.contains("target content"),
            "confirmation metadata must not disclose transcript bodies"
        );

        state
            .execute(
                "UPDATE threads SET title='Other activity',updated_at_ms=2 WHERE id='neighbor'",
                [],
            )
            .unwrap();
        history
            .execute(
                "INSERT INTO thread_items VALUES('neighbor','userMessage','unrelated history',0)",
                [],
            )
            .unwrap();
        std::fs::write(root.join("unrelated-config"), "other activity").unwrap();
        std::fs::create_dir_all(root.join("sessions/2026/10/16")).unwrap();
        std::fs::write(directory.join("other.jsonl"), "unrelated rollout").unwrap();
        assert_eq!(original, version(home.path(), &target));
        let manifest = inspected(home.path(), &target);
        assert_eq!(
            manifest.stamps.len(),
            2,
            "only executable and exact target rollout are stamped"
        );

        state
            .execute(
                "UPDATE threads SET title='Changed without updating timestamp' WHERE id=?1",
                [UUID],
            )
            .unwrap();
        let row_changed = version(home.path(), &target);
        assert_ne!(original, row_changed);
        history
            .execute(
                "UPDATE thread_items SET item_json='changed target content' WHERE thread_id=?1",
                [UUID],
            )
            .unwrap();
        let history_changed = version(home.path(), &target);
        assert_ne!(row_changed, history_changed);
        std::fs::write(&rollout, "changed synthetic rollout content").unwrap();
        let rollout_changed = version(home.path(), &target);
        assert_ne!(history_changed, rollout_changed);
        let archived = root.join("archived_sessions");
        std::fs::create_dir(&archived).unwrap();
        std::fs::write(
            archived.join(format!("rollout-{UUID}.jsonl")),
            "target archive",
        )
        .unwrap();
        let archive_changed = version(home.path(), &target);
        assert_ne!(rollout_changed, archive_changed);
        let mut reindexed = target;
        reindexed.version = "2".into();
        assert_ne!(
            archive_changed,
            version(home.path(), &reindexed),
            "index source version remains part of confirmation"
        );
    }

    #[test]
    fn opencode_confirmation_tracks_descendants_messages_parts_and_ownership() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/opencode");
        std::fs::create_dir_all(&root).unwrap();
        let db = Connection::open(root.join("opencode.db")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE session(id TEXT PRIMARY KEY,parent_id TEXT,title TEXT,time_updated INTEGER);
            CREATE TABLE message(id TEXT PRIMARY KEY,session_id TEXT,data TEXT);
            CREATE TABLE part(id TEXT PRIMARY KEY,session_id TEXT,message_id TEXT,data TEXT);
            INSERT INTO session VALUES('target',NULL,'Target',1),('child','target','Child',1),('neighbor',NULL,'Neighbor',1);
            INSERT INTO message VALUES('message','child','child message'),('other','neighbor','other message');
            INSERT INTO part VALUES('part','child','message','child part'),('other','neighbor','other','other part');").unwrap();
        let target = chat("opencode", "target");
        let original = version(home.path(), &target);
        assert_eq!(inspected(home.path(), &target).ids, vec!["child", "target"]);

        db.execute_batch(
            "UPDATE session SET title='Unrelated update',time_updated=2 WHERE id='neighbor';
            UPDATE message SET data='unrelated message update' WHERE session_id='neighbor';
            INSERT INTO part VALUES('another','neighbor','other','unrelated new part');",
        )
        .unwrap();
        assert_eq!(original, version(home.path(), &target));
        assert_eq!(inspected(home.path(), &target).stamps.len(), 1);

        db.execute(
            "UPDATE part SET data='changed child part' WHERE id='part'",
            [],
        )
        .unwrap();
        let part_changed = version(home.path(), &target);
        assert_ne!(original, part_changed);
        db.execute(
            "UPDATE message SET data='changed child message' WHERE id='message'",
            [],
        )
        .unwrap();
        let message_changed = version(home.path(), &target);
        assert_ne!(part_changed, message_changed);
        db.execute(
            "INSERT INTO session VALUES('grandchild','child','Grandchild',1)",
            [],
        )
        .unwrap();
        let descendant_changed = version(home.path(), &target);
        assert_ne!(message_changed, descendant_changed);
        assert_eq!(
            inspected(home.path(), &target).ids,
            vec!["child", "grandchild", "target"]
        );
        db.execute(
            "UPDATE part SET message_id='different-owner' WHERE id='part'",
            [],
        )
        .unwrap();
        assert_ne!(descendant_changed, version(home.path(), &target));
        db.execute("DROP TABLE part", []).unwrap();
        assert!(
            inspect(home.path(), &target, std::env::current_exe().unwrap()).is_err(),
            "unknown required schema must refuse preflight"
        );
    }

    #[test]
    fn row_fingerprint_is_order_independent_and_preserves_sqlite_types() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE thread_items(thread_id TEXT,item_json);
            INSERT INTO thread_items VALUES('target','first'),('target','second');",
        )
        .unwrap();
        let ids = vec!["target".into()];
        let path = Path::new("/synthetic/history.sqlite");
        let mut original = vec![];
        snapshot_rows(&db, path, "thread_items", "thread_id", &ids, &mut original).unwrap();
        db.execute_batch("DELETE FROM thread_items; INSERT INTO thread_items VALUES('target','second'),('target','first');").unwrap();
        let mut reordered = vec![];
        snapshot_rows(&db, path, "thread_items", "thread_id", &ids, &mut reordered).unwrap();
        assert_eq!(original, reordered);
        db.execute(
            "UPDATE thread_items SET item_json=CAST(item_json AS BLOB) WHERE item_json='first'",
            [],
        )
        .unwrap();
        let mut blob = vec![];
        snapshot_rows(&db, path, "thread_items", "thread_id", &ids, &mut blob).unwrap();
        assert_ne!(original, blob);
    }
}
