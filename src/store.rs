use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::{
    fs,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
};

trait FromRow: Sized {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self>;
}

#[derive(Debug, Clone)]
pub struct ScaffoldRecord {
    pub id: i64,
    pub description: String, // original user prompt
    pub content: String,     // full scaffold text
    pub created_at: DateTime<Utc>,
}

impl ScaffoldRecord {
    pub fn format_changes(&self) -> String {
        format!(
            "**ID {}**: {}\n{}",
            &self.id, &self.description, &self.content
        )
    }
}

impl FromRow for ScaffoldRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            description: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FileChangeRecord {
    pub id: i64,
    pub file_path: String,
    pub hunk_idx: i64,     // hunk position in the file
    pub change_id: String, // UUID or timestamp-based grouping per save event
    pub old_start: i64,
    pub old_count: i64,
    pub new_start: i64,
    pub new_count: i64,
    pub before_lines: String,
    pub after_lines: String,
    pub changed_at: DateTime<Utc>,
}

impl FromRow for FileChangeRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            file_path: row.get(1)?,
            hunk_idx: row.get(2)?,
            change_id: row.get(3)?,
            old_start: row.get(4)?,
            old_count: row.get(5)?,
            new_start: row.get(6)?,
            new_count: row.get(7)?,
            before_lines: row.get(8)?,
            after_lines: row.get(9)?,
            changed_at: row.get(10)?,
        })
    }
}

impl FileChangeRecord {
    pub fn format_changes(&self) -> String {
        format!(
            "**ID {}** `{}` ({}):\n\n@@ -{},{} +{},{}
            @@\n\nBefore:\n```\n{}\n```\n\nAfter:\n```\n{}\n```",
            self.id,
            self.file_path,
            self.changed_at,
            self.old_start,
            self.old_count,
            self.new_start,
            self.new_count,
            self.before_lines,
            self.after_lines
        )
    }
}

#[derive(Debug, Clone)]
pub struct SaveEventSummary {
    pub change_id: String,
    pub file_path: String,
    pub changed_at: DateTime<Utc>,
    pub hunk_count: i64,
}

impl FromRow for SaveEventSummary {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            change_id: row.get(0)?,
            file_path: row.get(1)?,
            changed_at: row.get(2)?,
            hunk_count: row.get(3)?,
        })
    }
}

impl SaveEventSummary {
    pub fn format_summary(&self) -> String {
        format!(
            "**ID {}** `{}` ({}):\n\n{} hunk{}",
            self.change_id,
            self.file_path,
            self.changed_at,
            self.hunk_count,
            if self.hunk_count == 1 { "" } else { "s" }
        )
    }
}

