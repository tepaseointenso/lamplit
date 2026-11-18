// Matching a game from another store to its page on Steam's public store by name: what to search for, and which
// search result is the game. art.rs makes the requests.

/// What to search the store for: the name, then the name without its edition when that differs.
pub fn queries(name: &str) -> Vec<String> {
    let mut queries = vec![name.to_string()];
    let stripped = strip_edition(name);
    if stripped != norm(name) {
        queries.push(stripped);
    }
    queries
}

/// The same name (editions aside: "Tomb Raider: Definitive Edition" is Steam's "Tomb Raider Game of the Year"),
/// else the longest store title the game's name starts with, never "Shadow of the Tomb Raider".
pub fn best_match(name: &str, items: &[(u32, String)]) -> Option<u32> {
    let wanted = [norm(name), strip_edition(name)];
    if let Some((id, _)) = items.iter().find(|(_, n)| wanted.contains(&norm(n)) || wanted.contains(&strip_edition(n))) {
        return Some(*id);
    }
    items
        .iter()
        .map(|(id, n)| (*id, norm(n)))
        .filter(|(_, n)| !n.is_empty() && wanted.iter().any(|w| w.starts_with(&format!("{n} "))))
        .max_by_key(|(_, n)| n.len())
        .map(|(id, _)| id)
}

/// More to search for, for databases that list every platform's games (IGDB, SteamGridDB) and so search less
/// forgivingly than Steam: the name, without its edition, without a year ("... 2026"), and the part before a colon
/// or dash ("Call of Duty: Modern Warfare 4 - Season 1").
pub fn loose_queries(name: &str) -> Vec<String> {
    let mut out = queries(name);
    let mut push = |q: String| {
        if !q.is_empty() && !out.iter().any(|o| norm(o) == norm(&q)) {
            out.push(q);
        }
    };
    let without_year = without_years(&strip_edition(name));
    push(without_year.clone());
    if let Some(head) = name.split([':', '-', '–', '(']).next() {
        push(without_years(&strip_edition(head)));
    }
    // A name glued together as some folders are ("GTAVEnhanced"): split where a capital follows a small letter.
    let spaced = split_camel(name);
    if spaced != name {
        push(without_years(&strip_edition(&spaced)));
    }
    out
}

