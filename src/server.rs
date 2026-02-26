use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};

use crate::{
    claude::{ClaudeClient, SCAFFOLD_PROMPT, SYSTEM_PROMPT},
    docs_rs::fetch_docs,
    man,
    store::{FileChangeRecord, SaveEventSummary, ScaffoldRecord, TutorStore},
    tools::{
        CheckCrateDocsParams, GetChangesByChangeIdParams, GetFileChangesParams, GetManPageParams,
        GetScaffoldParams, ListRecentChangesParams, ListScaffoldsParams, ReviewFileParams,
        SaveScaffoldParams, ScaffoldParams,
    },
    watcher::FileWatcher,
};

#[derive(Clone)]
pub struct RustTutor {
    tool_router: ToolRouter<Self>,
    store: Arc<Mutex<TutorStore>>,
    claude: Option<ClaudeClient>,
    client: reqwest::Client,
}

const DEFAULT_LIST_LIMIT: i64 = 5;

#[tool_router]
impl RustTutor {
    pub fn new(claude: Option<ClaudeClient>) -> anyhow::Result<Self> {
        let store = Arc::new(Mutex::new(TutorStore::open()?));

        FileWatcher::spawn(Arc::clone(&store));
        Ok(Self {
            tool_router: Self::tool_router(),
            store,
            claude,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("Rust Tutor MCP")
                .build()?,
        })
    }

