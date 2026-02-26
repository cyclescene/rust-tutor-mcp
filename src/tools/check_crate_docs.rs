#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CheckCrateDocsParams {
    // Which crate to check for docs
    pub crate_name: String,
    // which type to look up
    pub type_name: String,
    pub version: Option<String>,
}
