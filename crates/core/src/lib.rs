//! Framework-independent settings, locale negotiation and infrastructure contracts.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidSettings,
    InvalidRegistry,
    StorageUnavailable,
    SecretUnavailable,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ErrorCode {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub language: String,
    pub theme: Theme,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "system".into(),
            theme: Theme::System,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Locale {
    pub id: String,
    pub name: String,
    pub direction: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct LocaleRegistry {
    fallback: String,
    locales: Vec<Locale>,
}
#[derive(Deserialize)]
struct RegistryInput {
    fallback: String,
    locales: Vec<Locale>,
}

fn normalized(tag: &str) -> String {
    tag.replace('_', "-").to_ascii_lowercase()
}
fn script(tag: &str) -> Option<&str> {
    tag.split('-')
        .skip(1)
        .find(|part| part.len() == 4 && part.bytes().all(|b| b.is_ascii_alphabetic()))
}
impl LocaleRegistry {
    pub fn from_json(json: &str) -> Result<Self, ErrorCode> {
        let input: RegistryInput =
            serde_json::from_str(json).map_err(|_| ErrorCode::InvalidRegistry)?;
        let registry = Self {
            fallback: input.fallback,
            locales: input.locales,
        };
        let mut names = HashSet::new();
        if registry.locales.is_empty()
            || !registry.locales.iter().any(|l| l.id == registry.fallback)
        {
            return Err(ErrorCode::InvalidRegistry);
        }
        for locale in &registry.locales {
            if locale.id.is_empty()
                || locale.name.is_empty()
                || !["ltr", "rtl"].contains(&locale.direction.as_str())
            {
                return Err(ErrorCode::InvalidRegistry);
            }
            for tag in std::iter::once(&locale.id).chain(locale.aliases.iter()) {
                if tag.is_empty() || !names.insert(normalized(tag)) {
                    return Err(ErrorCode::InvalidRegistry);
                }
            }
        }
        Ok(registry)
    }
    pub fn fallback(&self) -> &str {
        &self.fallback
    }
    pub fn locales(&self) -> &[Locale] {
        &self.locales
    }
    pub fn supports_choice(&self, language: &str) -> bool {
        language == "system" || self.locales.iter().any(|l| l.id == language)
    }
    pub fn validate(&self, settings: &Settings) -> Result<(), ErrorCode> {
        if self.supports_choice(&settings.language) {
            Ok(())
        } else {
            Err(ErrorCode::InvalidSettings)
        }
    }
    fn match_tag(&self, tag: &str) -> Option<&Locale> {
        let tag = normalized(tag);
        if let Some(found) = self.locales.iter().find(|locale| {
            normalized(&locale.id) == tag || locale.aliases.iter().any(|a| normalized(a) == tag)
        }) {
            return Some(found);
        }
        let language = tag.split('-').next()?;
        let requested_script = script(&tag);
        self.locales.iter().find(|locale| {
            let candidate = normalized(&locale.id);
            candidate.split('-').next() == Some(language)
                && match requested_script {
                    Some(wanted) => script(&candidate) == Some(wanted),
                    None => {
                        script(&candidate).is_none()
                            || locale.aliases.iter().any(|a| normalized(a) == language)
                    }
                }
        })
    }
    pub fn resolve(&self, preference: &str, system_locale: Option<&str>) -> &Locale {
        let candidate = if preference == "system" {
            system_locale
        } else {
            Some(preference)
        };
        candidate
            .and_then(|tag| self.match_tag(tag))
            .unwrap_or_else(|| {
                self.locales
                    .iter()
                    .find(|l| l.id == self.fallback)
                    .expect("validated registry fallback")
            })
    }
}

pub trait SettingsRepository {
    fn load(&self) -> Result<Settings, ErrorCode>;
    fn save(&mut self, settings: &Settings) -> Result<(), ErrorCode>;
}
pub trait SecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode>;
    fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode>;
    fn delete(&self, key: &str) -> Result<(), ErrorCode>;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registry() -> LocaleRegistry {
        LocaleRegistry::from_json(include_str!("../../../shared/locales/registry.json")).unwrap()
    }
    #[test]
    fn system_regions_and_uzbek_alias_resolve() {
        let r = registry();
        assert_eq!(r.resolve("system", Some("ru_RU")).id, "ru");
        assert_eq!(r.resolve("system", Some("uz-UZ")).id, "uz-Latn");
        assert_eq!(r.resolve("system", Some("uz-Latn-UZ")).id, "uz-Latn");
        assert_eq!(r.resolve("ru", Some("en")).id, "ru");
    }
    #[test]
    fn unsupported_script_and_unknown_locale_fall_back() {
        let r = registry();
        assert_eq!(r.resolve("system", Some("uz-Cyrl-UZ")).id, "en");
        assert_eq!(r.resolve("removed-locale", Some("ru")).id, "en");
        assert_eq!(r.resolve("system", None).id, "en");
    }
    #[test]
    fn additional_locale_is_data_not_a_language_branch() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../../shared/locales/registry.json")).unwrap();
        value["locales"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"de","name":"Deutsch","direction":"ltr","aliases":[]}));
        let r = LocaleRegistry::from_json(&value.to_string()).unwrap();
        assert_eq!(r.resolve("system", Some("de-DE")).id, "de");
        assert!(
            r.validate(&Settings {
                language: "de".into(),
                ..Settings::default()
            })
            .is_ok()
        );
    }
    #[test]
    fn rejects_ambiguous_registry_and_invalid_settings() {
        assert!(LocaleRegistry::from_json(r#"{"fallback":"en","locales":[]}"#).is_err());
        assert!(
            registry()
                .validate(&Settings {
                    language: "not-shipped".into(),
                    ..Settings::default()
                })
                .is_err()
        );
        assert!(
            serde_json::from_str::<Settings>(r#"{"language":"en","theme":"invalid"}"#).is_err()
        );
    }
}