    #[tool(
        name = "review_file",
        description = "Review a Rust source file for idiomatic patterns and common mistakes",
        annotations(title = "Review File")
    )]
    async fn review_file(
        &self,
        Parameters(params): Parameters<ReviewFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let contents = tokio::fs::read_to_string(&params.file_path)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to read file: {e}"), None))?;

        match &self.claude {
            Some(client) => {
                let review = client.review(&contents).await.map_err(|e| {
                    McpError::internal_error(format!("Claude API error: {e}"), None)
                })?;
                Ok(CallToolResult::success(vec![Content::text(review)]))
            }
            None => {
                // No API key — return file contents with review instructions
                // so the host LLM (e.g. Claude Code) performs the review itself.
                let response = format!(
                    "{SYSTEM_PROMPT}\n\n---\n\n**File: `{}`**\n\n```rust\n{contents}\n```",
                    params.file_path
                );
                Ok(CallToolResult::success(vec![Content::text(response)]))
            }
        }
    }

    #[tool(
        name = "scaffold",
        description = "Given a description of what you want to build in Rust, returns a step-by-step implementation plan with types, traits, crates, and build order",
        annotations(title = "Scaffold")
    )]
    async fn scaffold(
        &self,
        Parameters(params): Parameters<ScaffoldParams>,
    ) -> Result<CallToolResult, McpError> {
        match &self.claude {
            Some(client) => {
                let mut plan = client.scaffold(&params.description).await.map_err(|e| {
                    McpError::internal_error(format!("Claude API error: {e}"), None)
                })?;

                let id = self
                    .store
                    .lock()
                    .expect("store lock poisoned")
                    .save_scaffold(&params.description, &plan)
                    .map_err(|e| {
                        McpError::internal_error(format!("Failed to save scaffold: {e}"), None)
                    })?;

                plan.push_str(&format!("\n\n**ID**: {id}"));

                Ok(CallToolResult::success(vec![Content::text(plan)]))
            }
            None => {
                let response = format!(
                    "{SCAFFOLD_PROMPT}\n\n---\n\n**Project description:**\n\n{}",
                    params.description
                );
                Ok(CallToolResult::success(vec![Content::text(response)]))
            }
        }
    }

    #[tool(
        name = "save_scaffold",
        description = "Save a scaffold",
        annotations(title = "Save Scaffold")
    )]
    async fn save_scaffold(
        &self,
        Parameters(params): Parameters<SaveScaffoldParams>,
    ) -> Result<CallToolResult, McpError> {
        let id = {
            let store = self.store.lock().expect("store lock poisoned");
            store.save_scaffold(&params.description, &params.content)
        }
        .map_err(|e| McpError::internal_error(format!("Failed to save scaffold: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Saved scaffold with ID {id}"
        ))]))
    }

    #[tool(
        name = "list_scaffolds",
        description = "List scaffolds, if no query then list the most recent",
        annotations(title = "List Scaffolds")
    )]
    async fn list_scaffolds(
        &self,
        Parameters(params): Parameters<ListScaffoldsParams>,
    ) -> Result<CallToolResult, McpError> {
        let records = {
            let store = self.store.lock().expect("store lock poisoned");
            match params.query {
                Some(q) => store.search_scaffolds(&q),
                None => store.list_recent_scaffolds(params.limit.unwrap_or(DEFAULT_LIST_LIMIT)),
            }
            .map_err(|e| McpError::internal_error(format!("Failed to list scaffolds: {e}"), None))?
        };

        let text = join_or_empty(
            &records,
            "No scaffolds found",
            ScaffoldRecord::format_changes,
        );

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        name = "get_scaffold",
        description = "Get a scaffold by ID",
        annotations(title = "Get Scaffold")
    )]
    async fn get_scaffold(
        &self,
        Parameters(params): Parameters<GetScaffoldParams>,
    ) -> Result<CallToolResult, McpError> {
        let record = {
            let store = self.store.lock().expect("store lock poisoned");

            store.get_scaffold_by_id(params.id).map_err(|e| {
                McpError::internal_error(format!("Failed to get scaffold: {e}"), None)
            })?
        };

        let text = match record {
            Some(r) => format!(
                "**ID {}** ({}): {}\n{}",
                r.id, r.created_at, r.description, r.content
            ),
            None => "No scaffold found".to_string(),
        };

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        name = "get_file_changes",
        description = "Get a list of recent file changes",
        annotations(title = "Get File Changes")
    )]
    async fn get_file_changes(
        &self,
        Parameters(params): Parameters<GetFileChangesParams>,
    ) -> Result<CallToolResult, McpError> {
        let changes = {
            let store = self.store.lock().expect("store lock poisoned");
            store
                .get_changes_for_file(
                    &params.file_path,
                    params.limit.unwrap_or(DEFAULT_LIST_LIMIT),
                )
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to get file changes: {e}"), None)
                })?
        };

        let text = join_or_empty(
            &changes,
            "No file changes found",
            FileChangeRecord::format_changes,
        );

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        name = "list_recent_change_ids",
        description = "List recent file changes, if no query then list the most recent",
        annotations(title = "List Recent Change IDs")
    )]
    async fn list_recent_changes(
        &self,
        Parameters(params): Parameters<ListRecentChangesParams>,
    ) -> Result<CallToolResult, McpError> {
        let change_ids = {
            let store = self.store.lock().expect("store lock poisoned");
            store
                .list_recent_change_ids(params.limit.unwrap_or(DEFAULT_LIST_LIMIT))
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to list file changes: {e}"), None)
                })?
        };

        let text = join_or_empty(
            &change_ids,
            "No file changes found",
            SaveEventSummary::format_summary,
        );

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        name = "get_changes_by_change_id",
        description = "Get all file changes for a given change ID",
        annotations(title = "Get Changes By Change Id")
    )]
    async fn get_changes_by_change_id(
        &self,
        Parameters(params): Parameters<GetChangesByChangeIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let changes = {
            let store = self.store.lock().expect("store lock poisoned");
            store
                .get_changes_for_change_id(&params.change_id)
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to get file changes: {e}"), None)
                })?
        };

        let text = join_or_empty(
            &changes,
            "No file changes found",
            FileChangeRecord::format_changes,
        );

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        name = "check_crate_docs",
        description = "check docs.rs for information on types",
        annotations(title = "Check Crate Docs", read_only_hint = true,)
    )]
    async fn check_crate_docs(
        &self,
        Parameters(params): Parameters<CheckCrateDocsParams>,
    ) -> Result<CallToolResult, McpError> {
        // check the version fallback to latest

        let version = params.version.unwrap_or_else(|| String::from("latest"));

        let results = fetch_docs(
            &self.client,
            &params.crate_name,
            &params.type_name,
            &version,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                "failed to get docs: reason - {} crate_name - {} type_name - {} version -{}",
                e,
                &params.crate_name,
                &params.type_name,
                version
            );
            McpError::internal_error(format!("failed to fetch docs: {e}"), None)
        })?;

        let text = if results.is_empty() {
            "No Results found".to_string()
        } else {
            format!(
                "**Results**:\n\n{}",
                results
                    .iter()
                    .map(|r| format!("{:?}\n\n", r))
                    .collect::<Vec<_>>()
                    .join("\n---\n")
            )
        };

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        name = "get_man_page",
        description = "get a man page for the inputted command",
        annotations(title = "Get Man Page", read_only_hint = true,)
    )]
    async fn get_man_page(
        &self,
        Parameters(params): Parameters<GetManPageParams>,
    ) -> Result<CallToolResult, McpError> {
        let man_page = man::query_man_page(&params.command).map_err(|e| {
            McpError::internal_error(format!("failed to fetch man page: {e}"), None)
        })?;

        Ok(CallToolResult::success(vec![Content::text(man_page)]))
    }
}

#[tool_handler]
impl ServerHandler for RustTutor {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("A Rust tutor that reviews .rs files for idiomatic patterns, common mistakes, and best practices.".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

fn join_or_empty<T>(items: &[T], msg: &str, f: impl Fn(&T) -> String) -> String {
    if items.is_empty() {
        msg.to_string()
    } else {
        items.iter().map(f).collect::<Vec<_>>().join("\n\n---\n\n")
    }
}
