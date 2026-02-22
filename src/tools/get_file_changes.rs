#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetFileChangesParams {
    pub file_path: String,
    pub limit: Option<i64>,
}
