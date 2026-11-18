// About's data: Steam's, in the language Settings picks (lang.rs), or IGDB's (igdb.rs, English) for a game Steam's
// store doesn't have. Cached for seven days, per language. The cache keeps the sources' URLs; only the IPC answer
// replaces images with asset URLs after downloading them under art, whose scope already lets the page read them.

use super::art::{SteamStore, details_appid, unreached};
use super::{ArtChoice, Library, igdb, lang, sgdb};
use crate::data;
use crate::error::{Error, Result};
use crate::files::{nonempty, write_atomic};
use crate::prefs;
use html5ever::tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, WebviewWindow};

const FRESH_SECS: u64 = 7 * 86_400;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Screenshot {
    pub thumb: String,
    pub full: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Trailer {
    pub url: String,
    pub poster: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Details {
    /// The Steam app, 0 for details from IGDB.
    pub appid: u32,
    /// Where they come from: "steam" or "igdb". Details cached before IGDB have none, and are Steam's.
    #[serde(default)]
    pub source: String,
    pub description: String,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub released: String,
    pub genres: Vec<String>,
    /// The platforms IGDB lists; empty from Steam.
    #[serde(default)]
    pub platforms: Vec<String>,
    pub screenshots: Vec<Screenshot>,
    pub trailer: Option<Trailer>,
}

#[derive(Serialize, Deserialize)]
struct Cache {
    fetched: u64,
    details: Option<Details>,
}

/// One fetch per game at a time: a quick close and reopen must not write the same atomic-write temporary file twice.
/// Another game's details don't wait for this game's downloads.
static FETCHING: Mutex<BTreeMap<u32, Arc<Mutex<()>>>> = Mutex::new(BTreeMap::new());

pub fn get(app: &AppHandle, id: &str) -> Result<Option<Details>> {
    if !prefs::get(app).online_art {
        return Ok(None);
    }
    let Some(game) = app.state::<Library>().game(id) else { return Ok(None) };
    let mut store = SteamStore::new(app);
    // Art chosen on SteamGridDB says which game this is: its Steam app, or its name for IGDB.
    let grid = match game.art_choice {
        Some(ArtChoice::SteamGridDb(grid)) => sgdb::game_info(&mut store, grid).map(|info| (grid, info)),
        _ => None,
    };
    let appid = match &grid {
        Some((_, info)) => info.steam,
        None => details_appid(&game),
    };
    let steam = match appid {
        Some(appid) => {
            let fetching = FETCHING.lock().entry(appid).or_default().clone();
            let _fetch = fetching.lock();
            fetch(app, appid, &mut store)
        }
        None => Ok(None),
    };
    // A game Steam's store doesn't have, or has no details for: IGDB, when the player gave it credentials.
    let (key, name) = match &grid {
        Some((grid, info)) => (format!("{}#sgdb{grid}", game.id), info.name.clone()),
        None => (game.id.clone(), game.name.clone()),
    };
    let result = match steam {
        Ok(None) => from_igdb(app, &key, &name, &mut store),
        other => other,
    };
    // A setting changed during an HTTP request takes effect before anything reaches the page.
    if prefs::get(app).online_art { result } else { Ok(None) }
}

/// Forgets the IGDB lookups that found nothing, so About asks again at once rather than after FRESH_SECS.
pub fn forget_igdb_misses() {
    let Ok(dir) = std::fs::read_dir(data::data_dir().join("details")) else { return };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("igdb-") || !name.ends_with(".json") {
            continue;
        }
        let cached: Option<Cache> = data::load(&format!("details/{name}")).ok().flatten();
        if cached.is_some_and(|c| c.details.is_none()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// IGDB's details of the game called `name` (`key`: whose they are, for the caches), cached like Steam's, with the
/// screenshots downloaded.
fn from_igdb(app: &AppHandle, key: &str, name: &str, store: &mut SteamStore) -> Result<Option<Details>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let file = {
        let mut hash = DefaultHasher::new();
        key.hash(&mut hash);
        format!("details/igdb-{:016x}.json", hash.finish())
    };
    let cached: Option<Cache> = data::load(&file)?;
    let details = if let Some(cache) = cached.filter(|c| fresh(c.fetched, now)) {
        cache.details
    } else {
        if !store.online() || igdb::state() != igdb::State::Ready {
            return Ok(None);
        }
        let details = igdb::details(store, key, name)?.map(|(_, d)| d);
        std::fs::create_dir_all(data::data_dir().join("details"))?;
        data::save(&file, &Cache { fetched: now, details: details.clone() })?;
        details
    };
    let Some(details) = details else { return Ok(None) };
    let dir = data::data_dir().join("art").join("igdb").join("screens");
    localize(app, store, details, &dir).map(Some)
}

fn fetch(app: &AppHandle, appid: u32, store: &mut SteamStore) -> Result<Option<Details>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let language = lang::steam(&prefs::get(app).details_language, &crate::win::region::locale());
    // English kept its old name, so the details cached before languages are still used.
    let name = if language == "english" {
        format!("details/{appid}.json")
    } else {
        format!("details/{appid}-{language}.json")
    };
    let cached: Option<Cache> = data::load(&name)?;
    let details = if let Some(cache) = cached.filter(|c| fresh(c.fetched, now)) {
        cache.details
    } else {
        if !store.online() {
            return Ok(None);
        }
        let body = store.details(appid, language).map_err(|e| unreached(store, "appdetails", &e))?;
        if !store.online() {
            return Ok(None);
        }
        let details = parse(appid, &body)?;
        std::fs::create_dir_all(data::data_dir().join("details"))?;
        data::save(&name, &Cache { fetched: now, details: details.clone() })?;
        details
    };
    let Some(details) = details else { return Ok(None) };
    let dir = data::data_dir().join("art").join(appid.to_string()).join("screens");
    localize(app, store, details, &dir).map(Some)
}

/// `details` with its screenshots and trailer poster downloaded to `dir`, as asset URLs the page can load.
fn localize(app: &AppHandle, store: &mut SteamStore, mut details: Details, dir: &Path) -> Result<Details> {
    let window = app.get_webview_window("main").ok_or(tauri::Error::WindowNotFound)?;
    std::fs::create_dir_all(dir)?;
    let dir = dir.to_path_buf();
    let mut screenshots = Vec::new();
    for screenshot in details.screenshots {
        let thumb = image(store, &window, &dir, &screenshot.thumb)?;
        let full = image(store, &window, &dir, &screenshot.full)?;
        if let (Some(thumb), Some(full)) = (thumb, full) {
            screenshots.push(Screenshot { thumb, full });
        }
    }
    details.screenshots = screenshots;
    if let Some(trailer) = details.trailer.take() {
        details.trailer = image(store, &window, &dir, &trailer.poster)?.map(|poster| Trailer { poster, ..trailer });
    }
    Ok(details)
}

fn fresh(fetched: u64, now: u64) -> bool {
    now.saturating_sub(fetched) < FRESH_SECS
}

fn image(store: &mut SteamStore, window: &WebviewWindow, dir: &Path, url: &str) -> Result<Option<String>> {
    if !store.online() {
        return Ok(None);
    }
    // A URL's hash keeps refreshed screenshots separate from older ones without trusting a remote file name.
    let mut hash = DefaultHasher::new();
    url.hash(&mut hash);
    let path = dir.join(format!("{:016x}.jpg", hash.finish()));
    if !nonempty(&path) {
        // A screenshot that fails to download is left out; the next opening tries it again.
        let bytes = match store.image(url) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(None),
            Err(e) => {
                store.failed(url, &e);
                return Ok(None);
            }
        };
        if !store.online() {
            return Ok(None);
        }
        write_atomic(&path, &bytes)?;
    }
    Ok(Some(window.convert_file_src(&path, None)?))
}

fn parse(appid: u32, body: &Value) -> Result<Option<Details>> {
    let entry = &body[appid.to_string()];
    if entry["success"] == false {
        return Ok(None);
    }
    let data = &entry["data"];
    if entry["success"] != true || !data.is_object() {
        return Err(Error::Internal(format!("Steam appdetails for {appid} has no data")));
    }
    let strings =
        |key: &str| data[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(plain_text).collect();
    let screenshots = data["screenshots"]
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|s| Some(Screenshot { thumb: image_url(&s["path_thumbnail"])?, full: image_url(&s["path_full"])? }))
        .collect();
    Ok(Some(Details {
        appid,
        source: "steam".into(),
        description: plain_text(data["short_description"].as_str().unwrap_or_default()),
        developers: strings("developers"),
        publishers: strings("publishers"),
        released: plain_text(data["release_date"]["date"].as_str().unwrap_or_default()),
        genres: data["genres"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g["description"].as_str().map(plain_text))
            .collect(),
        platforms: Vec::new(),
        screenshots,
        trailer: trailer(&data["movies"]),
    }))
}

/// Store responses name Steam CDNs. Do not turn an unexpected host into a request from the player's PC.
fn steam_url(value: &Value) -> Option<tauri::Url> {
    let url = tauri::Url::parse(value.as_str()?).ok()?;
    let host = url.host_str()?;
    (url.scheme() == "https"
        && (host.ends_with(".steamstatic.com") || host == "steamcdn-a.akamaihd.net")
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

fn image_url(value: &Value) -> Option<String> {
    steam_url(value).map(Into::into)
}

fn trailer(movies: &Value) -> Option<Trailer> {
    let movies = movies.as_array()?;
    let movie = movies.iter().find(|m| m["highlight"] == true).or_else(|| movies.first())?;
    let url = steam_url(&movie["dash_h264"])?;
    if !matches!(url.host_str(), Some("video.akamai.steamstatic.com" | "video.fastly.steamstatic.com"))
        || url.port().is_some()
    {
        return None;
    }
    Some(Trailer { url: url.into(), poster: image_url(&movie["thumbnail"])? })
}

#[derive(Default)]
struct Text(RefCell<String>);

impl TokenSink for Text {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        match token {
            Token::CharacterTokens(text) => self.0.borrow_mut().push_str(&text),
            Token::TagToken(tag) if matches!(tag.name.as_ref(), "br" | "p" | "div" | "li") => {
                self.0.borrow_mut().push(' ');
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn plain_text(html: &str) -> String {
    let input = BufferQueue::default();
    input.push_back(html.into());
    let tokenizer = Tokenizer::new(Text::default(), html5ever::tokenizer::TokenizerOpts::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.into_inner().split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn current_store_data_has_screenshots_and_the_highlighted_h264_manifest() {
        // Steam appdetails for 1245620, fetched 2026-11-10; unrelated store fields removed.
        let body = serde_json::from_str(include_str!("../../tests/appdetails-1245620.json")).unwrap();
        let details = parse(1_245_620, &body).unwrap().unwrap();
        assert!(details.description.starts_with("THE CRITICALLY ACCLAIMED"));
        assert_eq!(details.developers, ["FromSoftware, Inc."]);
        assert_eq!(details.released, "24 Feb, 2022");
        assert_eq!(details.genres, ["Action", "RPG"]);
        assert_eq!(details.screenshots.len(), 8);
        assert!(details.screenshots[0].full.starts_with("https://shared."));
        let movie = &body["1245620"]["data"]["movies"][0];
        assert_eq!(movie["highlight"], true);
        let trailer = details.trailer.unwrap();
        assert_eq!(trailer.url, movie["dash_h264"].as_str().unwrap());
        assert_eq!(trailer.poster, movie["thumbnail"].as_str().unwrap());
    }

    #[test]
    fn only_the_first_eight_screenshots_are_kept() {
        let shot = json!({
            "path_thumbnail": "https://shared.steamstatic.com/thumb.jpg",
            "path_full": "https://shared.steamstatic.com/full.jpg"
        });
        let mut body = json!({"70": {"success": true, "data": {"screenshots": vec![shot; 9]}}});
        assert_eq!(parse(70, &body).unwrap().unwrap().screenshots.len(), 8);
        body["70"]["data"]["screenshots"][0]["path_full"] = json!("https://elsewhere.test/full.jpg");
        assert_eq!(parse(70, &body).unwrap().unwrap().screenshots.len(), 7);
    }

    #[test]
    fn unavailable_is_not_a_malformed_answer() {
        assert_eq!(parse(70, &json!({"70": {"success": false}})).unwrap(), None);
        assert!(parse(70, &json!({})).is_err());
        assert!(parse(70, &json!({"70": {"success": true}})).is_err());
    }

    #[test]
    fn descriptions_are_text_with_decoded_entities() {
        assert_eq!(
            plain_text("<b>Rock &amp; roll</b><br>It&#39;s&nbsp;alive &#x1F525; &copy;"),
            "Rock & roll It's alive 🔥 ©"
        );
        assert_eq!(plain_text("&lt;player&gt; <!-- hidden --> &quot;Hi&quot;"), "<player> \"Hi\"");
    }

    #[test]
    fn trailers_use_the_first_highlight_or_the_first_movie() {
        let poster = "https://shared.steamstatic.com/poster.jpg";
        let first = "https://video.fastly.steamstatic.com/store_trailers/first.mpd?t=1";
        let highlight = "https://video.akamai.steamstatic.com/store_trailers/highlight.mpd";
        let mut movies = json!([
            {"thumbnail": poster, "dash_h264": first},
            {"highlight": true, "thumbnail": poster, "dash_h264": highlight},
            {"highlight": true, "thumbnail": poster, "dash_h264": first}
        ]);
        assert_eq!(trailer(&movies).unwrap(), Trailer { url: highlight.into(), poster: poster.into() });
        movies[1]["highlight"] = json!(false);
        movies[2]["highlight"] = json!(false);
        assert_eq!(trailer(&movies).unwrap().url, first);
    }

    #[test]
    fn trailers_need_h264_dash_on_the_selected_movie() {
        let poster = "https://shared.steamstatic.com/poster.jpg";
        let mut movies = json!([
            {"thumbnail": poster, "dash_h264": "https://video.fastly.steamstatic.com/first.mpd"},
            {
                "highlight": true,
                "thumbnail": poster,
                "hls_h264": "https://video.fastly.steamstatic.com/master.m3u8",
                "dash_av1": "https://video.fastly.steamstatic.com/av1.mpd",
                "mp4": {"max": "https://video.fastly.steamstatic.com/movie.mp4"}
            }
        ]);
        assert_eq!(trailer(&movies), None);
        movies[1]["dash_h264"] = Value::Null;
        assert_eq!(trailer(&movies), None);
        assert_eq!(trailer(&json!([])), None);
        assert_eq!(trailer(&Value::Null), None);
    }

    #[test]
    fn trailer_manifests_only_use_the_two_https_video_hosts() {
        let movie = |url: &str| json!([{"thumbnail": "https://shared.steamstatic.com/poster.jpg", "dash_h264": url}]);
        for host in ["video.akamai.steamstatic.com", "video.fastly.steamstatic.com"] {
            let url = format!("https://{host}/store_trailers/dash_h264.mpd?t=1");
            assert_eq!(trailer(&movie(&url)).unwrap().url, url);
        }
        for url in [
            "http://video.akamai.steamstatic.com/movie.mpd",
            "https://shared.steamstatic.com/movie.mpd",
            "https://video.other.steamstatic.com/movie.mpd",
            "https://video.fastly.steamstatic.com.evil.test/movie.mpd",
            "https://video.fastly.steamstatic.com:444/movie.mpd",
            "https://user@video.fastly.steamstatic.com/movie.mpd",
            "file:///C:/movie.mpd",
        ] {
            assert_eq!(trailer(&movie(url)), None, "{url}");
        }
    }

    #[test]
    fn cache_expires_at_seven_days() {
        assert!(fresh(100, 100 + FRESH_SECS - 1));
        assert!(!fresh(100, 100 + FRESH_SECS));
    }

    #[test]
    fn payload_matches_the_page_fixture() {
        let asset = "http://asset.localhost/C%3A%5Clamplit%5Cart%5C1245620%5Cscreens%5C";
        let details = Details {
            appid: 1_245_620,
            source: "steam".into(),
            description: "Rise, Tarnished, and be guided by grace to brandish the power of the Elden Ring.".into(),
            developers: vec!["FromSoftware, Inc.".into()],
            publishers: vec!["Bandai Namco Entertainment Inc.".into()],
            released: "24 Feb, 2022".into(),
            genres: vec!["Action".into(), "RPG".into()],
            platforms: Vec::new(),
            screenshots: vec![Screenshot { thumb: format!("{asset}thumb.jpg"), full: format!("{asset}full.jpg") }],
            trailer: Some(Trailer {
                url: "https://video.fastly.steamstatic.com/store_trailers/1245620/dash_h264.mpd".into(),
                poster: format!("{asset}poster.jpg"),
            }),
        };
        assert_eq!(serde_json::to_value(details).unwrap(), ipc_fixture!("details"));
    }
}
