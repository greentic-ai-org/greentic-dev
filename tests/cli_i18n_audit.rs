use std::fs;
use std::path::Path;

fn assert_no_hardcoded_clap_help(path: &Path) {
    let text = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", path.display());
    });
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("/// ") && !trimmed.starts_with("/// cli.") {
            panic!(
                "{}:{} contains a hardcoded Clap doc comment: {}",
                path.display(),
                idx + 1,
                trimmed
            );
        }
        if (trimmed.contains("about = \"")
            || trimmed.contains("long_about = \"")
            || trimmed.contains(".about(\"")
            || trimmed.contains(".help(\""))
            && !trimmed.contains("cli.")
        {
            panic!(
                "{}:{} contains a hardcoded CLI help string: {}",
                path.display(),
                idx + 1,
                trimmed
            );
        }
    }
}

#[test]
fn clap_help_text_is_routed_through_i18n() {
    assert_no_hardcoded_clap_help(Path::new("src/cli.rs"));
    assert_no_hardcoded_clap_help(Path::new("src/secrets_cli.rs"));
}
