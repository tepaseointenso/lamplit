// IGDB (igdb.com), About's source for a game Steam's store doesn't have: an emulated game, a recompilation, or one
// from another store whose name Steam doesn't know. It needs the player's own Twitch developer app (a Client ID and a
// Client Secret, Settings > System), which Lamplit trades for an access token. A game is matched to IGDB by name once
// (name_match.rs), and its details are cached for seven days like Steam's. IGDB's descriptions are in English.

use super::art::SteamStore;
use super::details::{Details, Screenshot};
use super::name_match;
use crate::data;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE: &str = "igdb.json";
const API: &str = "https://api.igdb.com/v4";
const IMAGES: &str = "https://images.igdb.com/igdb/image/upload";
/// A name that matched nothing is searched again after this long.
const RETRY_SECS: u64 = 7 * 86_400;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    client_id: String,
    secret: String,
    token: String,
    /// Unix seconds the token stops working.
    token_until: u64,
    /// Game id → IGDB game found by name (None = searched, nothing matched).
    matches: HashMap<String, Match>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Match {
    id: Option<u64>,
    at: u64,
}

fn load() -> Saved {
    data::load_or_default(FILE)
}

fn save(saved: &Saved) -> Result<()> {
    data::save(FILE, saved)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What Settings shows: nothing yet, the Client ID only, or both.
#[derive(Clone, Copy, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum State {
    None,
    Id,
    Ready,
}

pub fn state() -> State {
    let s = load();
    match (s.client_id.is_empty(), s.secret.is_empty()) {
        (true, _) => State::None,
        (false, true) => State::Id,
        (false, false) => State::Ready,
    }
}

/// Keeps the Client ID the player copied. Twitch's ids are 30 lowercase letters and digits.
pub fn set_client_id(text: &str) -> Result<()> {
    let id = text.trim();
    if !looks_like_credential(id) {
        return Err(Error::Player(
            "That isn't a Client ID. Copy it from dev.twitch.tv > Your Console > Applications, then try again.".into(),
        ));
    }
    let mut s = load();
    s.client_id = id.to_string();
    s.secret.clear();
    s.token.clear();
    save(&s)?;
    log::info!("igdb: client id saved");
    Ok(())
}

/// Keeps the Client Secret the player copied, once Twitch gives a token for the pair.
pub fn set_secret(store: &SteamStore, text: &str) -> Result<()> {
    let secret = text.trim();
    let mut s = load();
    if s.client_id.is_empty() {
        return Err(Error::Player("Paste the Client ID first, then the Client Secret.".into()));
    }
    if !looks_like_credential(secret) || secret == s.client_id {
        return Err(Error::Player(
            "That isn't a Client Secret. Copy it from your Twitch application, then try again.".into(),
        ));
    }
    let (token, until) = new_token(store, &s.client_id, secret).map_err(|e| match e {
        ureq::Error::StatusCode(400 | 401 | 403) => {
            Error::Player("Twitch didn't accept that Client ID and Secret. Check both and try again.".into())
        }
        e => {
            log::warn!("igdb: token request failed: {e}");
            Error::Player("Twitch didn't answer. Try again in a moment.".into())
        }
    })?;
    s.secret = secret.to_string();
    s.token = token;
    s.token_until = until;
    save(&s)?;
    log::info!("igdb: client secret saved");
    Ok(())
}

/// Forgets the games no IGDB game matched, so they are searched again at once.
pub fn forget_misses() {
    let mut s = load();
    s.matches.retain(|_, m| m.id.is_some());
    if let Err(e) = save(&s) {
        log::warn!("igdb: can't save: {e}");
    }
}

pub fn remove() -> Result<()> {
    save(&Saved { matches: load().matches, ..Saved::default() })?;
    log::info!("igdb: credentials removed");
    Ok(())
}

