use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::{fs, path::Path, process::Command};

#[derive(Debug, Clone)]
pub struct ScaffoldRecord {
    pub id: i64,
    pub description: String, // original user prompt
    pub content: String,     // full scaffold text
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ScaffoldStore {
    conn: rusqlite::Connection,
}

impl ScaffoldStore {
    // open - this opens the database at the default location
    pub fn open() -> Result<Self> {
        let path = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("could not resolve data dir"))?
            .join("rust-tutor-mcp")
            .join(Self::detect_project_slug())
            .join("scaffold.db");
        Self::open_at(&path)
    }

    // open_at - this opens a database at a specific path
    fn open_at(path: &Path) -> Result<Self> {
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("db path has no parent directory"))?,
        )?;

        // create a connection to the database here to be able to creat the tables
        let conn = rusqlite::Connection::open(path)?;

        // create tables - this is idempotent so we can safely call it repeatedly
        conn.execute_batch(
            r##"
            CREATE TABLE IF NOT EXISTS scaffolds (
                id INTEGER PRIMARY KEY,
                description TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL
            )
        "##,
        )
        .context("failed to create scaffold table")?;

        Ok(Self { conn })
    }

    fn detect_project_slug() -> String {
        let git_slug = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| {
                Path::new(s.trim())
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(ToString::to_string)
            });

        git_slug
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|p| p.file_name()?.to_str().map(|n| n.to_string()))
            })
            .unwrap_or_else(|| "default".to_string())
    }

    // save - this creates a new scaffold record
    pub fn save(&self, description: &str, content: &str) -> Result<i64> {
        let now = Utc::now();
        self.conn
            .execute(
                r##"
                INSERT INTO scaffolds (description, content, created_at)
                VALUES (?1, ?2, ?3)
            "##,
                params![description, content, now],
            )
            .context("failed to save scaffold")?;

        Ok(self.conn.last_insert_rowid())
    }

    // search - this searches for scaffolds that match the query
    pub fn search(&self, query: &str) -> Result<Vec<ScaffoldRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                r##"
           SELECT id, description, content, created_at
           FROM scaffolds
           WHERE description LIKE ?1
           ORDER BY created_at DESC
           LIMIT 10
            "##,
            )
            .context("failed to prepare search query")?;

        let rows = stmt
            .query_map(params![format!("%{}%", query)], |row| {
                Ok(ScaffoldRecord {
                    id: row.get(0)?,
                    description: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .context("failed to execute search query")?;

        rows.collect::<rusqlite::Result<_>>()
            .context("failed to collect search results")
    }

    // get - this gets a single scaffold by id
    pub fn get(&self, id: i64) -> Result<Option<ScaffoldRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                r##"
            SELECT id, description, content, created_at
            FROM scaffolds
            WHERE id = ?1
        "##,
            )
            .context("failed to prepare get query")?;

        match stmt.query_row(params![id], |row| {
            Ok(ScaffoldRecord {
                id: row.get(0)?,
                description: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
            })
        }) {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get scaffold"),
        }
    }

    // list_recent - this lists the most recent scaffolds
    pub fn list_recent(&self, limit: i64) -> Result<Vec<ScaffoldRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                r##"
                SELECT id, description, content, created_at
                FROM scaffolds
                ORDER BY created_at DESC
                LIMIT ?1
                "##,
            )
            .context("failed to prepare list query")?;

        let rows = stmt
            .query_map([limit], |row| {
                Ok(ScaffoldRecord {
                    id: row.get(0)?,
                    description: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .context("failed to execute list query")?;

        rows.collect::<rusqlite::Result<_>>()
            .context("failed to collect list results")
    }
}
