//! Bundle lifecycle management commands.
//!
//! Provides CLI commands for managing Greentic bundles:
//! - `bundle add` - Add a pack to a bundle
//! - `bundle setup` - Run setup flow for a provider
//! - `bundle update` - Update a provider's configuration
//! - `bundle remove` - Remove a provider from a bundle
//! - `bundle status` - Show bundle status

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::cli::{BundleAddArgs, BundleRemoveArgs, BundleSetupArgs, BundleStatusArgs};

/// Run the `bundle add` command.
pub fn add(args: BundleAddArgs) -> Result<()> {
    let bundle_dir = resolve_bundle_dir(args.bundle)?;

    println!("Adding pack to bundle...");
    println!("  Pack ref: {}", args.pack_ref);
    println!("  Bundle: {}", bundle_dir.display());
    println!("  Tenant: {}", args.tenant);
    println!("  Team: {}", args.team.as_deref().unwrap_or("default"));
    println!("  Env: {}", args.env);

    if args.dry_run {
        println!("\n[dry-run] Would add pack to bundle");
        return Ok(());
    }

    // Ensure bundle directory exists
    if !bundle_dir.exists() {
        std::fs::create_dir_all(&bundle_dir)
            .context("failed to create bundle directory")?;
    }

    // Create providers directory if needed
    let providers_dir = bundle_dir.join("providers");
    if !providers_dir.exists() {
        std::fs::create_dir_all(&providers_dir)
            .context("failed to create providers directory")?;
    }

    // Determine pack type and destination
    let pack_path = PathBuf::from(&args.pack_ref);
    if pack_path.exists() {
        // Local pack file - copy to providers directory
        let pack_name = pack_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.gtpack");

        // Determine domain from pack name (messaging-*, events-*, etc.)
        let domain = if pack_name.starts_with("messaging-") {
            "messaging"
        } else if pack_name.starts_with("events-") {
            "events"
        } else if pack_name.starts_with("oauth-") {
            "oauth"
        } else if pack_name.starts_with("secrets-") {
            "secrets"
        } else {
            "other"
        };

        let domain_dir = providers_dir.join(domain);
        if !domain_dir.exists() {
            std::fs::create_dir_all(&domain_dir)?;
        }

        let dest = domain_dir.join(pack_name);
        std::fs::copy(&pack_path, &dest)
            .context("failed to copy pack to bundle")?;

        println!("\nPack added: {}", dest.display());
    } else if args.pack_ref.contains('/') || args.pack_ref.contains(':') {
        // OCI reference - would need to pull from registry
        println!("\nOCI pack references require greentic-distributor-client.");
        println!("Use: gtc pack pull {} --out {}", args.pack_ref, providers_dir.display());
        bail!("OCI pack pull not yet integrated - use gtc pack pull first");
    } else {
        bail!("Pack not found: {}", args.pack_ref);
    }

    Ok(())
}

/// Run the `bundle setup` command.
pub fn setup(args: BundleSetupArgs) -> Result<()> {
    let bundle_dir = resolve_bundle_dir(args.bundle)?;

    println!("Setting up provider...");
    println!("  Provider: {}", args.provider_id);
    println!("  Bundle: {}", bundle_dir.display());
    println!("  Tenant: {}", args.tenant);
    println!("  Team: {}", args.team.as_deref().unwrap_or("default"));
    println!("  Env: {}", args.env);

    // Find the provider pack
    let pack_path = find_provider_pack(&bundle_dir, &args.provider_id)?;
    println!("  Pack: {}", pack_path.display());

    // Load answers if provided
    let answers: serde_json::Value = if let Some(answers_path) = &args.answers {
        let content = std::fs::read_to_string(answers_path)
            .context("failed to read answers file")?;
        if answers_path.extension().map_or(false, |e| e == "yaml" || e == "yml") {
            serde_yaml_bw::from_str(&content)?
        } else {
            serde_json::from_str(&content)?
        }
    } else if args.non_interactive {
        bail!("--answers required in non-interactive mode");
    } else {
        // Interactive mode - would use QA wizard
        println!("\nInteractive setup not yet implemented.");
        println!("Use --answers <file> to provide setup answers.");
        bail!("interactive setup not implemented");
    };

    // Persist config
    let config_dir = bundle_dir.join(".providers").join(&args.provider_id);
    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
    }

    let config_file = config_dir.join("config.json");
    let config_json = serde_json::to_string_pretty(&answers)?;
    std::fs::write(&config_file, &config_json)
        .context("failed to write config")?;

    println!("\nSetup complete: {}", config_file.display());
    Ok(())
}

