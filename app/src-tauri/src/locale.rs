//! The language AutoScene speaks: the person's choice, else the first
//! language on the system's list that it has words for.

use as_api::{Lang, UiLanguage, UiLocale};

/// What AutoScene speaks, by language code. English comes first: it is what
/// a system speaking none of these gets.
pub(crate) const SPOKEN: &[(&str, Lang)] = &[("en", Lang::En)];

/// What `setting` comes to on this computer, and what Automatic would. On a
/// Mac the list follows the language chosen for AutoScene in System Settings
/// too.
pub fn ui_locale(setting: UiLanguage) -> UiLocale {
    let comes_to = |setting| resolve_from(setting, sys_locale::get_locales(), SPOKEN);
    UiLocale { setting, resolved: comes_to(setting), automatic: comes_to(UiLanguage::System) }
}

/// A language chosen goes before everything on the system's list.
fn resolve_from<T: Copy>(setting: UiLanguage, system: impl IntoIterator<Item = String>, spoken: &[(&str, T)]) -> T {
    first_spoken(setting.code().map(str::to_string).into_iter().chain(system), spoken)
}

/// The first of `system` found in `spoken`, by language alone: "ro-MD" is
/// Romanian wherever it is written. Nothing found gives `spoken`'s first.
fn first_spoken<T: Copy>(system: impl IntoIterator<Item = String>, spoken: &[(&str, T)]) -> T {
    let found = system.into_iter().find_map(|tag| {
        let code = language_of(&tag);
        spoken.iter().find(|(known, _)| *known == code).map(|(_, lang)| *lang)
    });
    found.unwrap_or(spoken[0].1)
}

/// "ro-RO", "ro_MD" and "mo" (Moldavian, the old code for Romanian) → "ro".
fn language_of(tag: &str) -> String {
    let code = tag.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
    if code == "mo" { "ro".into() } else { code }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Romanian, before it is one of the languages AutoScene speaks.
    const WITH_ROMANIAN: &[(&str, &str)] = &[("en", "en"), ("ro", "ro")];

    fn resolved(system: &[&str]) -> &'static str {
        first_spoken(system.iter().map(|tag| tag.to_string()), WITH_ROMANIAN)
    }

    #[test]
    fn the_first_language_it_speaks_wins() {
        assert_eq!(resolved(&["ro-RO"]), "ro");
        assert_eq!(resolved(&["ro-MD"]), "ro");
        assert_eq!(resolved(&["mo"]), "ro");
        assert_eq!(resolved(&["RO_ro"]), "ro");
        assert_eq!(resolved(&["de-DE", "ro-RO"]), "ro");
        assert_eq!(resolved(&["de-DE", "en-US"]), "en");
        assert_eq!(resolved(&["en-US", "ro-RO"]), "en");
        assert_eq!(resolved(&["en-US"]), "en");
    }

    #[test]
    fn a_system_it_cant_speak_to_gets_english() {
        assert_eq!(resolved(&[]), "en");
        assert_eq!(resolved(&["de-DE", "fr-FR"]), "en");
    }

    #[test]
    fn automatic_follows_the_system() {
        let system = ["ro-RO".to_string()];
        assert_eq!(resolve_from(UiLanguage::System, system, WITH_ROMANIAN), "ro");
    }

    #[test]
    fn a_choice_is_kept_whatever_the_system_speaks() {
        let system = ["ro-RO".to_string()];
        assert_eq!(resolve_from(UiLanguage::En, system, WITH_ROMANIAN), "en");
    }
}
