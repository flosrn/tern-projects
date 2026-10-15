use super::*;
use rusqlite::types::ValueRef;

const PREFIXES: [&str; 6] = [
    "checkpointId",
    "codeBlockDiff",
    "bubbleId",
    "messageRequestContext",
    "codeBlockPartialInlineDiffFates",
    "ofsContent",
];
struct Change {
    table: &'static str,
    key: String,
    before: Vec<u8>,
    after: Option<Vec<u8>>,
    blob: bool,
}
struct Database {
    path: PathBuf,
    changes: Vec<Change>,
}
pub(super) struct Plan {
    root: PathBuf,
    databases: Vec<Database>,
    ids: Vec<String>,
}

fn user_root(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    return home.join("Library/Application Support/Cursor/User");
    #[cfg(not(target_os = "macos"))]
    return home.join(".config/Cursor/User");
}

fn cell(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<(Vec<u8>, bool)> {
    match row.get_ref(column)? {
        ValueRef::Text(v) => Ok((v.to_vec(), false)),
        ValueRef::Blob(v) => Ok((v.to_vec(), true)),
        _ => Err(rusqlite::Error::InvalidColumnType(
            column,
            "value".into(),
            row.get_ref(column)?.data_type(),
        )),
    }
}
fn get(conn: &Connection, table: &str, key: &str) -> Result<Option<(Vec<u8>, bool)>> {
    Ok(conn
        .query_row(
            &format!("SELECT value FROM {table} WHERE key=?1"),
            [key],
            |r| cell(r, 0),
        )
        .optional()?)
}
fn composer(conn: &Connection, id: &str) -> Result<(Value, Vec<u8>, bool)> {
    valid_id(id)?;
    let (bytes, blob) =
        get(conn, "cursorDiskKV", &format!("composerData:{id}"))?.with_context(|| {
            format!("Unsupported Cursor storage layout: required composer {id} is missing")
        })?;
    let value: Value =
        serde_json::from_slice(&bytes).context("Unsupported Cursor composer JSON")?;
    ensure!(
        value.is_object() && value.get("composerId").and_then(Value::as_str) == Some(id),
        "Cursor composerId does not match its storage key"
    );
    Ok((value, bytes, blob))
}
fn child_ids(value: &Value) -> Result<Vec<String>> {
    match value.get("subComposerIds") {
        None => Ok(vec![]),
        Some(v) => v
            .as_array()
            .context("Unsupported Cursor subComposerIds schema")?
            .iter()
            .map(|v| {
                let s = v.as_str().context("Unsupported Cursor child ID schema")?;
                valid_id(s)?;
                Ok(s.to_string())
            })
            .collect(),
    }
}

pub(super) fn prepare(home: &Path, chat: &Chat) -> Result<super::Plan> {
    ensure!(
        chat.source == format!("composerData:{}", chat.id),
        "Cursor source does not match composer ID"
    );
    let root = user_root(home);
    safe_path(home, &root, true)?;
    let global = root.join("globalStorage/state.vscdb");
    let mut stamps = BTreeSet::new();
    add_database(&root, &global, &mut stamps)?;
    let conn = crate::db::open_readonly(&global)?;
    let mut queue = vec![chat.id.clone()];
    let mut metadata = std::collections::BTreeMap::new();
    while let Some(id) = queue.pop() {
        if metadata.contains_key(&id) {
            continue;
        }
        let (value, bytes, blob) = composer(&conn, &id)?;
        queue.extend(child_ids(&value)?);
        metadata.insert(id, (value, bytes, blob));
    }
    let ids = metadata.keys().cloned().collect::<Vec<_>>();
    let mut changes = Vec::new();
    let mut targets = Vec::new();
    for (id, (_, bytes, blob)) in &metadata {
        let key = format!("composerData:{id}");
        targets.push(format!("{}: cursorDiskKV[{key}]", global.display()));
        changes.push(Change {
            table: "cursorDiskKV",
            key,
            before: bytes.clone(),
            after: None,
            blob: *blob,
        });
        for prefix in PREFIXES {
            let prefix = format!("{prefix}:{id}:");
            // Bound exact prefix comparison: IDs containing SQL wildcard characters remain literal.
            let mut stmt = conn.prepare(
                "SELECT key,value FROM cursorDiskKV WHERE substr(key,1,length(?1))=?1 ORDER BY key",
            )?;
            let rows = stmt
                .query_map([&prefix], |r| Ok((r.get::<_, String>(0)?, cell(r, 1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if !rows.is_empty() {
                targets.push(format!(
                    "{}: cursorDiskKV prefix {prefix} ({} owned keys)",
                    global.display(),
                    rows.len()
                ));
            }
            for (key, (before, blob)) in rows {
                changes.push(Change {
                    table: "cursorDiskKV",
                    key,
                    before,
                    after: None,
                    blob,
                });
            }
        }
    }
    // Deleting a best-of-N child must not leave a surviving parent's selection dangling.
    let mut parents = std::collections::BTreeMap::<String, (Value, Vec<u8>, bool)>::new();
    for (id, (value, _, _)) in &metadata {
        if let Some(flag) = value.get("isBestOfNSubcomposer") {
            ensure!(
                flag.is_boolean(),
                "Unsupported Cursor isBestOfNSubcomposer schema"
            );
        }
        if value.get("isBestOfNSubcomposer").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let parent = value
            .pointer("/subagentInfo/parentComposerId")
            .and_then(Value::as_str)
            .context("Unsupported Cursor child metadata: missing parentComposerId")?;
        valid_id(parent)?;
        if metadata.contains_key(parent) {
            continue;
        }
        if !parents.contains_key(parent) {
            parents.insert(parent.to_string(), composer(&conn, parent)?);
        }
        let (parent_value, _, _) = parents.get(parent).unwrap();
        ensure!(
            child_ids(parent_value)?.iter().any(|s| s == id),
            "Cursor parent does not reference the child being deleted"
        );
    }
    for (id, (mut value, before, blob)) in parents {
        let mut children = child_ids(&value)?;
        children.retain(|s| !metadata.contains_key(s));
        let selected = value
            .get("selectedSubComposerId")
            .and_then(Value::as_str)
            .context("Unsupported Cursor parent selectedSubComposerId")?
            .to_string();
        value["subComposerIds"] = json!(children);
        if metadata.contains_key(&selected) {
            value["selectedSubComposerId"] =
                json!(children.first().map(String::as_str).unwrap_or(&id));
        }
        targets.push(format!(
            "{}: composerData:{id} (repair surviving parent's child references)",
            global.display()
        ));
        changes.push(Change {
            table: "cursorDiskKV",
            key: format!("composerData:{id}"),
            before,
            after: Some(serde_json::to_vec(&value)?),
            blob,
        });
    }
    let mut databases = vec![Database {
        path: global.clone(),
        changes,
    }];
    let workspace = root.join("workspaceStorage");
    if workspace.try_exists()? {
        safe_path(&root, &workspace, true)?;
        stamps.insert(workspace.clone());
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&workspace)? {
            let path = entry?.path();
            let m = std::fs::symlink_metadata(&path)?;
            ensure!(
                !m.file_type().is_symlink(),
                "Refusing symlink in Cursor workspaceStorage: {}",
                path.display()
            );
            if !m.is_dir() {
                continue;
            }
            safe_path(&root, &path, true)?;
            stamps.insert(path.clone());
            let db = path.join("state.vscdb");
            if std::fs::symlink_metadata(&db).is_ok() {
                paths.push(db);
            }
        }
        paths.sort();
        for path in paths {
            add_database(&root, &path, &mut stamps)?;
            databases.push(Database {
                path,
                changes: vec![],
            });
        }
    }
    // Parse and validate every relevant header before the first write in any database.
    for database in &mut databases {
        let db = crate::db::open_readonly(&database.path)?;
        ensure!(
            table_exists(&db, "ItemTable")?,
            "Unsupported Cursor storage layout: ItemTable missing in {}",
            database.path.display()
        );
        for key in ["composer.composerHeaders", "composer.composerData"] {
            let Some((before, blob)) = get(&db, "ItemTable", key)? else {
                continue;
            };
            let mut value: Value = serde_json::from_slice(&before).with_context(|| {
                format!(
                    "Unsupported Cursor header JSON in {}:{key}",
                    database.path.display()
                )
            })?;
            let object = value
                .as_object_mut()
                .context("Unsupported Cursor header shape: expected an object")?;
            let headers = object
                .get_mut("allComposers")
                .and_then(Value::as_array_mut)
                .context("Unsupported Cursor header shape: expected allComposers array")?;
            for header in headers.iter() {
                ensure!(
                    header.is_object()
                        && header.get("composerId").and_then(Value::as_str).is_some(),
                    "Unsupported Cursor allComposers entry shape"
                );
            }
            headers.retain(|h| !metadata.contains_key(h["composerId"].as_str().unwrap()));
            for field in ["selectedComposerIds", "lastFocusedComposerIds"] {
                if let Some(v) = object.get_mut(field) {
                    let list = v
                        .as_array_mut()
                        .context("Unsupported Cursor selected/focused composer IDs shape")?;
                    ensure!(
                        list.iter().all(Value::is_string),
                        "Unsupported Cursor selected/focused composer ID"
                    );
                    list.retain(|s| !metadata.contains_key(s.as_str().unwrap()));
                }
            }
            let after = serde_json::to_vec(&value)?;
            if serde_json::from_slice::<Value>(&before)? != value {
                targets.push(format!(
                    "{}: ItemTable[{key}] (remove affected composer IDs)",
                    database.path.display()
                ));
                database.changes.push(Change {
                    table: "ItemTable",
                    key: key.into(),
                    before,
                    after: Some(after),
                    blob,
                });
            }
        }
    }
    Ok(super::Plan {
        operation: Operation::Cursor(Plan {
            root,
            databases,
            ids: ids.clone(),
        }),
        targets,
        ids,
        stamps,
    })
}

pub(super) fn execute(p: &Plan) -> Result<()> {
    let write_set = p
        .databases
        .iter()
        .filter(|db| !db.changes.is_empty())
        .collect::<Vec<_>>();
    ensure!(!write_set.is_empty(), "Missing Cursor source changes");
    let mut connections = Vec::with_capacity(write_set.len());
    for db in &write_set {
        safe_path(&p.root, &db.path, false)?;
        let conn =
            Connection::open_with_flags(&db.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(std::time::Duration::from_millis(0))?;
        connections.push(conn);
    }
    // Separate connections avoid SQLite's ATTACH limit. Acquire every required write lock
    // before changing any source; an acquisition/validation failure rolls everything back.
    let mut transactions = Vec::with_capacity(connections.len());
    for (conn, db) in connections.iter_mut().zip(&write_set) {
        transactions.push(
            conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .with_context(|| {
                    format!(
                        "Cannot lock Cursor source {}; no source changes were committed",
                        db.path.display()
                    )
                })?,
        );
    }
    for (tx, db) in transactions.iter().zip(&write_set) {
        for change in &db.changes {
            let current = tx
                .query_row(
                    &format!("SELECT value FROM {} WHERE key=?1", change.table),
                    [&change.key],
                    |r| cell(r, 0),
                )
                .optional()?;
            ensure!(
                current
                    .as_ref()
                    .is_some_and(|(bytes, blob)| bytes == &change.before && *blob == change.blob),
                "Cursor source changed after preflight; obtain a fresh preview"
            );
        }
    }
    for (tx, db) in transactions.iter().zip(&write_set) {
        for change in &db.changes {
            if let Some(after) = &change.after {
                let sql = format!("UPDATE {} SET value=?1 WHERE key=?2", change.table);
                if change.blob {
                    tx.execute(&sql, params![after, change.key])?;
                } else {
                    tx.execute(&sql, params![std::str::from_utf8(after)?, change.key])?;
                }
            } else {
                tx.execute(
                    &format!("DELETE FROM {} WHERE key=?1", change.table),
                    [&change.key],
                )?;
            }
        }
    }
    // WAL databases cannot provide a crash-atomic cross-file commit. Make any partial
    // cleanup explicit, retain the cache, and never automatically retry the source operation.
    for (index, tx) in transactions.into_iter().enumerate() {
        if let Err(error) = tx.commit() {
            let completed = write_set[..index]
                .iter()
                .map(|db| db.path.display().to_string())
                .collect::<Vec<_>>();
            let remaining = write_set[index..]
                .iter()
                .map(|db| db.path.display().to_string())
                .collect::<Vec<_>>();
            bail!("Cursor source commit failed: {error}. Completed database paths: {}. Remaining database paths: {}. No automatic retry was made",completed.join(", "),remaining.join(", "));
        }
    }
    Ok(())
}

pub(super) fn verify(p: &Plan) -> Result<()> {
    for db in &p.databases {
        let conn = crate::db::open_readonly(&db.path)?;
        for change in &db.changes {
            let actual = get(&conn, change.table, &change.key)?;
            match &change.after {
                Some(after) => ensure!(
                    actual.as_ref().is_some_and(|(bytes, _)| bytes == after),
                    "Cursor source update did not persist: {}:{}",
                    db.path.display(),
                    change.key
                ),
                None => ensure!(
                    actual.is_none(),
                    "Cursor source key survives: {}:{}",
                    db.path.display(),
                    change.key
                ),
            }
        }
        if db.path == p.databases[0].path {
            for id in &p.ids {
                ensure!(
                    get(&conn, "cursorDiskKV", &format!("composerData:{id}"))?.is_none(),
                    "Cursor composer remains after deletion"
                );
                for prefix in PREFIXES {
                    let prefix = format!("{prefix}:{id}:");
                    let count: i64 = conn.query_row(
                        "SELECT count(*) FROM cursorDiskKV WHERE substr(key,1,length(?1))=?1",
                        [prefix],
                        |r| r.get(0),
                    )?;
                    ensure!(count == 0, "Cursor owned prefix remains after deletion");
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct Fixture {
        home: TempDir,
        global: PathBuf,
        workspace: PathBuf,
    }
    fn fixture() -> Fixture {
        let home = tempfile::tempdir().unwrap();
        let root = user_root(home.path());
        let global = root.join("globalStorage/state.vscdb");
        let workspace = root.join("workspaceStorage/fixture/state.vscdb");
        for path in [&global, &workspace] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let conn = Connection::open(path).unwrap();
            conn.execute_batch("CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE ItemTable(key TEXT PRIMARY KEY,value BLOB);").unwrap();
        }
        Fixture {
            home,
            global,
            workspace,
        }
    }
    fn put(path: &Path, table: &str, key: &str, value: Value) {
        let conn = Connection::open(path).unwrap();
        let bytes = serde_json::to_vec(&value).unwrap();
        if table == "ItemTable" {
            conn.execute(
                "INSERT OR REPLACE INTO ItemTable(key,value) VALUES(?1,?2)",
                params![key, bytes],
            )
            .unwrap();
        } else {
            conn.execute(
                "INSERT OR REPLACE INTO cursorDiskKV(key,value) VALUES(?1,?2)",
                params![key, String::from_utf8(bytes).unwrap()],
            )
            .unwrap();
        }
    }
    fn value(path: &Path, table: &str, key: &str) -> Option<Value> {
        let conn = crate::db::open_readonly(path).unwrap();
        get(&conn, table, key)
            .unwrap()
            .map(|(bytes, _)| serde_json::from_slice(&bytes).unwrap())
    }
    fn chat(id: &str) -> Chat {
        Chat {
            uid: format!("cursor:{id}"),
            agent: "cursor".into(),
            id: id.into(),
            source: format!("composerData:{id}"),
            title: "Synthetic fixture".into(),
            version: "1".into(),
        }
    }
    fn seed_headers(f: &Fixture) {
        for path in [&f.global, &f.workspace] {
            for key in ["composer.composerHeaders", "composer.composerData"] {
                put(
                    path,
                    "ItemTable",
                    key,
                    json!({"allComposers":[{"composerId":"parent","name":"keep fields"},{"composerId":"child"},{"composerId":"parent-extra"}],"selectedComposerIds":["parent","parent-extra"],"lastFocusedComposerIds":["child","parent-extra"],"unrelated":{"preserve":true}}),
                );
            }
        }
    }
    fn seed_composers(f: &Fixture) {
        put(
            &f.global,
            "cursorDiskKV",
            "composerData:parent",
            json!({"composerId":"parent","subComposerIds":["child"],"selectedSubComposerId":"child","gitWorktree":{"worktreePath":"/never/remove/worktree"},"name":"Parent"}),
        );
        put(
            &f.global,
            "cursorDiskKV",
            "composerData:child",
            json!({"composerId":"child","subComposerIds":[],"isBestOfNSubcomposer":true,"subagentInfo":{"parentComposerId":"parent"}}),
        );
        put(
            &f.global,
            "cursorDiskKV",
            "composerData:parent-extra",
            json!({"composerId":"parent-extra","name":"Neighbor"}),
        );
        for id in ["parent", "child", "parent-extra"] {
            for prefix in PREFIXES {
                put(
                    &f.global,
                    "cursorDiskKV",
                    &format!("{prefix}:{id}:fixture"),
                    json!({"keep":id}),
                );
            }
        }
        put(
            &f.global,
            "cursorDiskKV",
            "agentKv:blob:shared",
            json!({"shared":true}),
        );
        put(
            &f.global,
            "ItemTable",
            "unrelated-key",
            json!({"preserve":"all"}),
        );
        seed_headers(f);
    }

    #[test]
    fn deletes_only_explicit_composers_and_owned_prefixes_across_headers() {
        let f = fixture();
        seed_composers(&f);
        let worktree = f.home.path().join("working-tree");
        std::fs::create_dir(&worktree).unwrap();
        std::fs::write(worktree.join("source.rs"), "unchanged source").unwrap();
        let prepared = prepare(f.home.path(), &chat("parent")).unwrap();
        assert_eq!(prepared.ids, vec!["child", "parent"]);
        assert!(prepared
            .targets
            .iter()
            .any(|p| p.contains("workspaceStorage")));
        let Operation::Cursor(plan) = prepared.operation else {
            panic!("wrong operation")
        };
        execute(&plan).unwrap();
        verify(&plan).unwrap();
        assert!(value(&f.global, "cursorDiskKV", "composerData:parent").is_none());
        assert!(value(&f.global, "cursorDiskKV", "composerData:child").is_none());
        assert_eq!(
            value(&f.global, "cursorDiskKV", "composerData:parent-extra").unwrap()["name"],
            "Neighbor"
        );
        for prefix in PREFIXES {
            assert!(value(
                &f.global,
                "cursorDiskKV",
                &format!("{prefix}:parent-extra:fixture")
            )
            .is_some());
        }
        assert_eq!(
            value(&f.global, "cursorDiskKV", "agentKv:blob:shared").unwrap(),
            json!({"shared":true})
        );
        assert_eq!(
            value(&f.global, "ItemTable", "unrelated-key").unwrap(),
            json!({"preserve":"all"})
        );
        for path in [&f.global, &f.workspace] {
            for key in ["composer.composerHeaders", "composer.composerData"] {
                let v = value(path, "ItemTable", key).unwrap();
                assert_eq!(v["allComposers"], json!([{"composerId":"parent-extra"}]));
                assert_eq!(v["selectedComposerIds"], json!(["parent-extra"]));
                assert_eq!(v["lastFocusedComposerIds"], json!(["parent-extra"]));
                assert_eq!(v["unrelated"], json!({"preserve":true}));
            }
        }
        assert_eq!(
            std::fs::read_to_string(worktree.join("source.rs")).unwrap(),
            "unchanged source"
        );
    }

    #[test]
    fn deleting_best_of_n_child_repairs_surviving_parent() {
        let f = fixture();
        seed_composers(&f);
        let prepared = prepare(f.home.path(), &chat("child")).unwrap();
        assert_eq!(prepared.ids, vec!["child"]);
        let Operation::Cursor(plan) = prepared.operation else {
            panic!("wrong operation")
        };
        execute(&plan).unwrap();
        verify(&plan).unwrap();
        let parent = value(&f.global, "cursorDiskKV", "composerData:parent").unwrap();
        assert_eq!(parent["subComposerIds"], json!([]));
        assert_eq!(parent["selectedSubComposerId"], "parent");
        assert_eq!(
            parent["gitWorktree"]["worktreePath"],
            "/never/remove/worktree"
        );
        assert!(value(&f.global, "cursorDiskKV", "bubbleId:parent:fixture").is_some());
    }

    #[test]
    fn unknown_workspace_headers_refuse_before_any_source_write() {
        let f = fixture();
        seed_composers(&f);
        put(
            &f.workspace,
            "ItemTable",
            "composer.composerHeaders",
            json!({"allComposers":"unknown"}),
        );
        let before = std::fs::read(&f.global).unwrap();
        assert!(prepare(f.home.path(), &chat("parent")).is_err());
        assert_eq!(std::fs::read(&f.global).unwrap(), before);
        assert!(value(&f.global, "cursorDiskKV", "composerData:parent").is_some());
        assert!(value(&f.global, "cursorDiskKV", "composerData:child").is_some());
    }

    #[test]
    fn malformed_required_parent_refuses_without_deleting_child() {
        let f = fixture();
        seed_composers(&f);
        put(
            &f.global,
            "cursorDiskKV",
            "composerData:parent",
            json!({"composerId":"parent","subComposerIds":[],"selectedSubComposerId":"child"}),
        );
        assert!(prepare(f.home.path(), &chat("child")).is_err());
        assert!(value(&f.global, "cursorDiskKV", "composerData:child").is_some());
    }
}

#[cfg(test)]
mod preview_summary_tests {
    use super::*;

    #[test]
    fn summarizes_owned_prefixes_without_trimming_the_exact_inventory() {
        let home = tempfile::tempdir().unwrap();
        let root = user_root(home.path());
        let global = root.join("globalStorage/state.vscdb");
        std::fs::create_dir_all(global.parent().unwrap()).unwrap();
        let mut db = Connection::open(&global).unwrap();
        db.execute_batch("CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE ItemTable(key TEXT PRIMARY KEY,value BLOB);
            INSERT INTO cursorDiskKV VALUES('composerData:target','{\"composerId\":\"target\"}');").unwrap();
        let tx = db.transaction().unwrap();
        for i in 0..1000 {
            tx.execute(
                "INSERT INTO cursorDiskKV VALUES(?1,'{}')",
                [format!("bubbleId:target:{i}")],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let chat = Chat {
            uid: "cursor:target".into(),
            agent: "cursor".into(),
            id: "target".into(),
            source: "composerData:target".into(),
            title: "Synthetic fixture".into(),
            version: "1".into(),
        };
        let prepared = prepare(home.path(), &chat).unwrap();
        assert_eq!(
            prepared.targets.len(),
            2,
            "one composer line and one owned-prefix summary"
        );
        assert!(prepared
            .targets
            .iter()
            .any(|line| line.contains(&global.display().to_string())
                && line.contains("bubbleId:target: (1000 owned keys)")));
        let Operation::Cursor(plan) = prepared.operation else {
            panic!("wrong operation")
        };
        let changes = &plan.databases[0].changes;
        assert_eq!(
            changes.len(),
            1001,
            "full preflight inventory remains intact"
        );
        let keys = changes
            .iter()
            .map(|change| change.key.as_str())
            .collect::<BTreeSet<_>>();
        assert!(changes
            .iter()
            .filter(|change| change.key.starts_with("bubbleId:"))
            .all(|change| change.before == b"{}" && change.after.is_none()));
        for i in 0..1000 {
            assert!(keys.contains(format!("bubbleId:target:{i}").as_str()));
        }
    }
}
