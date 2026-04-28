#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolchainPackageSpec {
    pub crate_name: &'static str,
    pub bins: &'static [&'static str],
}

pub const GREENTIC_TOOLCHAIN_PACKAGES: &[ToolchainPackageSpec] = &[
    ToolchainPackageSpec {
        crate_name: "greentic-dev",
        bins: &["greentic-dev"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-operator",
        bins: &["greentic-operator"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-bundle",
        bins: &["greentic-bundle"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-setup",
        bins: &["greentic-setup"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-start",
        bins: &["greentic-start"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-deployer",
        bins: &["greentic-deployer"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-component",
        bins: &["greentic-component"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-flow",
        bins: &["greentic-flow"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-pack",
        bins: &["greentic-pack"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-runner",
        bins: &["greentic-runner"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-gui",
        bins: &["greentic-gui"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-secrets",
        bins: &["greentic-secrets"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-mcp",
        bins: &["greentic-mcp"],
    },
];

#[cfg(test)]
mod tests {
    use super::GREENTIC_TOOLCHAIN_PACKAGES;
    use std::collections::BTreeSet;

    #[test]
    fn catalogue_contains_expected_public_toolchain() {
        let expected = [
            ("greentic-dev", "greentic-dev"),
            ("greentic-operator", "greentic-operator"),
            ("greentic-bundle", "greentic-bundle"),
            ("greentic-setup", "greentic-setup"),
            ("greentic-start", "greentic-start"),
            ("greentic-deployer", "greentic-deployer"),
            ("greentic-component", "greentic-component"),
            ("greentic-flow", "greentic-flow"),
            ("greentic-pack", "greentic-pack"),
            ("greentic-runner", "greentic-runner"),
            ("greentic-gui", "greentic-gui"),
            ("greentic-secrets", "greentic-secrets"),
            ("greentic-mcp", "greentic-mcp"),
        ];

        let actual = catalogue_pairs();
        for pair in expected {
            assert!(actual.contains(&pair), "missing {pair:?}");
        }
    }

    #[test]
    fn catalogue_has_no_duplicate_crate_bin_pairs() {
        let mut seen = BTreeSet::new();
        for package in GREENTIC_TOOLCHAIN_PACKAGES {
            for bin in package.bins {
                assert!(
                    seen.insert((package.crate_name, *bin)),
                    "duplicate crate/bin pair: {}/{}",
                    package.crate_name,
                    bin
                );
            }
        }
    }

    fn catalogue_pairs() -> BTreeSet<(&'static str, &'static str)> {
        GREENTIC_TOOLCHAIN_PACKAGES
            .iter()
            .flat_map(|package| {
                package
                    .bins
                    .iter()
                    .map(move |bin| (package.crate_name, *bin))
            })
            .collect()
    }
}
