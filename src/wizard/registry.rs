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
        "operator.create" => WizardRegistration {
            key: "operator.create",
            provider: ProviderKind::Shell,
        },
        "pack.create" => WizardRegistration {
            key: "pack.create",
            provider: ProviderKind::Shell,
        },
        "pack.build" => WizardRegistration {
            key: "pack.build",
            provider: ProviderKind::Shell,
        },
        "component.scaffold" => WizardRegistration {
            key: "component.scaffold",
            provider: ProviderKind::Shell,
        },
        "component.build" => WizardRegistration {
            key: "component.build",
            provider: ProviderKind::Shell,
        },
        "flow.create" => WizardRegistration {
            key: "flow.create",
            provider: ProviderKind::Shell,
        },
        "flow.wire" => WizardRegistration {
            key: "flow.wire",
            provider: ProviderKind::Shell,
        },
        "bundle.create" => WizardRegistration {
            key: "bundle.create",
            provider: ProviderKind::Shell,
        },
        "dev.doctor" => WizardRegistration {
            key: "dev.doctor",
            provider: ProviderKind::Shell,
        },
        "dev.run" => WizardRegistration {
            key: "dev.run",
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
        assert!(resolve("pack", "create").is_some());
        assert!(resolve("dev", "doctor").is_some());
    }

    #[test]
    fn rejects_unsupported_keys() {
        assert!(resolve("pack", "update").is_none());
    }
}