/// `best_match`, else the result most like the name: most of their words shared, and the same numbers, so "Modern
/// Warfare 4" is never "Modern Warfare 2". Years in the player's name ("... 2026") don't count against a match.
pub fn loose_match(name: &str, items: &[(u32, String)]) -> Option<u32> {
    if let Some(id) = best_match(name, items) {
        return Some(id);
    }
    let wanted = words(&without_years(&strip_edition(&split_camel(name))));
    if wanted.is_empty() {
        return None;
    }
    let numbers =
        |w: &[String]| -> Vec<String> { w.iter().filter(|t| t.chars().all(|c| c.is_ascii_digit())).cloned().collect() };
    let want_numbers = numbers(&wanted);
    items
        .iter()
        .filter_map(|(id, n)| {
            let got = words(&without_years(&strip_edition(n)));
            if got.is_empty() || numbers(&got) != want_numbers {
                return None;
            }
            let shared = got.iter().filter(|t| wanted.contains(t)).count();
            let score = (2 * shared) as f64 / (got.len() + wanted.len()) as f64;
            // The same words in the same order break a tie.
            let score = score + if got == wanted { 0.01 } else { 0.0 };
            (score >= 0.8).then_some((*id, score))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// The words of a name, as `norm` writes them, with roman numerals II to X as numbers.
fn words(s: &str) -> Vec<String> {
    const ROMAN: [&str; 9] = ["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];
    norm(s)
        .split(' ')
        .filter(|w| !w.is_empty() && *w != "the" && *w != "of" && *w != "a")
        .map(|w| ROMAN.iter().position(|r| *r == w).map_or_else(|| w.to_string(), |i| (i + 2).to_string()))
        .collect()
}

/// `s` without four-digit years from 1970 to 2099.
fn without_years(s: &str) -> String {
    norm(s)
        .split(' ')
        .filter(|w| !(w.len() == 4 && w.parse::<u32>().is_ok_and(|y| (1970..2100).contains(&y))))
        .collect::<Vec<_>>()
        .join(" ")
}

/// "GTAVEnhanced" as "GTAV Enhanced", "SuperMario" as "Super Mario": a space before a capital that follows a small
/// letter, or that starts a word after a run of capitals.
fn split_camel(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1);
        let after_lower = prev.is_some_and(char::is_lowercase);
        let ends_acronym = prev.is_some_and(char::is_uppercase) && next.is_some_and(|n| n.is_lowercase());
        if c.is_uppercase() && (after_lower || ends_acronym) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

pub(super) fn norm(s: &str) -> String {
    let mapped: String = s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' }).collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_edition(name: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "definitive edition", "pc edition", "game of the year edition", "game of the year", "goty edition", "goty",
        "deluxe edition", "complete edition", "ultimate edition", "standard edition", "gold edition",
    ];
    let mut n = norm(name);
    for s in SUFFIXES {
        if let Some(rest) = n.strip_suffix(s) {
            n = rest.trim_end().to_string();
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_store_titles() {
        let shadow = vec![(750_920, "Shadow of the Tomb Raider: Definitive Edition".to_string())];
        assert_eq!(best_match("Tomb Raider: Definitive Edition", &shadow), None);
        let tr = vec![
            (391_220, "Rise of the Tomb Raider™".to_string()),
            (750_920, "Shadow of the Tomb Raider: Definitive Edition".to_string()),
            (203_160, "Tomb Raider Game of the Year".to_string()),
            (8000, "Tomb Raider: Anniversary".to_string()),
        ];
        assert_eq!(best_match("Tomb Raider: Definitive Edition", &tr), Some(203_160));
        let skate = vec![(1_263_240, "Skate Story".to_string()), (3_354_750, "skate.".to_string())];
        assert_eq!(best_match("Skate", &skate), Some(3_354_750));
        let cities = vec![(949_230, "Cities: Skylines II".to_string())];
        assert_eq!(best_match("Cities: Skylines II - PC Edition", &cities), Some(949_230));
        let ghost = vec![(460_930, "Tom Clancy's Ghost Recon® Wildlands".to_string())];
        assert_eq!(best_match("Tom Clancy's Ghost Recon Wildlands", &ghost), Some(460_930));
    }

    #[test]
    fn loose_matching_forgives_years_and_punctuation() {
        assert!(
            loose_queries("Call of Duty Modern Warfare 4 2026").contains(&"call of duty modern warfare 4".to_string())
        );
        let found = vec![
            (1, "Call of Duty: Modern Warfare 2".to_string()),
            (2, "Call of Duty 4: Modern Warfare".to_string()),
            (3, "Call of Duty: Modern Warfare 4".to_string()),
        ];
        assert_eq!(loose_match("Call of Duty Modern Warfare 4 2026", &found), Some(3));
        // Other numbers are another game.
        assert_eq!(loose_match("Call of Duty Modern Warfare 3", &found), None);
        let zelda = vec![(7346, "The Legend of Zelda: Breath of the Wild".to_string())];
        // Most words shared: the short name the player gave is still the game.
        assert_eq!(loose_match("Zelda Breath of the Wild", &zelda), Some(7346));
        // The same words in another order are a weaker match than in the same order.
        let reversed =
            vec![(2, "Call of Duty 4: Modern Warfare".to_string()), (3, "Call of Duty: Modern Warfare 4".to_string())];
        assert_eq!(loose_match("Call of Duty Modern Warfare 4", &reversed), Some(3));
        let reversed =
            vec![(3, "Call of Duty: Modern Warfare 4".to_string()), (2, "Call of Duty 4: Modern Warfare".to_string())];
        assert_eq!(loose_match("Call of Duty Modern Warfare 4", &reversed), Some(3));
        assert_eq!(loose_match("Legend of Zelda Breath of the Wild", &zelda), Some(7346));
        let gta = vec![(1020, "Grand Theft Auto V".to_string())];
        assert_eq!(loose_match("Grand Theft Auto 5", &gta), Some(1020));
        assert!(loose_queries("GTAVEnhanced").contains(&"gtav enhanced".to_string()));
    }

    #[test]
    fn searches_without_the_edition_too() {
        assert_eq!(queries("Tomb Raider: Definitive Edition"), vec!["Tomb Raider: Definitive Edition", "tomb raider"]);
        assert_eq!(queries("Skate"), vec!["Skate"]);
    }

    #[test]
    fn strips_editions() {
        assert_eq!(strip_edition("Some Game GOTY Edition"), "some game");
        assert_eq!(strip_edition("Some Game: Game of the Year Edition"), "some game");
        // Suffixes stack, in the order SUFFIXES lists them.
        assert_eq!(strip_edition("Some Game PC Edition: Definitive Edition"), "some game");
        assert_eq!(strip_edition("Edition"), "edition");
    }
}