fn looks_like_credential(s: &str) -> bool {
    (20..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// An app access token for the pair, and when it stops working.
fn new_token(store: &SteamStore, id: &str, secret: &str) -> std::result::Result<(String, u64), ureq::Error> {
    let body: Value = store
        .agent()
        .post("https://id.twitch.tv/oauth2/token")
        .query("client_id", id)
        .query("client_secret", secret)
        .query("grant_type", "client_credentials")
        .send_empty()?
        .body_mut()
        .read_json()?;
    let token = body["access_token"].as_str().unwrap_or_default().to_string();
    // A minute early, so a request never goes out with a token that ends on the way.
    let until = now() + body["expires_in"].as_u64().unwrap_or(0).saturating_sub(60);
    Ok((token, until))
}

/// The credentials and a working token, renewed when it ran out. None while the player hasn't given both.
fn session(store: &SteamStore) -> Option<(String, String)> {
    let mut s = load();
    if s.client_id.is_empty() || s.secret.is_empty() {
        return None;
    }
    if s.token.is_empty() || now() >= s.token_until {
        match new_token(store, &s.client_id, &s.secret) {
            Ok((token, until)) => {
                s.token = token;
                s.token_until = until;
                if let Err(e) = save(&s) {
                    log::warn!("igdb: can't save the token: {e}");
                }
            }
            Err(e) => {
                log::warn!("igdb: token renewal failed: {e}");
                return None;
            }
        }
    }
    Some((s.client_id, s.token))
}

/// An IGDB query (apicalypse) on `endpoint`.
fn query(
    store: &SteamStore,
    auth: &(String, String),
    endpoint: &str,
    body: &str,
) -> std::result::Result<Value, ureq::Error> {
    store
        .agent()
        .post(format!("{API}/{endpoint}"))
        .header("Client-ID", &auth.0)
        .header("Authorization", format!("Bearer {}", auth.1))
        .header("Accept", "application/json")
        .send(body)?
        .body_mut()
        .read_json()
}

/// `s` as an apicalypse string.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace(['\\', '"'], " "))
}

/// The IGDB game that is `g`, looked up by name once and remembered.
/// The IGDB game called `name`, looked up once for `key` (a game's id, or its id and the SteamGridDB game chosen for
/// it) and remembered.
fn match_game(store: &mut SteamStore, auth: &(String, String), key: &str, name: &str) -> Option<u64> {
    let mut s = load();
    let t = now();
    if let Some(m) = s.matches.get(key).filter(|m| m.id.is_some() || t.saturating_sub(m.at) < RETRY_SECS) {
        return m.id;
    }
    let mut found = None;
    for q in name_match::loose_queries(name) {
        let body = format!("search {}; fields name; where version_parent = null; limit 20;", quoted(&q));
        let items = match query(store, auth, "games", &body) {
            Ok(v) => parse_names(&v),
            Err(e) => {
                store.failed(&format!("igdb search for {name}"), &e);
                return None;
            }
        };
        // name_match works on u32 ids, which IGDB's still are.
        let short: Vec<(u32, String)> =
            items.iter().filter_map(|(id, n)| Some((u32::try_from(*id).ok()?, n.clone()))).collect();
        if let Some(id) = name_match::loose_match(name, &short) {
            found = Some(u64::from(id));
            break;
        }
    }
    log::info!("igdb: {name} → IGDB game {found:?}");
    s.matches.insert(key.to_string(), Match { id: found, at: t });
    if let Err(e) = save(&s) {
        log::warn!("igdb: can't save the match: {e}");
    }
    found
}

/// The games in a search answer, with their names.
fn parse_names(v: &Value) -> Vec<(u64, String)> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| Some((g["id"].as_u64()?, g["name"].as_str()?.to_string())))
        .collect()
}

/// IGDB's details for `g`, as About shows them; Ok(None) when the player gave no credentials or IGDB doesn't know
/// the game. Screenshots are IGDB URLs here; details.rs downloads them.
pub fn details(store: &mut SteamStore, key: &str, name: &str) -> Result<Option<(u64, Details)>> {
    let Some(auth) = session(store) else { return Ok(None) };
    let Some(id) = match_game(store, &auth, key, name) else { return Ok(None) };
    let body = format!(
        "fields summary,first_release_date,genres.name,platforms.name,platforms.abbreviation,\
         involved_companies.developer,involved_companies.publisher,involved_companies.company.name,\
         screenshots.image_id; where id = {id};"
    );
    let v = query(store, &auth, "games", &body).map_err(|e| {
        store.failed("igdb details", &e);
        Error::Player("IGDB didn't answer. Try again in a moment.".into())
    })?;
    Ok(parse_details(&v).map(|d| (id, d)))
}

