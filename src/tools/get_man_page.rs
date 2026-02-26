#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetManPageParams {
    pub command: String,
}
