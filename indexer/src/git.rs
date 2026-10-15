use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct GitInfo {
    pub worktree_root: String,
    pub project_root: String,
    pub branch: Option<String>,
}

/// Resolve git info for a cwd, using `cache` to avoid repeated disk reads.
pub fn resolve(cwd: &str, cache: &mut HashMap<String, GitInfo>) -> GitInfo {
    if cwd.is_empty() {
        return GitInfo::default();
    }
    if let Some(info) = cache.get(cwd) {
        return info.clone();
    }
    let result = resolve_inner(cwd);
    cache.insert(cwd.to_string(), result.clone());
    result
}

fn resolve_inner(cwd: &str) -> GitInfo {
    let start = Path::new(cwd);

    // Walk up from cwd, skipping non-existent leaf components.
    // Find the first existing directory to start walking from.
    let mut current = start.to_path_buf();
    while !current.exists() {
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => return fallback(cwd),
        }
    }
    // Ensure we're starting from a directory.
    if !current.is_dir() {
        current = match current.parent() {
            Some(p) => p.to_path_buf(),
            None => return fallback(cwd),
        };
    }

    // Walk up looking for .git.
    let mut dir = current.clone();
    loop {
        let git_path = dir.join(".git");
        if git_path.exists() {
            return match parse_git(&dir, &git_path) {
                Ok(info) => info,
                Err(_) => fallback(cwd),
            };
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }

    // No .git found.
    fallback(cwd)
}

fn fallback(cwd: &str) -> GitInfo {
    GitInfo {
        worktree_root: cwd.to_string(),
        project_root: cwd.to_string(),
        branch: None,
    }
}

fn parse_git(worktree_dir: &Path, git_path: &Path) -> anyhow::Result<GitInfo> {
    let worktree_root = worktree_dir.to_string_lossy().into_owned();

    // Determine whether .git is a directory or a file.
    let (gitdir, is_worktree) = if git_path.is_dir() {
        (git_path.to_path_buf(), false)
    } else {
        // .git is a file: "gitdir: <relative-path>"
        let content = std::fs::read_to_string(git_path)?;
        let rel = content
            .lines()
            .find_map(|l| l.strip_prefix("gitdir:").map(str::trim))
            .ok_or_else(|| anyhow::anyhow!("no gitdir line"))?;
        let gitdir = if Path::new(rel).is_absolute() {
            PathBuf::from(rel)
        } else {
            worktree_dir.join(rel)
        };
        let gitdir = normalize_path(&gitdir);
        (gitdir, true)
    };

    // Determine branch from HEAD.
    let head_path = gitdir.join("HEAD");
    let branch = read_branch(&head_path);

    // Determine project_root.
    let project_root = if is_worktree {
        // Read <gitdir>/commondir (relative to gitdir).
        let commondir_path = gitdir.join("commondir");
        if let Ok(rel) = std::fs::read_to_string(&commondir_path) {
            let rel = rel.trim();
            let common = if Path::new(rel).is_absolute() {
                PathBuf::from(rel)
            } else {
                gitdir.join(rel)
            };
            let common = normalize_path(&common);
            // project_root = parent of common when its basename is ".git", else common itself.
            if common.file_name().map(|n| n == ".git").unwrap_or(false) {
                common
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| worktree_root.clone())
            } else {
                common.to_string_lossy().into_owned()
            }
        } else {
            // No commondir; use parent of gitdir's parent (worktrees/<name> -> .git -> project).
            gitdir
                .parent() // worktrees/<name>
                .and_then(|p| p.parent()) // .git
                .and_then(|p| p.parent()) // project root
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| worktree_root.clone())
        }
    } else {
        worktree_root.clone()
    };

    Ok(GitInfo {
        worktree_root,
        project_root,
        branch,
    })
}

fn read_branch(head: &Path) -> Option<String> {
    let content = std::fs::read_to_string(head).ok()?;
    let line = content.lines().next()?.trim();
    // "ref: refs/heads/<branch>"
    line.strip_prefix("ref: refs/heads/").map(|b| b.to_string())
}

/// Resolve `..` components without calling std::fs::canonicalize (which requires existence).
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        use std::path::Component::*;
        match component {
            CurDir => {}
            ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
