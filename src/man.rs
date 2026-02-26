use std::process::Command;

use anyhow::{anyhow, Result};

pub fn query_man_page(command: &str) -> Result<String> {
    Command::new("man")
        .arg(command)
        .env("MANPAGER", "cat")
        .output()
        .map_err(|_| anyhow!("man is not installed"))
        .and_then(|o| {
            if o.status.success() {
                Ok(o)
            } else {
                Err(anyhow!("no content found for: {}", command))
            }
        })
        .and_then(|o| String::from_utf8(o.stdout).map_err(|e| anyhow!(e)))
}
