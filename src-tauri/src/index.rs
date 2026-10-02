//! Local SQLite index of bundles seen by this portable copy of the app
//! (`<data_dir>/index.sqlite`). It stores only paths, ids and statuses —
//! never payload data — and is skipped entirely when no data dir exists.

use crate::error::AppResult;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct IndexedBundle {
    pub bundle_id: String,
    pub path: String,
    pub computer_name: String,
    pub created_at: String,
    pub status: String,
    pub last_event: String,
    pub updated_at: String,
}

pub struct BundleIndex {
    conn: Connection,
}

impl BundleIndex {
    pub fn open(data_dir: &Path) -> AppResult<Self> {
        let conn = Connection::open(data_dir.join("index.sqlite"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS bundles (
                bundle_id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                computer_name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL,
                last_event TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    pub fn record(&self, bundle_id: &str, path: &str, computer: &str, created_at: &str, status: &str, event: &str) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO bundles (bundle_id, path, computer_name, created_at, status, last_event, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(bundle_id) DO UPDATE SET path=excluded.path, status=excluded.status, last_event=excluded.last_event, updated_at=excluded.updated_at",
            params![bundle_id, path, computer, created_at, status, event, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn list(&self) -> AppResult<Vec<IndexedBundle>> {
        let mut stmt = self.conn.prepare("SELECT bundle_id, path, computer_name, created_at, status, last_event, updated_at FROM bundles ORDER BY updated_at DESC LIMIT 100")?;
        let rows = stmt.query_map([], |r| {
            Ok(IndexedBundle {
                bundle_id: r.get(0)?,
                path: r.get(1)?,
                computer_name: r.get(2)?,
                created_at: r.get(3)?,
                status: r.get(4)?,
                last_event: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn forget(&self, bundle_id: &str) -> AppResult<()> {
        self.conn.execute("DELETE FROM bundles WHERE bundle_id=?1", params![bundle_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_lists() {
        let d = tempfile::tempdir().unwrap();
        let idx = BundleIndex::open(d.path()).unwrap();
        idx.record("b1", "/x", "PC", "2024", "verified", "capture").unwrap();
        idx.record("b1", "/x", "PC", "2024", "verified", "restore").unwrap();
        let l = idx.list().unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].last_event, "restore");
        idx.forget("b1").unwrap();
        assert!(idx.list().unwrap().is_empty());
    }
}
