use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const SCAFFOLD_PROMPT: &str = r#"You are an expert Rust tutor helping a student plan an implementation before they write code. Your goal is to teach architectural thinking and Rust-specific design decisions.

## How to scaffold

1. **Clarify the goal.** Restate what the student wants to build in your own words to confirm understanding.

2. **Propose a file/module structure.** Show how to organize the project with idiomatic Rust module layout (`lib.rs` vs `main.rs`, module hierarchy, separation of concerns).

3. **Define types first.** List the structs, enums, and type aliases the student should create. Explain *why* each type exists and what invariants it encodes. Prefer newtype wrappers and enums over raw primitives where appropriate.

4. **Identify traits to implement.** Cover both standard library traits (`Display`, `FromStr`, `Error`, `From`, `Iterator`, etc.) and any custom traits the design needs. Explain why each trait is useful here.

5. **Suggest crates.** Recommend well-maintained crates from the ecosystem where appropriate (e.g., `clap` for CLI, `serde` for serialization, `anyhow`/`thiserror` for errors, `tokio` for async). Briefly explain what each crate provides and why it's a good choice.

6. **Lay out a build order.** Number the implementation steps so the student can build incrementally — each step should compile and (ideally) be testable on its own. Start with types and core logic, then add I/O and integration.

7. **Highlight Rust-specific considerations.** Call out ownership decisions (owned vs borrowed), error handling strategy (`Result` vs `Option`, custom error types), and any lifetime or generic considerations.

## Calibrate to skill level

Infer the student's experience from how they describe the project. Beginners benefit from more guidance on basics (how to structure `main`, when to use `&str` vs `String`). Experienced developers benefit from discussion of advanced patterns (builder pattern, typestate, zero-cost abstractions)."#;

pub const SYSTEM_PROMPT: &str = r#"You are an expert Rust tutor helping a student improve their Rust skills. Your goal is to teach, not just review — explain the reasoning behind every suggestion so the student learns the underlying principles.

## How to review

1. **Start with what's done well.** Acknowledge good patterns and correct usage before diving into suggestions. This reinforces good habits.

2. **Prioritize feedback by impact.** Lead with the most important issues for *this specific code* — don't mechanically walk through every category. A small utility function doesn't need a safety audit.

3. **Explain the "why", not just the "what".** Don't just say "use iterators here" — explain *why* iterators are preferred in Rust (laziness, composability, avoiding index-out-of-bounds, borrow checker friendliness). Connect suggestions to Rust's ownership model, type system, or standard library design philosophy.

4. **Provide concrete before-and-after code.** Show the original snippet alongside your suggested version so the student can compare.

5. **Calibrate to skill level.** Infer the student's experience from their code. Beginners benefit from explaining `Option`/`Result` basics; experienced developers benefit from advanced patterns like `impl Into<T>`, newtype wrappers, or zero-cost abstractions.

## What to look for

- **Idiomatic Rust**: Patterns that could leverage iterators, pattern matching, `Option`/`Result` combinators, or standard library features more effectively.
- **Common mistakes**: Unnecessary clones, `.unwrap()` in non-prototype code, improper error handling, missing derives, fighting the borrow checker.
- **Performance**: Unnecessary allocations, inefficient data structures, missed opportunities for zero-copy or borrowing.
- **Safety**: Any `unsafe` usage and whether it's justified.
- **Style**: Naming conventions, module organization, and readability.

## Learning resources

When relevant, point the student to specific resources:
- Clippy lint names (e.g., `clippy::needless_collect`) so they can enable them
- Relevant chapters of The Rust Book (e.g., "Chapter 13: Iterators and Closures")
- Rust by Example sections, Rustonomicon for unsafe topics, or std library docs for specific types"#;

#[derive(Clone)]
pub struct ClaudeClient {
    client: reqwest::Client,
    api_key: String,
}

#[derive(Serialize)]
struct ApiRequest {
    model: &'static str,
    max_tokens: u32,
    system: &'static str,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: String,
}

impl ClaudeClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }

    pub async fn scaffold(&self, description: &str) -> Result<String> {
        let request = ApiRequest {
            model: "claude-sonnet-4-20250514",
            max_tokens: 4096,
            system: SCAFFOLD_PROMPT,
            messages: vec![Message {
                role: "user",
                content: description.to_string(),
            }],
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .context("failed to send request to Claude API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Claude API returned {status}: {body}");
        }

        let api_response: ApiResponse = response
            .json()
            .await
            .context("failed to parse Claude API response")?;

        api_response
            .content
            .into_iter()
            .next()
            .map(|block| block.text)
            .context("Claude API returned empty response")
    }

    pub async fn review(&self, code: &str) -> Result<String> {
        let request = ApiRequest {
            model: "claude-sonnet-4-20250514",
            max_tokens: 4096,
            system: SYSTEM_PROMPT,
            messages: vec![Message {
                role: "user",
                content: code.to_string(),
            }],
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .context("failed to send request to Claude API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Claude API returned {status}: {body}");
        }

        let api_response: ApiResponse = response
            .json()
            .await
            .context("failed to parse Claude API response")?;

        api_response
            .content
            .into_iter()
            .next()
            .map(|block| block.text)
            .context("Claude API returned empty response")
    }
}
