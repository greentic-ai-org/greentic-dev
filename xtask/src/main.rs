use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use convert_case::{Case, Casing};
use once_cell::sync::Lazy;

static WORKSPACE_ROOT: Lazy<PathBuf> = Lazy::new(|| {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("xtask is located inside the workspace root")
        .to_path_buf()
});

const TEMPLATE_COMPONENT_CARGO: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/component_Cargo.toml"
));
const TEMPLATE_SRC_LIB: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/src_lib.rs"));
const TEMPLATE_SRC_DESCRIBE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/src_describe.rs"
));
const TEMPLATE_TEST_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/tests_schema_validates_examples.rs"
));
const TEMPLATE_EXAMPLE_MIN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/examples_flows_min.yaml"
));
const TEMPLATE_SCHEMA_NODE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/schemas_v1_node.schema.json"
));
const TEMPLATE_CI_WORKFLOW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/.github_workflows_ci.yml"
));
const TEMPLATE_DOCS_GETTING_STARTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/docs_getting_started.md"
));
const TEMPLATE_DOCS_CONFIG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/docs_config.md"
));
const TEMPLATE_DOCS_TESTING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/docs_testing.md"
));
const TEMPLATE_DOCS_CI: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/docs_ci.md"));
const TEMPLATE_DOCS_SECURITY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/docs_security.md"
));

#[derive(Parser)]
#[command(name = "xtask")]
#[command(version)]
#[command(about = "Developer tooling tasks for the Greentic workspace")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new component repository
    NewComponent { name: String },
    /// Validate generated assets (reserved for future use)
    Validate {},
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::NewComponent { name } => new_component(&name),
        Commands::Validate {} => {
            println!("validate command is not implemented yet");
            Ok(())
        }
    }
}

fn new_component(name: &str) -> Result<()> {
    let context = TemplateContext::new(name)?;
    let component_dir = WORKSPACE_ROOT.join(context.component_dir());

    if component_dir.exists() {
        bail!(
            "component directory `{}` already exists",
            component_dir.display()
        );
    }

    println!(
        "Creating new component scaffold at `{}`",
        component_dir.display()
    );

    // Pre-create directory structure.
    create_dir(component_dir.join("src"))?;
    create_dir(component_dir.join("tests"))?;
    create_dir(component_dir.join("schemas/v1"))?;
    create_dir(component_dir.join("examples/flows"))?;
    create_dir(component_dir.join(".github/workflows"))?;
    create_dir(component_dir.join("docs"))?;

    // Write templated files.
    write_template(
        &component_dir.join("Cargo.toml"),
        TEMPLATE_COMPONENT_CARGO,
        &context,
    )?;
    write_template(
        &component_dir.join("src/lib.rs"),
        TEMPLATE_SRC_LIB,
        &context,
    )?;
    write_template(
        &component_dir.join("src/describe.rs"),
        TEMPLATE_SRC_DESCRIBE,
        &context,
    )?;
    write_template(
        &component_dir.join("tests/schema_validates_examples.rs"),
        TEMPLATE_TEST_SCHEMA,
        &context,
    )?;
    write_template(
        &component_dir.join("examples/flows/min.yaml"),
        TEMPLATE_EXAMPLE_MIN,
        &context,
    )?;
    write_template(
        &component_dir.join(format!(
            "schemas/v1/{}.node.schema.json",
            context.component_kebab
        )),
        TEMPLATE_SCHEMA_NODE,
        &context,
    )?;
    write_template(
        &component_dir.join(".github/workflows/ci.yml"),
        TEMPLATE_CI_WORKFLOW,
        &context,
    )?;
    write_template(
        &component_dir.join("docs/getting-started.md"),
        TEMPLATE_DOCS_GETTING_STARTED,
        &context,
    )?;
    write_template(
        &component_dir.join("docs/config.md"),
        TEMPLATE_DOCS_CONFIG,
        &context,
    )?;
    write_template(
        &component_dir.join("docs/testing.md"),
        TEMPLATE_DOCS_TESTING,
        &context,
    )?;
    write_template(
        &component_dir.join("docs/ci.md"),
        TEMPLATE_DOCS_CI,
        &context,
    )?;
    write_template(
        &component_dir.join("docs/security.md"),
        TEMPLATE_DOCS_SECURITY,
        &context,
    )?;

    println!(
        "Component `{}` scaffolded successfully.",
        context.component_name
    );

    Ok(())
}

fn create_dir(path: PathBuf) -> Result<()> {
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create directory `{}`", path.display()))
}

fn write_template(path: &Path, template: &str, context: &TemplateContext) -> Result<()> {
    if path.exists() {
        bail!("file `{}` already exists", path.display());
    }

    let rendered = render_template(template, context);
    fs::write(path, rendered).with_context(|| format!("failed to write `{}`", path.display()))
}

fn render_template(template: &str, context: &TemplateContext) -> String {
    let mut output = template.to_owned();
    for (key, value) in &context.placeholders {
        let token = format!("{{{{{key}}}}}");
        output = output.replace(&token, value);
    }
    output
}

struct TemplateContext {
    component_name: String,
    component_kebab: String,
    placeholders: HashMap<String, String>,
}

impl TemplateContext {
    fn new(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("component name cannot be empty");
        }

        let component_kebab = trimmed.to_case(Case::Kebab);
        let component_snake = trimmed.to_case(Case::Snake);
        let component_pascal = trimmed.to_case(Case::Pascal);

        let component_name = component_kebab.clone();

        let mut placeholders = HashMap::new();
        placeholders.insert("component_name".into(), component_name.clone());
        placeholders.insert("component_kebab".into(), component_kebab.clone());
        placeholders.insert("component_snake".into(), component_snake.clone());
        placeholders.insert("component_pascal".into(), component_pascal.clone());
        placeholders.insert(
            "component_dir".into(),
            format!("component-{}", component_kebab),
        );

        Ok(Self {
            component_name,
            component_kebab,
            placeholders,
        })
    }

    fn component_dir(&self) -> String {
        format!("component-{}", self.component_kebab)
    }
}
