use std::fs;

use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::tempdir;

// Regression guard: greentic-pack-built gtpack artifacts should be accepted by greentic-dev inspect.
#[test]
fn greentic_pack_gtpack_is_inspectable() -> Result<()> {
    let temp = tempdir()?;
    let pack_dir = temp.path().join("demo-pack");
    let gtpack_path = pack_dir.join("pack.gtpack");

    let new_status = std::process::Command::new("greentic-pack")
        .args(["new", "--dir", pack_dir.to_str().unwrap(), "demo.test"])
        .status()
        .expect("failed to spawn greentic-pack new");
    assert!(new_status.success(), "greentic-pack new failed");

    let build_status = std::process::Command::new("greentic-pack")
        .args([
            "build",
            "--in",
            pack_dir.to_str().unwrap(),
            "--gtpack-out",
            gtpack_path.to_str().unwrap(),
            "--offline",
            "--allow-oci-tags",
        ])
        .status()
        .expect("failed to spawn greentic-pack build");
    assert!(build_status.success(), "greentic-pack build failed");

    assert!(fs::metadata(&gtpack_path).is_ok(), "gtpack not written");

    cargo_bin_cmd!("greentic-dev")
        .arg("pack")
        .arg("inspect")
        .arg(&gtpack_path)
        .assert()
        .success();

    Ok(())
}
