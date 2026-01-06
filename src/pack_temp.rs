use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

use packc::BuildArgs;
use packc::build::{self, BuildOptions};
use packc::runtime;

/// Ensure we have a concrete .gtpack on disk. If the input is already a file, reuse it.
/// If it is a directory, shell out to packc to build a temporary .gtpack and return its path.
pub fn materialize_pack_path(input: &Path, verbose: bool) -> Result<(Option<TempDir>, PathBuf)> {
    let metadata =
        fs::metadata(input).with_context(|| format!("unable to read input {}", input.display()))?;
    if metadata.is_file() {
        return Ok((None, input.to_path_buf()));
    }
    if metadata.is_dir() {
        let temp = TempDir::new().context("failed to create temporary directory for pack build")?;
        let pack_path = temp.path().join("pack.gtpack");
        build_packc_temp(input, &pack_path, verbose)?;
        return Ok((Some(temp), pack_path));
    }
    bail!(
        "input {} is neither a file nor a directory",
        input.display()
    );
}

fn build_packc_temp(source: &Path, gtpack_out: &Path, verbose: bool) -> Result<()> {
    let _ = verbose;
    // Use packc library to build a temporary gtpack.
    let build_args = BuildArgs {
        input: source.to_path_buf(),
        component_out: Some(gtpack_out.with_extension("wasm")),
        manifest: Some(gtpack_out.with_extension("cbor")),
        sbom: Some(gtpack_out.with_extension("cdx.json")),
        gtpack_out: Some(gtpack_out.to_path_buf()),
        dry_run: false,
        secrets_req: None,
        default_secret_scope: None,
        allow_oci_tags: false,
    };
    let runtime = runtime::resolve_runtime(None, None, true, None)?;
    let opts = BuildOptions::from_args(build_args, &runtime)?;
    build::run(&opts).context("packc build failed for temporary .gtpack")
}
