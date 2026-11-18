// SteamGridDB (steamgriddb.com), a second place Change art looks for covers once the player has given Lamplit their
// own API key (Settings > System). Its games have their own ids; their art is downloaded to art\sgdb-<id>\ when the
// player chooses one, so a game without a page on Steam's store can still have a cover, a background and a logo.
// The key stays in steamgriddb.json in the data folder: the page only learns whether there is one.

use super::Art;
use super::art::SteamStore;
use crate::data::{self, data_dir};
use crate::error::{Error, Result};
use crate::files::{nonempty, write_atomic};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const API: &str = "https://www.steamgriddb.com/api/v2";
const FILE: &str = "steamgriddb.json";
/// The most games of one search that get a cover downloaded, so a search stays quick.
const MOST: usize = 12;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    key: String,
}

/// The player's API key, when they gave one.
pub fn key() -> Option<String> {
    let key = data::load_or_default::<Saved>(FILE).key.trim().to_string();
    (!key.is_empty()).then_some(key)
}

/// Whether the player has given a key.
pub fn has_key() -> bool {
    key().is_some()
}

/// Checks `key` with SteamGridDB, then keeps it.
pub fn set_key(store: &SteamStore, key: &str) -> Result<()> {
    let key = key.trim();
    if !looks_like_key(key) {
        return Err(Error::Player(
            "That isn't a SteamGridDB API key. Copy it from steamgriddb.com > Preferences > API, then try again."
                .into(),
        ));
    }
    match get(store, key, &format!("{API}/search/autocomplete/{}", path_part("lamplit"))) {
        Ok(_) => {}
        Err(ureq::Error::StatusCode(401 | 403)) => {
            return Err(Error::Player("SteamGridDB didn't accept that key. Check it and try again.".into()));
        }
        Err(e) => {
            log::warn!("steamgriddb: key check failed: {e}");
            return Err(Error::Player("SteamGridDB didn't answer. Try again in a moment.".into()));
        }
    }
    data::save(FILE, &Saved { key: key.to_string(), ..data::load_or_default(FILE) })?;
    log::info!("steamgriddb: key saved");
    Ok(())
}

pub fn remove_key() -> Result<()> {
    data::save(FILE, &Saved { key: String::new(), ..data::load_or_default(FILE) })?;
    log::info!("steamgriddb: key removed");
    Ok(())
}

/// A key is 32 hexadecimal characters.
fn looks_like_key(key: &str) -> bool {
    key.len() == 32 && key.chars().all(|c| c.is_ascii_hexdigit())
}

/// Where the art of SteamGridDB game `id` goes.
fn dir(id: u32) -> PathBuf {
    data_dir().join("art").join(format!("sgdb-{id}"))
}

/// The art of SteamGridDB game `id` already on the PC.
pub fn local(id: u32) -> Art {
    let dir = dir(id);
    let have = |name: &str| {
        let p = dir.join(name);
        nonempty(&p).then(|| p.to_string_lossy().into_owned())
    };
    Art { portrait: have("portrait.jpg"), hero: have("hero.jpg"), logo: have("logo.png") }
}

/// The games SteamGridDB lists for `term`, with their names, each with its cover downloaded when it has one.
pub fn search(store: &mut SteamStore, key: &str, term: &str) -> std::result::Result<Vec<(u32, String)>, ureq::Error> {
    let found: Vec<(u32, String)> = games(store, key, term)?.into_iter().take(MOST).collect();
    let covers: Vec<(u32, Kind)> =
        found.iter().filter(|(id, _)| local(*id).portrait.is_none()).map(|(id, _)| (*id, Kind::Portrait)).collect();
    download(store, key, &covers);
    Ok(found)
}

/// The games SteamGridDB lists for `term`, in its order.
fn games(store: &SteamStore, key: &str, term: &str) -> std::result::Result<Vec<(u32, String)>, ureq::Error> {
    Ok(parse_games(&get(store, key, &format!("{API}/search/autocomplete/{}", path_part(term)))?))
}

