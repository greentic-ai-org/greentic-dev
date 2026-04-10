use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const FLOW_HELP: &str =
    "FLOW_HELP new update add-step update-step delete-step doctor bind-component";
const COMPONENT_HELP: &str =
    "COMPONENT_HELP add new templates doctor inspect hash build flow store";
const PACK_HELP: &str = "PACK_HELP build lint components update new sign verify gui inspect doctor plan events config run init new-provider";
const RUNNER_HELP: &str = "RUNNER_HELP --pack --entry --input --json";
const GUI_HELP: &str = "GUI_HELP serve pack-dev";
struct StubBins {
    _dir: TempDir,
    flow: PathBuf,
    component: PathBuf,
    pack: PathBuf,
    runner: PathBuf,
    gui: PathBuf,
    mcp: PathBuf,
}

fn write_stub(dir: &Path, name: &str, output: &str) -> PathBuf {
    #[cfg(windows)]
    let path = dir.join(format!("{name}.cmd"));
    #[cfg(not(windows))]
    let path = dir.join(name);

    #[cfg(windows)]
    let script = format!("@echo {output}\r\n");
    #[cfg(not(windows))]
    let script = format!("#!/bin/sh\necho \"{}\"\n", output.replace('"', "\\\""));

    fs::write(&path, script).unwrap();

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }

    path
}

fn write_mcp_stub(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    let path = dir.join("greentic-mcp.cmd");
    #[cfg(not(windows))]
    let path = dir.join("greentic-mcp");

    #[cfg(windows)]
    let script = "@echo mcp:%1:%2:%3:%4\r\n";
    #[cfg(not(windows))]
    let script = "#!/bin/sh\necho \"mcp:$1:$2:$3:$4\"\n";

    fs::write(&path, script).unwrap();

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }

    path
}

fn build_stubs() -> StubBins {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    StubBins {
        _dir: dir,
        flow: write_stub(&root, "greentic-flow", FLOW_HELP),
        component: write_stub(&root, "greentic-component", COMPONENT_HELP),
        pack: write_stub(&root, "greentic-pack", PACK_HELP),
        runner: write_stub(&root, "greentic-runner-cli", RUNNER_HELP),
        gui: write_stub(&root, "greentic-gui", GUI_HELP),
        mcp: write_mcp_stub(&root),
    }
}

fn assert_passthrough_help(args: &[&str], expected: &[&str]) {
    let stubs = build_stubs();
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.env("GREENTIC_DEV_BIN_GREENTIC_FLOW", &stubs.flow)
        .env("GREENTIC_DEV_BIN_GREENTIC_COMPONENT", &stubs.component)
        .env("GREENTIC_DEV_BIN_GREENTIC_PACK", &stubs.pack)
        .env("GREENTIC_DEV_BIN_GREENTIC_RUNNER_CLI", &stubs.runner)
        .env("GREENTIC_DEV_BIN_GREENTIC_GUI", &stubs.gui)
        .env("GREENTIC_DEV_BIN_GREENTIC_MCP", &stubs.mcp);

    let mut assert = cmd.args(args).assert().success();
    for item in expected {
        assert = assert.stdout(contains(*item));
    }
}

#[test]
fn flow_help_passthrough() {
    assert_passthrough_help(
        &["flow", "--help"],
        &[
            "FLOW_HELP",
            "new",
            "update",
            "add-step",
            "update-step",
            "delete-step",
            "doctor",
            "bind-component",
        ],
    );
}

#[test]
fn component_help_passthrough() {
    assert_passthrough_help(
        &["component", "--help"],
        &[
            "COMPONENT_HELP",
            "add",
            "new",
            "templates",
            "doctor",
            "inspect",
            "hash",
            "build",
            "flow",
            "store",
        ],
    );
}

#[test]
fn pack_help_passthrough() {
    assert_passthrough_help(
        &["pack", "--help"],
        &[
            "PACK_HELP",
            "build",
            "lint",
            "components",
            "update",
            "new",
            "sign",
            "verify",
            "gui",
            "inspect",
            "doctor",
            "plan",
            "events",
            "config",
            "run",
            "init",
            "new-provider",
        ],
    );
}

#[test]
fn gui_help_passthrough() {
    assert_passthrough_help(&["gui", "--help"], &["GUI_HELP", "serve", "pack-dev"]);
}

#[test]
fn runner_help_passthrough_via_pack_run() {
    assert_passthrough_help(
        &["pack", "run", "--help"],
        &["RUNNER_HELP", "--pack", "--entry"],
    );
}

#[test]
fn mcp_compose_passthrough() {
    assert_passthrough_help(
        &["mcp", "--compose", "./weatherapi.wasm", "-o", "weatherapi.component.wasm"],
        &["mcp:compose:./weatherapi.wasm:-o:weatherapi.component.wasm"],
    );
}
