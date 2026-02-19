/// Input parameters for the `scaffold` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScaffoldParams {
    /// Description of the feature or project the student wants to build
    pub description: String,
}
