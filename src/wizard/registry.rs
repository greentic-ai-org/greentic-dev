#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Shell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardRegistration {
    pub key: &'static str,
    pub provider: ProviderKind,
}

pub fn resolve(target: &str, mode: &str) -> Option<WizardRegistration> {
    let key = format!("{target}.{mode}");
    let registration = match key.as_str() {
        "launcher.main" => WizardRegistration {
            key: "launcher.main",
            provider: ProviderKind::Shell,
        },
        _ => return None,
    };
    Some(registration)
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn resolves_supported_keys() {
        assert!(resolve("launcher", "main").is_some());
    }

    #[test]
    fn rejects_unsupported_keys() {
        assert!(resolve("pack", "update").is_none());
    }
}
