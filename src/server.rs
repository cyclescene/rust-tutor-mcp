use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    ErrorData as McpError,
};

use crate::claude::{ClaudeClient, SCAFFOLD_PROMPT, SYSTEM_PROMPT};
use crate::tools::{ReviewFileParams, ScaffoldParams};

#[derive(Clone)]
pub struct RustTutor {
    tool_router: ToolRouter<Self>,
    claude: Option<ClaudeClient>,
}

#[tool_router]
impl RustTutor {
    pub fn new(claude: Option<ClaudeClient>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            claude,
        }
    }

    #[tool(
        name = "review_file",
        description = "Review a Rust source file for idiomatic patterns and common mistakes"
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
                let review = client
                    .review(&contents)
                    .await
                    .map_err(|e| {
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
        description = "Given a description of what you want to build in Rust, returns a step-by-step implementation plan with types, traits, crates, and build order"
    )]
    async fn scaffold(
        &self,
        Parameters(params): Parameters<ScaffoldParams>,
    ) -> Result<CallToolResult, McpError> {
        match &self.claude {
            Some(client) => {
                let plan = client
                    .scaffold(&params.description)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("Claude API error: {e}"), None)
                    })?;
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
