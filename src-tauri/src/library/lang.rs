// The language of About's descriptions: the player's choice in Settings > System, else the PC's. Steam's store takes
// its own language names; IGDB has English only.

/// The languages Settings offers, by code, with Steam's name for each. "" is Automatic, the PC's language.
pub const LANGUAGES: [(&str, &str); 13] = [
    ("en", "english"),
    ("es", "spanish"),
    ("es-419", "latam"),
    ("pt", "portuguese"),
    ("pt-br", "brazilian"),
    ("fr", "french"),
    ("de", "german"),
    ("it", "italian"),
    ("ja", "japanese"),
    ("ko", "koreana"),
    ("zh", "schinese"),
    ("ru", "russian"),
    ("pl", "polish"),
];

/// Whether Settings can save `code`.
pub fn known(code: &str) -> bool {
    code.is_empty() || LANGUAGES.iter().any(|(c, _)| *c == code)
}

/// Steam's name for the player's choice, or for the PC's language (`locale`, as Windows names it: "es-CL").
pub fn steam(choice: &str, locale: &str) -> &'static str {
    let code = if choice.is_empty() { from_locale(locale) } else { choice.to_string() };
    LANGUAGES.iter().find(|(c, _)| *c == code).map_or("english", |(_, s)| *s)
}

/// A Windows locale as one of LANGUAGES' codes: Spanish outside Spain is Latin American, Brazil's Portuguese is its
/// own, and the rest go by language.
fn from_locale(locale: &str) -> String {
    let locale = locale.to_ascii_lowercase();
    let mut parts = locale.split('-');
    let lang = parts.next().unwrap_or_default();
    // A script ("zh-Hans-CN") comes before the region.
    let region = parts.find(|p| p.len() == 2 || p.len() == 3).unwrap_or_default();
    match (lang, region) {
        ("es", "es" | "") => "es".into(),
        ("es", _) => "es-419".into(),
        ("pt", "br") => "pt-br".into(),
        (lang, _) => lang.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pcs_language_picks_steams() {
        assert_eq!(steam("", "es-CL"), "latam");
        assert_eq!(steam("", "es-ES"), "spanish");
        assert_eq!(steam("", "pt-BR"), "brazilian");
        assert_eq!(steam("", "pt-PT"), "portuguese");
        assert_eq!(steam("", "de-AT"), "german");
        assert_eq!(steam("", "zh-Hans-CN"), "schinese");
        assert_eq!(steam("", "sw-KE"), "english");
        assert_eq!(steam("", ""), "english");
    }

    #[test]
    fn a_choice_wins_over_the_pc() {
        assert_eq!(steam("fr", "es-CL"), "french");
        assert!(known("") && known("es-419") && !known("xx"));
    }
}
