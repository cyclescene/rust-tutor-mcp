#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListScaffoldsParams {
    pub query: Option<String>,
    pub limit: Option<i64>,
}
