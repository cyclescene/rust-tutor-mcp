use std::collections::HashMap;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use scraper::Html;
use scraper::Selector;

// fetch_docs - used to get type information for dependencies
pub async fn fetch_docs(
    client: &reqwest::Client,
    crate_name: &str,
    type_name: &str,
    version: &str,
) -> Result<HashMap<String, String>, Error> {
    let web_name = crate_name.replace('-', "_");

    let base = format!("https://docs.rs/{}/{}/{}", crate_name, version, web_name);

    let url = format!("{}/all.html", base);

    let docs_html = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to get docs for {}", crate_name))?
        .text()
        .await
        .with_context(|| format!("Failed to get html for {}", crate_name))?;

    Ok(Html::parse_document(&docs_html)
        .select(&Selector::parse("ul.all-items a").unwrap())
        .filter_map(|item| {
            let name = item.inner_html();
            if name.to_lowercase().contains(&type_name.to_lowercase()) {
                let href = format!("{}/{}", base, item.attr("href")?.to_string());
                Some((name, href))
            } else {
                None
            }
        })
        .collect())
}
