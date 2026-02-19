/// Input parameters for the `scaffold` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SaveScaffoldParams {
    /// Description of the feature or project the student wants to build
    pub description: String,
    /// The scaffold text
    pub content: String,
}
