#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetChangesByChangeIdParams {
    pub change_id: String,
}