/// Run the `bundle update` command.
pub fn update(args: BundleSetupArgs) -> Result<()> {
    // Update is similar to setup but for existing providers
    println!("Updating provider configuration...");
    setup(args)
}

/// Run the `bundle remove` command.
pub fn remove(args: BundleRemoveArgs) -> Result<()> {
    let bundle_dir = resolve_bundle_dir(args.bundle)?;

    println!("Removing provider...");
    println!("  Provider: {}", args.provider_id);
    println!("  Bundle: {}", bundle_dir.display());

    // Find provider config directory
    let config_dir = bundle_dir.join(".providers").join(&args.provider_id);

    if !config_dir.exists() {
        println!("Provider not configured: {}", args.provider_id);
        return Ok(());
    }

    if !args.force {
        println!("\nThis will remove the provider configuration.");
        println!("Use --force to confirm.");
        bail!("removal cancelled - use --force to confirm");
    }

    // Remove config directory
    std::fs::remove_dir_all(&config_dir)
        .context("failed to remove provider config")?;

    println!("\nProvider removed: {}", args.provider_id);
    Ok(())
}

/// Run the `bundle status` command.
pub fn status(args: BundleStatusArgs) -> Result<()> {
    let bundle_dir = resolve_bundle_dir(args.bundle)?;

    if !bundle_dir.exists() {
        if args.format == "json" {
            println!(r#"{{"exists": false, "path": "{}"}}"#, bundle_dir.display());
        } else {
            println!("Bundle not found: {}", bundle_dir.display());
        }
        return Ok(());
    }

    // Count providers
    let providers_dir = bundle_dir.join("providers");
    let mut pack_count = 0;
    let mut packs = Vec::new();

    if providers_dir.exists() {
        for domain in &["messaging", "events", "oauth", "secrets", "mcp", "other"] {
            let domain_dir = providers_dir.join(domain);
            if domain_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&domain_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(false, |e| e == "gtpack") {
                            pack_count += 1;
                            if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                                packs.push(format!("{}/{}", domain, name));
                            }
                        }
                    }
                }
            }
        }
    }

    // Count configured providers
    let config_dir = bundle_dir.join(".providers");
    let mut configured_count = 0;
    let mut configured = Vec::new();

    if config_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&config_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    configured_count += 1;
                    if let Some(name) = entry.file_name().to_str() {
                        configured.push(name.to_string());
                    }
                }
            }
        }
    }

    if args.format == "json" {
        let status = serde_json::json!({
            "exists": true,
            "path": bundle_dir.display().to_string(),
            "pack_count": pack_count,
            "packs": packs,
            "configured_count": configured_count,
            "configured": configured,
        });
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("Bundle: {}", bundle_dir.display());
        println!("Packs: {} installed", pack_count);
        for pack in &packs {
            println!("  - {}", pack);
        }
        println!("Providers: {} configured", configured_count);
        for provider in &configured {
            println!("  - {}", provider);
        }
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────

fn resolve_bundle_dir(bundle: Option<PathBuf>) -> Result<PathBuf> {
    match bundle {
        Some(path) => Ok(path),
        None => std::env::current_dir().context("failed to get current directory"),
    }
}

fn find_provider_pack(bundle_dir: &PathBuf, provider_id: &str) -> Result<PathBuf> {
    let providers_dir = bundle_dir.join("providers");

    // Check common locations
    for domain in &["messaging", "events", "oauth", "secrets", "mcp", "other"] {
        let pack = providers_dir.join(domain).join(format!("{}.gtpack", provider_id));
        if pack.exists() {
            return Ok(pack);
        }
    }

    // Check flat layout
    let flat = providers_dir.join(format!("{}.gtpack", provider_id));
    if flat.exists() {
        return Ok(flat);
    }

    bail!("provider pack not found: {}", provider_id)
}
