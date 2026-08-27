//! Regenerates the canonical OpenAPI YAML artifact (docs/contracts/API_CONTRACT.md).
//!
//! Usage: `openapi-dump [out.yaml]` (default: stdout).
//! CI asserts the committed artifact matches (no accidental contract drift).

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1);
    let yaml = cicd::api::openapi_yaml()?;
    match out {
        Some(path) => std::fs::write(path, yaml)?,
        None => print!("{yaml}"),
    }
    Ok(())
}
