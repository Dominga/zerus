//! Advisory Tests/Builds metadata. This grants no execution or dispatch policy.
use serde::{Deserialize, Serialize};

pub const CONFIG_BYTES: usize = 65_536;
pub const PROVIDERS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub name: String,
    pub provider: String,
    pub kind: String,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub reports: Vec<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: u32,
    pub provider: Vec<Provider>,
}
impl Config {
    pub fn check(&self) -> Result<(), String> {
        if self.schema != 1 || self.provider.len() > PROVIDERS {
            return Err("Unsupported config schema or too many providers (maximum 16).".into());
        }
        let mut names = std::collections::BTreeSet::new();
        for p in &self.provider {
            if p.name.is_empty()
                || p.name.len() > 64
                || !p
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || !names.insert(&p.name)
            {
                return Err(
                    "Provider names must be unique ASCII identifiers (1..64 bytes).".into(),
                );
            }
            if !matches!(p.provider.as_str(), "cargo" | "npm" | "pytest" | "cmake") {
                return Err(
                    "Unknown provider; supported metadata: cargo, npm, pytest, cmake.".into(),
                );
            }
            if !matches!(p.kind.as_str(), "test" | "build")
                || p.scopes.is_empty()
                || p.scopes.len() > 2
                || p.scopes
                    .iter()
                    .any(|s| !matches!(s.as_str(), "full" | "focused"))
                || p.reports.len() > 2
                || p.reports
                    .iter()
                    .any(|s| !matches!(s.as_str(), "junit" | "json"))
            {
                return Err("Invalid kind, scope or report format.".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Binds the native-selected checkout object and every observed input.
    pub source_digest: String,
    pub digest: String,
    pub override_config: Option<Config>,
}
impl Selection {
    pub fn check(&self) -> Result<(), String> {
        for d in [&self.source_digest, &self.digest] {
            if d.len() != 64
                || !d
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("Invalid configuration digest.".into());
            }
        }
        if let Some(config) = &self.override_config {
            config.check()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    pub schema: u32,
    pub team: String,
    pub revision: u64,
    pub project: String,
    pub home: String,
    pub checkout: String,
    pub source: String,
    pub source_digest: String,
    pub digest: String,
    pub valid: bool,
    pub providers: Vec<Provider>,
    pub diagnostics: Vec<Diagnostic>,
    pub override_config: Option<Config>,
    pub saved: Option<Selection>,
    pub saved_current: bool,
    pub unsupported: Vec<String>,
}
impl Inspection {
    pub fn selection(&self) -> Selection {
        Selection {
            source_digest: self.source_digest.clone(),
            digest: self.digest.clone(),
            override_config: self.override_config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            schema: 1,
            provider: vec![Provider {
                name: "cargo-test".into(),
                provider: "cargo".into(),
                kind: "test".into(),
                scopes: vec!["full".into()],
                reports: vec![],
                disabled: false,
            }],
        }
    }
    #[test]
    fn unknown_metadata_never_becomes_a_command() {
        let c = config();
        assert!(c.check().is_ok());
        for field in ["schema", "provider", "kind", "scopes", "reports", "name"] {
            let mut c = config();
            match field {
                "schema" => c.schema = 2,
                "provider" => c.provider[0].provider = "shell".into(),
                "kind" => c.provider[0].kind = "runner".into(),
                "scopes" => c.provider[0].scopes = vec![],
                "reports" => c.provider[0].reports = vec!["xml-network".into()],
                _ => c.provider[0].name = "../escape".into(),
            };
            assert!(c.check().is_err(), "{field}");
        }
        let mut many = config();
        many.provider = vec![many.provider[0].clone(); 17];
        assert!(many.check().is_err());
        let mut value = serde_json::to_value(config()).unwrap();
        value["program"] = serde_json::json!("touch marker");
        assert!(serde_json::from_value::<Config>(value).is_err());
    }
}

#[cfg(test)]
mod bound_tests {
    use super::*;
    fn provider() -> Provider {
        Provider {
            name: "cargo-test".into(),
            provider: "cargo".into(),
            kind: "test".into(),
            scopes: vec!["full".into()],
            reports: vec![],
            disabled: false,
        }
    }
    #[test]
    fn provider_counts_names_and_duplicate_names_are_bounded() {
        let mut c = Config {
            schema: 1,
            provider: (0..16)
                .map(|n| Provider {
                    name: format!("p{n}"),
                    ..provider()
                })
                .collect(),
        };
        assert!(c.check().is_ok());
        c.provider.push(Provider {
            name: "p16".into(),
            ..provider()
        });
        assert!(c.check().is_err());
        c.provider = vec![provider(), provider()];
        assert!(c.check().is_err());
        for name in [
            String::new(),
            "a".repeat(65),
            "../escape".into(),
            "é".into(),
        ] {
            c.provider = vec![Provider { name, ..provider() }];
            assert!(c.check().is_err());
        }
    }
    #[test]
    fn kind_scope_and_report_capabilities_are_explicit() {
        for (kind, scopes, reports) in [
            ("runner", vec!["full"], vec![]),
            ("test", vec![], vec![]),
            ("test", vec!["all"], vec![]),
            ("test", vec!["full"; 3], vec![]),
            ("build", vec!["full"], vec!["xml"]),
            ("test", vec!["full"], vec!["json"; 3]),
        ] {
            let c = Config {
                schema: 1,
                provider: vec![Provider {
                    kind: kind.into(),
                    scopes: scopes.into_iter().map(String::from).collect(),
                    reports: reports.into_iter().map(String::from).collect(),
                    ..provider()
                }],
            };
            assert!(c.check().is_err());
        }
    }
    #[test]
    fn selected_digests_cannot_be_empty_truncated_nonhex_or_uppercase() {
        let original = Selection {
            source_digest: "a".repeat(64),
            digest: "b".repeat(64),
            override_config: None,
        };
        assert!(original.check().is_ok());
        for digest in [
            String::new(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
        ] {
            assert!(Selection {
                source_digest: digest.clone(),
                ..original.clone()
            }
            .check()
            .is_err());
            assert!(Selection {
                digest,
                ..original.clone()
            }
            .check()
            .is_err());
        }
    }
}
