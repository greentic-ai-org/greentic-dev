use std::path::{Path, PathBuf};

use anyhow::Result;

#[cfg(feature = "verify")]
use anyhow::{Context, anyhow};
#[cfg(feature = "verify")]
use greentic_pack::reader::{self, SigningPolicy};
#[cfg(feature = "verify")]
use tokio::{fs, task};

#[cfg(feature = "verify")]
pub async fn verify_pack(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .await
        .with_context(|| format!("pack file {path:?} does not exist"))?;
    if !metadata.is_file() {
        anyhow::bail!("pack path {path:?} is not a file");
    }
    let path = path.to_path_buf();
    task::spawn_blocking(move || verify_pack_blocking(path))
        .await
        .context("pack verification task failed")??;
    Ok(())
}

#[cfg(not(feature = "verify"))]
pub async fn verify_pack(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(feature = "verify")]
fn verify_pack_blocking(path: PathBuf) -> Result<()> {
    let strict = std::env::var("PACK_VERIFY_STRICT")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or_else(|_| std::env::var("PACK_PUBLIC_KEY").is_ok());
    let policy = if strict {
        SigningPolicy::Strict
    } else {
        SigningPolicy::DevOk
    };
    reader::open_pack(&path, policy).map_err(|err| anyhow!(err.message))?;
    Ok(())
}