#[derive(Debug)]
pub struct TutorStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl TutorStore {
    // open - this opens the database at the default location
    pub fn open() -> Result<Self> {
        let path = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("could not resolve data dir"))?
            .join("rust-tutor-mcp")
            .join(Self::detect_project_slug())
            .join("tutor.db");
        // log to show where the tuttor db files live on the host
        tracing::debug!(path = %path.display(), "tutor db location");
        Self::open_at(&path)
    }

    // open_at - this opens a database at a specific path and creates the tables if they don't exist
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

        conn.execute_batch(
            r##"
            CREATE TABLE IF NOT EXISTS file_changes (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                hunk_idx INTEGER NOT NULL,
                change_id TEXT NOT NULL,
                old_start INTEGER NOT NULL,
                old_count INTEGER NOT NULL,
                new_start INTEGER NOT NULL,
                new_count INTEGER NOT NULL,
                before_lines TEXT NOT NULL,
                after_lines TEXT NOT NULL,
                changed_at TEXT NOT NULL

            ) 
        "##,
        )
        .context("failed to create file_changes table")?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // detect_project_slug - this detects the project slug from the current directory
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
                    .and_then(|p| p.file_name()?.to_str().map(ToString::to_string))
            })
            .unwrap_or_else(|| "default".to_string())
    }

    // ROW HELPERS

    fn collect_rows<T: FromRow>(
        stmt: &mut rusqlite::Statement<'_>,
        params: impl rusqlite::Params,
    ) -> Result<Vec<T>> {
        stmt.query_map(params, T::from_row)?
            .collect::<rusqlite::Result<_>>()
            .context("failed to collect results")
    }

    // SCAFFOLDS

    // save - this creates a new scaffold record
    pub fn save_scaffold(&self, description: &str, content: &str) -> Result<i64> {
        let conn = self.conn.lock().expect("store lock poisoned");

        conn.execute(
            r##"
                INSERT INTO scaffolds (description, content, created_at)
                VALUES (?1, ?2, ?3)
            "##,
            params![description, content, Utc::now()],
        )
        .context("failed to save scaffold")?;

        Ok(conn.last_insert_rowid())
    }
    // search - this searches for scaffolds that match the query
    pub fn search_scaffolds(&self, query: &str) -> Result<Vec<ScaffoldRecord>> {
        let conn = self.conn.lock().expect("store lock poisoned");
        let mut stmt = conn
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

        Self::collect_rows(&mut stmt, params![format!("%{}%", query)])
            .context("failed to collect search results")
    }

    // get - this gets a single scaffold by id
    pub fn get_scaffold_by_id(&self, id: i64) -> Result<Option<ScaffoldRecord>> {
        let conn = self.conn.lock().expect("store lock poisoned");
        let mut stmt = conn
            .prepare(
                r##"
            SELECT id, description, content, created_at
            FROM scaffolds
            WHERE id = ?1
        "##,
            )
            .context("failed to prepare get query")?;

        match stmt.query_row(params![id], ScaffoldRecord::from_row) {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to return scaffold"),
        }
    }

    // list_recent - this lists the most recent scaffolds
    pub fn list_recent_scaffolds(&self, limit: i64) -> Result<Vec<ScaffoldRecord>> {
        let conn = self.conn.lock().expect("store lock poisoned");

        let mut stmt = conn
            .prepare(
                r##"
                SELECT id, description, content, created_at
                FROM scaffolds
                ORDER BY created_at DESC
                LIMIT ?1
                "##,
            )
            .context("failed to prepare list query")?;

        Self::collect_rows(&mut stmt, [limit])
    }

    // FILE CHANGES

    // save_file_change - this creates a new file change record
    pub fn save_file_change(&self, file_change: &FileChangeRecord) -> Result<i64> {
        let conn = self.conn.lock().expect("store lock poisoned");

        conn
            .execute(r##"
            INSERT INTO file_changes (file_path, hunk_idx, change_id, old_start, old_count, new_start, new_count, before_lines, after_lines, changed_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "##,
            params![
                file_change.file_path,
                file_change.hunk_idx,
                file_change.change_id,
                file_change.old_start,
                file_change.old_count,
                file_change.new_start,
                file_change.new_count,
                file_change.before_lines,
                file_change.after_lines,
                file_change.changed_at,
            ])
            .context("failed to save file change")?;

        Ok(conn.last_insert_rowid())
    }

    pub fn get_changes_for_file(
        &self,
        file_path: &str,
        limit: i64,
    ) -> Result<Vec<FileChangeRecord>> {
        let conn = self.conn.lock().expect("store lock poisoned");
        let mut stmt = conn
            .prepare(
                r##"
            SELECT id, file_path, hunk_idx, change_id, old_start, old_count, new_start, new_count, before_lines, after_lines, changed_at
            FROM file_changes
            WHERE file_path = ?1
            ORDER BY changed_at DESC
            LIMIT ?2
            "##,
            )
            .context("failed to prepare get query")?;

        Self::collect_rows(&mut stmt, params![file_path, limit])
            .context("failed to collect get results")
    }

    pub fn list_recent_change_ids(&self, limit: i64) -> Result<Vec<SaveEventSummary>> {
        let conn = self.conn.lock().expect("store lock poisoned");

        let mut stmt = conn
            .prepare(
                r##"
                    SELECT change_id, file_path, changed_at, COUNT(*) as hunk_count
                    FROM file_changes
                    GROUP BY change_id
                    ORDER BY changed_at DESC
                    LIMIT ?1
                "##,
            )
            .context("failed to prepare list query")?;

        Self::collect_rows(&mut stmt, [limit]).context("failed to collect list results")
    }

    pub fn get_changes_for_change_id(&self, change_id: &str) -> Result<Vec<FileChangeRecord>> {
        let conn = self.conn.lock().expect("store lock poisoned");
        let mut stmt = conn
            .prepare(
               r##"
               SELECT id, file_path, hunk_idx, change_id, old_start, old_count, new_start, new_count, before_lines, after_lines, changed_at
               FROM file_changes
               WHERE change_id = ?1
               ORDER BY changed_at DESC
               "## 
                ).context("failed to prepare get query")?;

        Self::collect_rows(&mut stmt, params![change_id]).context("failed to collect get results")
    }
}