/// The SteamGridDB game that is `name`, as Steam's store search is matched (name_match.rs): Ok(None) when nothing
/// is close enough.
pub fn best_match(store: &SteamStore, key: &str, name: &str) -> std::result::Result<Option<u32>, ureq::Error> {
    for q in super::name_match::queries(name) {
        if let Some(id) = super::name_match::best_match(name, &games(store, key, &q)?) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Downloads the art of SteamGridDB game `id` that isn't on the PC yet, for the player who chose it.
pub fn fetch(store: &mut SteamStore, id: u32) -> Result<()> {
    let key =
        key().ok_or_else(|| Error::Player("Give Lamplit a SteamGridDB key first, in Settings > System.".into()))?;
    fill(store, &key, id);
    if local(id).portrait.is_none() {
        return Err(Error::Player("SteamGridDB has no cover for that game, or didn't answer. Try another.".into()));
    }
    Ok(())
}

/// Downloads whichever of game `id`'s cover, background and logo aren't on the PC yet.
pub fn fill(store: &mut SteamStore, key: &str, id: u32) {
    let have = local(id);
    let missing: Vec<(u32, Kind)> = [
        (Kind::Portrait, have.portrait.is_none()),
        (Kind::Hero, have.hero.is_none()),
        (Kind::Logo, have.logo.is_none()),
    ]
    .into_iter()
    .filter_map(|(k, m)| m.then_some((id, k)))
    .collect();
    download(store, key, &missing);
}

#[derive(Clone, Copy)]
enum Kind {
    Portrait,
    Hero,
    Logo,
}

impl Kind {
    /// The API path and query for game `id`'s images of this kind, static ones only.
    fn url(self, id: u32) -> String {
        match self {
            Kind::Portrait => {
                format!("{API}/grids/game/{id}?dimensions=600x900&mimes=image/jpeg,image/png&types=static")
            }
            Kind::Hero => format!("{API}/heroes/game/{id}?mimes=image/jpeg,image/png&types=static"),
            Kind::Logo => format!("{API}/logos/game/{id}?mimes=image/png&types=static"),
        }
    }

    fn file(self) -> &'static str {
        match self {
            Kind::Portrait => "portrait.jpg",
            Kind::Hero => "hero.jpg",
            Kind::Logo => "logo.png",
        }
    }
}

/// Downloads the best-voted image of each kind for each game, all at once: one search's dozen covers then take about
/// as long as one. Failures are logged; a game shows what there is.
fn download(store: &mut SteamStore, key: &str, jobs: &[(u32, Kind)]) {
    if jobs.is_empty() || !store.online() {
        return;
    }
    let shared: &SteamStore = store;
    let results: Vec<(u32, Kind, Got)> = std::thread::scope(|scope| {
        let running: Vec<_> =
            jobs.iter().map(|&(id, kind)| (id, kind, scope.spawn(move || image(shared, key, id, kind)))).collect();
        running
            .into_iter()
            .map(|(id, kind, t)| (id, kind, t.join().unwrap_or(Got::Nothing("its download thread panicked".into()))))
            .collect()
    });
    for (id, kind, got) in results {
        match got {
            Got::Image(bytes) => {
                let dir = dir(id);
                if let Err(e) =
                    std::fs::create_dir_all(&dir).and_then(|()| write_atomic(&dir.join(kind.file()), &bytes))
                {
                    log::warn!("steamgriddb: can't save {} for game {id}: {e}", kind.file());
                }
            }
            Got::Nothing(why) => log::info!("steamgriddb: no {} for game {id}: {why}", kind.file()),
            Got::Failed(what, e) => store.failed(&what, &e),
        }
    }
}

/// What one download got.
enum Got {
    Image(Vec<u8>),
    Nothing(String),
    Failed(String, ureq::Error),
}

/// The best-voted image of `kind` for game `id`: the API names it, then it comes from SteamGridDB's CDN.
fn image(store: &SteamStore, key: &str, id: u32, kind: Kind) -> Got {
    let url = match get(store, key, &kind.url(id)) {
        Ok(body) => first_url(&body),
        Err(e) => return Got::Failed(format!("steamgriddb {} for game {id}", kind.file()), e),
    };
    let Some(url) = url else { return Got::Nothing("SteamGridDB has none".into()) };
    match store.image(&url) {
        Ok(Some(bytes)) => Got::Image(bytes),
        Ok(None) => Got::Nothing(format!("{url} isn't a JPEG or PNG")),
        Err(e) => Got::Failed(url, e),
    }
}

/// An API call with the player's key.
fn get(store: &SteamStore, key: &str, url: &str) -> std::result::Result<serde_json::Value, ureq::Error> {
    store.agent().get(url).header("Authorization", format!("Bearer {key}")).call()?.body_mut().read_json()
}

/// The games in a search answer, in SteamGridDB's order.
fn parse_games(body: &serde_json::Value) -> Vec<(u32, String)> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| Some((u32::try_from(g["id"].as_u64()?).ok()?, g["name"].as_str()?.to_string())))
        .collect()
}

/// The first image's URL in a grids, heroes or logos answer (SteamGridDB sorts them by votes).
fn first_url(body: &serde_json::Value) -> Option<String> {
    body["data"].as_array()?.iter().find_map(|i| i["url"].as_str()).map(str::to_string)
}

/// `s` made safe for one segment of a URL path.
fn path_part(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_32_hex_characters() {
        assert!(looks_like_key("0123456789abcdef0123456789ABCDEF"));
        assert!(!looks_like_key("0123456789abcdef"));
        assert!(!looks_like_key("0123456789abcdef0123456789abcdeg"));
    }

    #[test]
    fn search_answers_list_games_in_order() {
        let body = json!({ "success": true, "data": [
            { "id": 5_247_009, "name": "Hades II", "types": ["steam"], "verified": true },
            { "id": 7, "name": "No name" , "x": 1 },
            { "name": "No id" },
            { "id": 36_157, "name": "Hades" },
        ] });
        assert_eq!(
            parse_games(&body),
            vec![(5_247_009, "Hades II".into()), (7, "No name".into()), (36_157, "Hades".into())]
        );
        assert!(parse_games(&json!({ "success": false })).is_empty());
    }

    #[test]
    fn the_best_voted_image_comes_first() {
        let body = json!({ "data": [ { "id": 1, "url": "https://cdn2.steamgriddb.com/grid/a.png" }, { "url": "b" } ] });
        assert_eq!(first_url(&body).as_deref(), Some("https://cdn2.steamgriddb.com/grid/a.png"));
        assert_eq!(first_url(&json!({ "data": [] })), None);
    }

    #[test]
    fn search_terms_are_escaped_for_the_path() {
        assert_eq!(path_part("Baldur's Gate 3"), "Baldur%27s%20Gate%203");
        assert_eq!(path_part("a/b?c"), "a%2Fb%3Fc");
        assert_eq!(path_part("Pokémon"), "Pok%C3%A9mon");
    }
}