fn parse_details(v: &Value) -> Option<Details> {
    let g = v.as_array()?.first()?;
    let names = |key: &str, field: &str| -> Vec<String> {
        g[key].as_array().into_iter().flatten().filter_map(|x| x[field].as_str().map(str::to_string)).collect()
    };
    let companies = |role: &str| -> Vec<String> {
        g["involved_companies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c[role] == true)
            .filter_map(|c| c["company"]["name"].as_str().map(str::to_string))
            .collect()
    };
    let screenshots = g["screenshots"]
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|s| {
            let id = s["image_id"].as_str()?;
            id.chars().all(|c| c.is_ascii_alphanumeric()).then(|| Screenshot {
                thumb: format!("{IMAGES}/t_screenshot_med/{id}.jpg"),
                full: format!("{IMAGES}/t_1080p/{id}.jpg"),
            })
        })
        .collect();
    Some(Details {
        appid: 0,
        source: "igdb".into(),
        description: g["summary"].as_str().unwrap_or_default().trim().to_string(),
        developers: companies("developer"),
        publishers: companies("publisher"),
        released: g["first_release_date"].as_i64().map(date).unwrap_or_default(),
        genres: names("genres", "name"),
        platforms: names("platforms", "name"),
        screenshots,
        trailer: None,
    })
}

/// Unix seconds as Steam writes release dates: "24 Feb, 2022".
fn date(secs: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{day} {}, {year}", MONTHS[(month - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn credentials_are_letters_and_digits() {
        assert!(looks_like_credential("abcdefghij0123456789abcdefghij"));
        assert!(!looks_like_credential("short"));
        assert!(!looks_like_credential("abcdefghij 0123456789 abcdefghij"));
    }

    #[test]
    fn release_dates_read_as_steams_do() {
        assert_eq!(date(1_645_660_800), "24 Feb, 2022");
        assert_eq!(date(0), "1 Jan, 1970");
        assert_eq!(date(951_782_400), "29 Feb, 2000");
    }

    #[test]
    fn search_strings_cannot_end_the_query() {
        assert_eq!(quoted(r#"Zelda "TOTK" \ x"#), r#""Zelda  TOTK    x""#);
    }

    #[test]
    fn details_come_from_one_game() {
        let v = json!([{
            "id": 119_388,
            "summary": "An epic adventure across the land and skies of Hyrule. ",
            "first_release_date": 1_683_849_600,
            "genres": [{ "name": "Adventure" }],
            "platforms": [{ "name": "Nintendo Switch", "abbreviation": "Switch" }],
            "involved_companies": [
                { "developer": true, "publisher": false, "company": { "name": "Nintendo EPD" } },
                { "developer": false, "publisher": true, "company": { "name": "Nintendo" } },
            ],
            "screenshots": [{ "image_id": "sc6abc" }, { "image_id": "../bad" }],
        }]);
        let d = parse_details(&v).unwrap();
        assert_eq!(d.source, "igdb");
        assert_eq!(d.description, "An epic adventure across the land and skies of Hyrule.");
        assert_eq!(d.released, "12 May, 2023");
        assert_eq!((d.developers, d.publishers), (vec!["Nintendo EPD".to_string()], vec!["Nintendo".to_string()]));
        assert_eq!(d.platforms, ["Nintendo Switch"]);
        assert_eq!(d.screenshots.len(), 1);
        assert!(d.screenshots[0].full.ends_with("/t_1080p/sc6abc.jpg"));
        assert!(parse_details(&json!([])).is_none());
    }

    #[test]
    fn search_answers_list_games() {
        let v = json!([{ "id": 7346, "name": "The Legend of Zelda: Breath of the Wild" }, { "name": "x" }]);
        assert_eq!(parse_names(&v), vec![(7346, "The Legend of Zelda: Breath of the Wild".into())]);
    }
}
