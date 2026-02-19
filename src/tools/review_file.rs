/// Input parameters for the `review_file` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ReviewFileParams {
    /// Path to the Rust source file to review
    pub file_path: String,
}
