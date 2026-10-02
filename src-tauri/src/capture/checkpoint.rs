//! Per-bundle SQLite checkpoint store (`logs/checkpoint.sqlite`). Records
//! every planned and completed file so an interrupted capture resumes
//! without recopying finished files, and scales to very large file counts.

use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Pending,
    Done,
    Skipped,
    Failed,
}

impl FileStatus {
    fn as_str(self) -> &'static str {
        match self {
            FileStatus::Pending => "pending",
            FileStatus::Done => "done",
            FileStatus::Skipped => "skipped",
            FileStatus::Failed => "failed",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "done" => FileStatus::Done,
            "skipped" => FileStatus::Skipped,
            "failed" => FileStatus::Failed,
            _ => FileStatus::Pending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub task_id: String,
    /// Item-relative source path (forward slashes).
    pub rel: String,
    pub size: u64,
    pub mtime: i64,
    pub status: FileStatus,
    /// Bundle-relative stored path.
    pub stored_path: Option<String>,
    pub stored_hash: Option<String>,
    pub plain_hash: Option<String>,
    pub stored_size: Option<u64>,
}

pub struct Checkpoint {
    conn: Connection,
}

impl Checkpoint {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS files (
                task_id TEXT NOT NULL,
                rel TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                status TEXT NOT NULL,
                stored_path TEXT,
                stored_hash TEXT,
                plain_hash TEXT,
                stored_size INTEGER,
                error TEXT,
                PRIMARY KEY (task_id, rel)
            );
            CREATE TABLE IF NOT EXISTS tasks (
                task_id TEXT PRIMARY KEY,
                state TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    pub fn get(&self, task_id: &str, rel: &str) -> AppResult<Option<FileRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT size, mtime, status, stored_path, stored_hash, plain_hash, stored_size FROM files WHERE task_id=?1 AND rel=?2",
                params![task_id, rel],
                |r| {
                    Ok(FileRecord {
                        task_id: task_id.to_string(),
                        rel: rel.to_string(),
                        size: r.get::<_, i64>(0)? as u64,
                        mtime: r.get(1)?,
                        status: FileStatus::parse(&r.get::<_, String>(2)?),
                        stored_path: r.get(3)?,
                        stored_hash: r.get(4)?,
                        plain_hash: r.get(5)?,
                        stored_size: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                    })
                },
            )
            .optional()?)
    }

    pub fn upsert(&self, rec: &FileRecord, error: Option<&str>) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO files (task_id, rel, size, mtime, status, stored_path, stored_hash, plain_hash, stored_size, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(task_id, rel) DO UPDATE SET size=excluded.size, mtime=excluded.mtime, status=excluded.status,
               stored_path=excluded.stored_path, stored_hash=excluded.stored_hash, plain_hash=excluded.plain_hash,
               stored_size=excluded.stored_size, error=excluded.error",
            params![
                rec.task_id,
                rec.rel,
                rec.size as i64,
                rec.mtime,
                rec.status.as_str(),
                rec.stored_path,
                rec.stored_hash,
                rec.plain_hash,
                rec.stored_size.map(|v| v as i64),
                error
            ],
        )?;
        Ok(())
    }

    pub fn set_task_state(&self, task_id: &str, state: &str) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO tasks (task_id, state, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(task_id) DO UPDATE SET state=excluded.state, updated_at=excluded.updated_at",
            params![task_id, state, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn task_state(&self, task_id: &str) -> AppResult<Option<String>> {
        Ok(self.conn.query_row("SELECT state FROM tasks WHERE task_id=?1", params![task_id], |r| r.get(0)).optional()?)
    }

    /// All completed files for a task (used to build hash lists on resume).
    pub fn done_files(&self, task_id: &str) -> AppResult<Vec<FileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT rel, size, mtime, stored_path, stored_hash, plain_hash, stored_size FROM files WHERE task_id=?1 AND status='done' ORDER BY rel")?;
        let rows = stmt.query_map(params![task_id], |r| {
            Ok(FileRecord {
                task_id: task_id.to_string(),
                rel: r.get(0)?,
                size: r.get::<_, i64>(1)? as u64,
                mtime: r.get(2)?,
                status: FileStatus::Done,
                stored_path: r.get(3)?,
                stored_hash: r.get(4)?,
                plain_hash: r.get(5)?,
                stored_size: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Drop records of files that no longer exist in the source (resume after source change).
    pub fn retain_only(&self, task_id: &str, rels: &std::collections::HashSet<String>) -> AppResult<u64> {
        let mut stmt = self.conn.prepare("SELECT rel FROM files WHERE task_id=?1")?;
        let existing: Vec<String> = stmt.query_map(params![task_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut removed = 0;
        for rel in existing.into_iter().filter(|r| !rels.contains(r)) {
            self.conn.execute("DELETE FROM files WHERE task_id=?1 AND rel=?2", params![task_id, rel])?;
            removed += 1;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let cp = Checkpoint::open(&d.path().join("c.sqlite")).unwrap();
        let mut r = FileRecord {
            task_id: "t".into(),
            rel: "a/b.txt".into(),
            size: 5,
            mtime: 1,
            status: FileStatus::Pending,
            stored_path: None,
            stored_hash: None,
            plain_hash: None,
            stored_size: None,
        };
        cp.upsert(&r, None).unwrap();
        assert_eq!(cp.get("t", "a/b.txt").unwrap().unwrap().status, FileStatus::Pending);
        r.status = FileStatus::Done;
        r.stored_hash = Some("x".into());
        cp.upsert(&r, None).unwrap();
        assert_eq!(cp.done_files("t").unwrap().len(), 1);
        cp.set_task_state("t", "completed").unwrap();
        assert_eq!(cp.task_state("t").unwrap().as_deref(), Some("completed"));
        let keep = std::collections::HashSet::new();
        assert_eq!(cp.retain_only("t", &keep).unwrap(), 1);
    }
}
