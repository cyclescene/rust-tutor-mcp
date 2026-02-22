#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListRecentChangesParams {
    pub limit: Option<i64>,
}
