use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use greentic_dev::config;
use greentic_dev::distributor;
use once_cell::sync::Lazy;
use tempfile::TempDir;

static ENV_GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[test]
fn config_resolution_prefers_xdg_over_greentic_home() -> Result<()> {
    let _guard = ENV_GUARD.lock().unwrap();
    let env = TestEnv::new()?;
    let xdg_path = env.write_xdg_config(
        r#"
[distributor]
default_profile = "default"

[distributor.profiles.default]
base_url = "https://xdg.example"
tenant_id = "xdg-tenant"
environment_id = "xdg-env"
"#,
    );
    let legacy_path = env.write_legacy_config(
        r#"
[distributor]
default_profile = "legacy"

[distributor.profiles.legacy]
base_url = "https://legacy.example"
tenant_id = "legacy-tenant"
environment_id = "legacy-env"
"#,
    );

    let loaded = config::load_with_meta(None)?;
    assert_eq!(
        loaded.loaded_from.as_ref().map(|p| p.as_path()),
        Some(xdg_path.as_path()),
        "should prefer XDG config over legacy ~/.greentic"
    );
    let profile = distributor::resolve_profile(&loaded, None)?;
    assert_eq!(profile.url, "https://xdg.example");
    assert!(loaded.attempted_paths.contains(&legacy_path));
    Ok(())
}

#[test]
fn default_profile_string_resolves_to_profiles_table() -> Result<()> {
    let _guard = ENV_GUARD.lock().unwrap();
    let env = TestEnv::new()?;
    env.write_xdg_config(
        r#"
[distributor]
default_profile = "string-default"

[distributor.profiles.string-default]
base_url = "https://string.example"
tenant_id = "string-tenant"
environment_id = "string-env"
"#,
    );

    let loaded = config::load_with_meta(None)?;
    let profile = distributor::resolve_profile(&loaded, None)?;
    assert_eq!(profile.url, "https://string.example");
    assert_eq!(profile.tenant_id, "string-tenant");
    assert_eq!(profile.environment_id, "string-env");
    Ok(())
}

#[test]
fn default_profile_string_missing_profile_errors_with_available_profiles() -> Result<()> {
    let _guard = ENV_GUARD.lock().unwrap();
    let env = TestEnv::new()?;
    let config_path = env.write_xdg_config(
        r#"
[distributor]
default_profile = "missing"

[distributor.profiles.one]
base_url = "https://one.example"
tenant_id = "t1"
environment_id = "e1"

[distributor.profiles.two]
base_url = "https://two.example"
tenant_id = "t2"
environment_id = "e2"
"#,
    );

    let loaded = config::load_with_meta(None)?;
    let err = distributor::resolve_profile(&loaded, None).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("available: one, two"),
        "message should list available profiles: {msg}"
    );
    assert!(
        msg.contains(&config_path.display().to_string()),
        "message should include loaded config path: {msg}"
    );
    assert!(
        msg.contains("GREENTIC_DEV_CONFIG_FILE"),
        "message should hint at override env var: {msg}"
    );
    Ok(())
}

#[test]
fn default_profile_inline_struct_works() -> Result<()> {
    let _guard = ENV_GUARD.lock().unwrap();
    let env = TestEnv::new()?;
    env.write_xdg_config(
        r#"
[distributor]
default_profile = { name = "inline", base_url = "https://inline.example", tenant_id = "inline-tenant", environment_id = "inline-env" }
"#,
    );

    let loaded = config::load_with_meta(None)?;
    let profile = distributor::resolve_profile(&loaded, None)?;
    assert_eq!(profile.name, "inline");
    assert_eq!(profile.url, "https://inline.example");
    assert_eq!(profile.tenant_id, "inline-tenant");
    assert_eq!(profile.environment_id, "inline-env");
    Ok(())
}

#[test]
fn error_message_mentions_actual_loaded_path() -> Result<()> {
    let _guard = ENV_GUARD.lock().unwrap();
    let env = TestEnv::new()?;
    let xdg_path = env.write_xdg_config(
        r#"
[distributor]
default_profile = "default"

[distributor.profiles.default]
base_url = "https://valid.example"
tenant_id = "valid-tenant"
environment_id = "valid-env"
"#,
    );
    let broken = env.home.join(".greentic/config.toml");
    fs::create_dir_all(broken.parent().unwrap()).unwrap();
    fs::write(&broken, "this is not toml").unwrap();

    let loaded = config::load_with_meta(None)?;
    assert_eq!(
        loaded.loaded_from.as_ref().map(|p| p.as_path()),
        Some(xdg_path.as_path()),
        "loader should ignore lower-precedence invalid configs"
    );
    let profile = distributor::resolve_profile(&loaded, None)?;
    assert_eq!(profile.url, "https://valid.example");
    assert!(
        loaded.attempted_paths.contains(&broken),
        "attempted paths should list legacy path even if not selected"
    );
    Ok(())
}

struct TestEnv {
    _temp: TempDir,
    home: PathBuf,
    xdg: PathBuf,
}

impl TestEnv {
    fn new() -> Result<Self> {
        let temp = TempDir::new()?;
        let home = temp.path().join("home");
        let xdg = temp.path().join(".config");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&xdg)?;
        set_base_env(&home, &xdg);
        Ok(Self {
            _temp: temp,
            home,
            xdg,
        })
    }

    fn write_xdg_config(&self, contents: &str) -> PathBuf {
        let path = self.xdg.join("greentic-dev").join("config.toml");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents.as_bytes()).unwrap();
        path
    }

    fn write_legacy_config(&self, contents: &str) -> PathBuf {
        let path = self.home.join(".greentic").join("config.toml");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents.as_bytes()).unwrap();
        path
    }
}

fn set_base_env(home: &Path, xdg: &Path) {
    unsafe {
        std::env::set_var("HOME", home);
        std::env::set_var("XDG_CONFIG_HOME", xdg);
        let base = home.parent().unwrap_or(home);
        std::env::set_var("XDG_DATA_HOME", base.join(".local/share"));
        std::env::set_var("XDG_STATE_HOME", base.join(".local/state"));
        std::env::set_var("XDG_CACHE_HOME", base.join(".cache"));
        std::env::remove_var("GREENTIC_DEV_CONFIG_FILE");
        std::env::remove_var("GREENTIC_CONFIG_FILE");
        std::env::remove_var("GREENTIC_CONFIG");
        std::env::remove_var("GREENTIC_DISTRIBUTOR_PROFILE");
    }
}
