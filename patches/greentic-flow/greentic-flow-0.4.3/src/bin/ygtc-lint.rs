use anyhow::{Context, Result as AnyResult};
use clap::Parser;
use greentic_flow::{
    error::FlowError,
    flow_bundle::{FlowBundle, load_and_validate_bundle_with_schema_text},
    json_output::LintJsonOutput,
    lint::{lint_builtin_rules, lint_with_registry},
    registry::AdapterCatalog,
};
use std::{
    ffi::OsStr,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Validate YGTC flows against the schema and optional adapter registry."
)]
struct Cli {
    /// Path to the flow schema JSON file.
    #[arg(long, default_value = "schemas/ygtc.flow.schema.json")]
    schema: PathBuf,
    /// Optional adapter catalog used for adapter_resolvable linting.
    #[arg(long)]
    registry: Option<PathBuf>,
    /// Emit a machine-readable JSON payload describing the lint result for a single flow.
    #[arg(long)]
    json: bool,
    /// Read flow YAML from stdin (requires --json).
    #[arg(long)]
    stdin: bool,
    /// Flow files or directories to lint.
    #[arg(required_unless_present = "stdin")]
    targets: Vec<PathBuf>,
}

#[greentic_types::telemetry::main(service_name = "greentic-flow")]
async fn main() -> AnyResult<()> {
    run()
}

fn run() -> AnyResult<()> {
    let Cli {
        schema,
        registry,
        json,
        stdin,
        targets,
    } = Cli::parse();

    if stdin && !json {
        anyhow::bail!("--stdin currently requires --json");
    }

    if stdin && !targets.is_empty() {
        anyhow::bail!("--stdin cannot be combined with file targets");
    }

    let schema_text = fs::read_to_string(&schema)
        .with_context(|| format!("failed to read schema {}", schema.display()))?;
    let schema_label = schema.display().to_string();

    let registry = if let Some(path) = &registry {
        Some(AdapterCatalog::load_from_file(path)?)
    } else {
        None
    };

    if json {
        let stdin_content = if stdin {
            Some(read_stdin_flow()?)
        } else {
            None
        };
        return run_json(
            &targets,
            stdin_content,
            &schema_text,
            &schema_label,
            &schema,
            registry.as_ref(),
        );
    }

    let mut failures = 0usize;
    for target in &targets {
        lint_path(
            target,
            &schema_text,
            &schema_label,
            &schema,
            registry.as_ref(),
            &mut failures,
        )?;
    }

    if failures == 0 {
        println!("All flows valid");
        Ok(())
    } else {
        Err(anyhow::anyhow!("{failures} flow(s) failed validation"))
    }
}

fn lint_path(
    path: &Path,
    schema_text: &str,
    schema_label: &str,
    schema_path: &Path,
    registry: Option<&AdapterCatalog>,
    failures: &mut usize,
) -> AnyResult<()> {
    if path.is_file() {
        lint_file(
            path,
            schema_text,
            schema_label,
            schema_path,
            registry,
            failures,
        )?;
    } else if path.is_dir() {
        let entries = fs::read_dir(path)
            .with_context(|| format!("failed to read directory {}", path.display()))?;
        for entry in entries {
            let entry = entry
                .with_context(|| format!("failed to read directory entry in {}", path.display()))?;
            lint_path(
                &entry.path(),
                schema_text,
                schema_label,
                schema_path,
                registry,
                failures,
            )?;
        }
    }
    Ok(())
}

fn lint_file(
    path: &Path,
    schema_text: &str,
    schema_label: &str,
    schema_path: &Path,
    registry: Option<&AdapterCatalog>,
    failures: &mut usize,
) -> AnyResult<()> {
    if path.extension() != Some(OsStr::new("ygtc")) {
        return Ok(());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    match lint_flow(
        &content,
        Some(path),
        schema_text,
        schema_label,
        schema_path,
        registry,
    ) {
        Ok(result) => {
            if result.lint_errors.is_empty() {
                println!("OK  {} ({})", path.display(), result.bundle.id);
            } else {
                *failures += 1;
                eprintln!("ERR {}:", path.display());
                for err in result.lint_errors {
                    eprintln!("  {err}");
                }
            }
        }
        Err(err) => {
            *failures += 1;
            eprintln!("ERR {}: {err}", path.display());
        }
    }
    Ok(())
}

struct LintResult {
    bundle: FlowBundle,
    lint_errors: Vec<String>,
}

#[allow(clippy::result_large_err)]
fn lint_flow(
    content: &str,
    source_path: Option<&Path>,
    schema_text: &str,
    schema_label: &str,
    schema_path: &Path,
    registry: Option<&AdapterCatalog>,
) -> Result<LintResult, FlowError> {
    let (bundle, ir) = load_and_validate_bundle_with_schema_text(
        content,
        schema_text,
        schema_label.to_string(),
        Some(schema_path),
        source_path,
    )?;
    let lint_errors = if let Some(cat) = registry {
        lint_with_registry(&ir, cat)
    } else {
        lint_builtin_rules(&ir)
    };
    Ok(LintResult {
        bundle,
        lint_errors,
    })
}

fn run_json(
    targets: &[PathBuf],
    stdin_content: Option<String>,
    schema_text: &str,
    schema_label: &str,
    schema_path: &Path,
    registry: Option<&AdapterCatalog>,
) -> AnyResult<()> {
    let (content, source_display, source_path) = if let Some(stdin_flow) = stdin_content {
        (
            stdin_flow,
            "<stdin>".to_string(),
            Some(Path::new("<stdin>")),
        )
    } else {
        if targets.len() != 1 {
            anyhow::bail!("--json mode expects exactly one target file");
        }
        let target = &targets[0];
        if target.is_dir() {
            anyhow::bail!(
                "--json target must be a file, found directory {}",
                target.display()
            );
        }
        if target.extension() != Some(OsStr::new("ygtc")) {
            anyhow::bail!("--json target must be a .ygtc file");
        }
        let content = fs::read_to_string(target)
            .with_context(|| format!("failed to read {}", target.display()))?;
        (
            content,
            target.display().to_string(),
            Some(target.as_path()),
        )
    };

    let output = match lint_flow(
        &content,
        source_path,
        schema_text,
        schema_label,
        schema_path,
        registry,
    ) {
        Ok(result) => {
            if result.lint_errors.is_empty() {
                LintJsonOutput::success(result.bundle)
            } else {
                LintJsonOutput::lint_failure(result.lint_errors, Some(source_display.clone()))
            }
        }
        Err(err) => LintJsonOutput::error(err),
    };

    let ok = output.ok;
    let line = output.into_string();
    write_stdout_line(&line)?;
    if ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("validation failed"))
    }
}

fn read_stdin_flow() -> AnyResult<String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("failed to read flow YAML from stdin")?;
    Ok(buf)
}

fn write_stdout_line(line: &str) -> AnyResult<()> {
    let mut stdout = io::stdout().lock();
    match writeln!(stdout, "{line}") {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}
