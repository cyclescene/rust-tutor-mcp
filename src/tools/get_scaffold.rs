#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetScaffoldParams {
    pub id: i64,
}
