use anyhow::{Context, Result, anyhow};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;

use crate::config::Tokens;
use crate::kodik::{Kodik, UA, normalize_url};
use crate::source::{
    Episode, Source, StreamSet, Title, TitleDetails, Translation, episode_sort_key,
};
use crate::strings;

const API: &str = "https://api.yani.tv";
pub const PUBLIC_TOKEN: &str = "yp_ls7tmtv9n74hp";
const PAGE_SIZE: u32 = 20;

#[derive(Deserialize)]
struct Envelope<T> {
    response: T,
}

#[derive(Deserialize)]
struct Named {
    name: Option<String>,
}

#[derive(Deserialize)]
struct EpisodeInfo {
    count: Option<u32>,
    aired: Option<u32>,
}

#[derive(Deserialize)]
struct RawTitle {
    anime_url: String,
    title: String,
    year: Option<u32>,
    #[serde(rename = "type")]
    kind: Option<Named>,
    #[serde(default)]
    other_titles: Vec<String>,
    description: Option<String>,
    episodes: Option<EpisodeInfo>,
}

#[derive(Deserialize)]
struct VideoData {
    player: Option<String>,
    dubbing: Option<String>,
}

#[derive(Deserialize)]
struct RawVideo {
    data: VideoData,
    number: String,
    iframe_url: String,
    #[serde(default = "max_index")]
    index: u64,
}

fn max_index() -> u64 {
    u64::MAX
}

#[derive(Deserialize)]
struct RawDetails {
    #[serde(flatten)]
    title: RawTitle,
    #[serde(default)]
    videos: Vec<RawVideo>,
}

impl From<RawTitle> for Title {
    fn from(r: RawTitle) -> Self {
        Title {
            slug: r.anime_url,
            name: r.title,
            year: r.year.filter(|y| *y > 0),
            kind: r.kind.and_then(|k| k.name),
            other_names: r
                .other_titles
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect(),
            description: r.description.filter(|d| !d.trim().is_empty()),
            episodes_total: r.episodes.as_ref().and_then(|e| e.count).filter(|c| *c > 0),
            episodes_aired: r.episodes.and_then(|e| e.aired),
        }
    }
}

pub fn parse_search(body: &str) -> Result<Vec<Title>> {
    let env: Envelope<Vec<RawTitle>> =
        serde_json::from_str(body).context(strings::ERR_BAD_RESPONSE)?;
    Ok(env.response.into_iter().map(Title::from).collect())
}

pub fn parse_details(body: &str) -> Result<TitleDetails> {
    let env: Envelope<RawDetails> =
        serde_json::from_str(body).context(strings::ERR_BAD_RESPONSE)?;
    let RawDetails { title, mut videos } = env.response;

    videos.retain(|v| {
        v.data
            .player
            .as_deref()
            .is_some_and(|p| p.to_lowercase().contains("kodik"))
    });
    videos.sort_by_key(|v| v.index);

    let mut translations: Vec<Translation> = Vec::new();
    for v in videos {
        let url = normalize_url(&v.iframe_url);
        if url.is_empty() {
            continue;
        }
        let name = v
            .data
            .dubbing
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| "—".into());
        let pos = match translations.iter().position(|t| t.name == name) {
            Some(i) => i,
            None => {
                translations.push(Translation {
                    name,
                    episodes: Vec::new(),
                });
                translations.len() - 1
            }
        };
        let eps = &mut translations[pos].episodes;
        let number = v.number.trim().to_owned();
        if !eps.iter().any(|e| e.number == number) {
            eps.push(Episode { number, url });
        }
    }
    for t in &mut translations {
        t.episodes
            .sort_by(|a, b| episode_sort_key(&a.number).total_cmp(&episode_sort_key(&b.number)));
    }
    Ok(TitleDetails {
        title: title.into(),
        translations,
    })
}

pub struct YummyAnime {
    client: Client,
    tokens: Tokens,
}

#[derive(Deserialize)]
struct Profile {
    nickname: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    token: String,
}

#[derive(Deserialize)]
struct ApiError {
    error: String,
}

pub fn parse_profile(body: &str) -> Result<String> {
    let env: Envelope<Profile> = serde_json::from_str(body).context(strings::ERR_BAD_RESPONSE)?;
    Ok(env.response.nickname)
}

pub fn parse_token(body: &str) -> Result<String> {
    let env: Envelope<TokenResponse> =
        serde_json::from_str(body).context(strings::ERR_BAD_RESPONSE)?;
    let token = env.response.token.trim().to_owned();
    if token.is_empty() {
        return Err(anyhow!(strings::ERR_BAD_RESPONSE));
    }
    Ok(token)
}

fn api_error(status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    match serde_json::from_str::<ApiError>(body) {
        Ok(e) => anyhow!("{} (HTTP {status}): {}", strings::ERR_BAD_RESPONSE, e.error),
        Err(_) => anyhow!("{}: HTTP {status}", strings::ERR_BAD_RESPONSE),
    }
}

impl YummyAnime {
    pub fn new(tokens: Tokens) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(20))
                .build()?,
            tokens,
        })
    }

    fn api(&self, path: &str, query: &[(&str, &str)]) -> Result<String> {
        let mut req = self
            .client
            .get(format!("{API}{path}"))
            .query(query)
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, UA)
            .header("Lang", "ru")
            .header(
                "X-Application",
                self.tokens.app.as_deref().unwrap_or(PUBLIC_TOKEN),
            );
        if let Some(user) = &self.tokens.user {
            req = req.header(AUTHORIZATION, format!("Yummy {user}"));
        }
        let resp = req.send()?;
        let status = resp.status();
        let body = resp.text()?;
        if !status.is_success() {
            return Err(api_error(status, &body));
        }
        Ok(body)
    }

    pub fn profile(&self) -> Result<String> {
        parse_profile(&self.api("/profile", &[])?)
    }

    pub fn refresh_token(&self) -> Result<String> {
        parse_token(&self.api("/profile/token", &[])?)
    }
}

impl Source for YummyAnime {
    fn search(&self, query: &str) -> Result<Vec<Title>> {
        let limit = PAGE_SIZE.to_string();
        parse_search(&self.api(
            "/anime",
            &[("q", query.trim()), ("limit", &limit), ("offset", "0")],
        )?)
    }

    fn details(&self, slug: &str) -> Result<TitleDetails> {
        parse_details(&self.api(&format!("/anime/{slug}"), &[("need_videos", "true")])?)
    }

    fn streams(&self, episode: &Episode) -> Result<StreamSet> {
        Kodik::new(&self.client).streams(&episode.url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = include_str!("../tests/fixtures/yummy_search.json");
    const DETAILS: &str = include_str!("../tests/fixtures/yummy_details.json");

    #[test]
    fn search_results_parsed() {
        let titles = parse_search(SEARCH).unwrap();
        assert!(titles.len() >= 4);
        let first = &titles[0];
        assert_eq!(first.slug, "provozhayushchaya-posledniy-put-friren");
        assert_eq!(first.name, "Провожающая в последний путь Фрирен");
        assert_eq!(first.year, Some(2023));
        assert!(first.kind.is_some());
    }

    #[test]
    fn details_keep_only_kodik_grouped_by_translation_in_api_order() {
        let d = parse_details(DETAILS).unwrap();
        assert_eq!(d.title.episodes_total, Some(28));
        assert!(d.title.other_names.iter().any(|n| n == "Sousou no Frieren"));
        let names: Vec<_> = d.translations.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Озвучка AniLibria"));
        assert!(names.contains(&"Субтитры Crunchyroll"));
        assert!(names.contains(&"Озвучка AniDUB Online"));
        for t in &d.translations {
            assert!(
                t.episodes
                    .iter()
                    .all(|e| e.url.starts_with("https://kodikplayer.com/")),
                "{}",
                t.name
            );
            let nums: Vec<_> = t.episodes.iter().map(|e| e.number.as_str()).collect();
            assert_eq!(nums, ["1", "2"], "{}", t.name);
        }
    }

    #[test]
    fn details_without_kodik_yield_no_translations() {
        let body = r#"{"response":{"anime_url":"x","title":"X","videos":[
            {"data":{"player":"Плеер Aksor","dubbing":"A"},"number":"1","iframe_url":"https://aksor/1"}]}}"#;
        let d = parse_details(body).unwrap();
        assert!(d.translations.is_empty());
        assert_eq!(d.title.year, None);
    }

    #[test]
    fn profile_and_token_responses_parsed() {
        assert_eq!(
            parse_profile(r#"{"response":{"id":1,"nickname":"sw1ft"}}"#).unwrap(),
            "sw1ft"
        );
        assert_eq!(
            parse_token(r#"{"response":{"token":" a.b.c "}}"#).unwrap(),
            "a.b.c"
        );
        assert!(parse_token(r#"{"response":{"token":""}}"#).is_err());
        assert!(parse_profile(r#"{"error":"auth","error_code":1}"#).is_err());
    }

    #[test]
    fn api_error_surfaces_server_message() {
        let e = api_error(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":"Need auth","error_code":1}"#,
        );
        assert!(
            e.to_string().contains("Need auth") && e.to_string().contains("401"),
            "{e}"
        );
        let e = api_error(reqwest::StatusCode::BAD_GATEWAY, "<html>");
        assert!(e.to_string().contains("502"), "{e}");
    }

    #[test]
    fn episodes_sorted_numerically_and_deduplicated() {
        let body = r#"{"response":{"anime_url":"x","title":"X","videos":[
            {"data":{"player":"Плеер Kodik","dubbing":"A"},"number":"10","iframe_url":"//k/10","index":1},
            {"data":{"player":"Плеер Kodik","dubbing":"A"},"number":"2","iframe_url":"//k/2","index":2},
            {"data":{"player":"Плеер Kodik","dubbing":"A"},"number":"2","iframe_url":"//k/2b","index":3},
            {"data":{"player":"Плеер Kodik","dubbing":null},"number":"1","iframe_url":"//k/1","index":4}]}}"#;
        let d = parse_details(body).unwrap();
        let a = &d.translations[0];
        assert_eq!(
            a.episodes
                .iter()
                .map(|e| e.number.as_str())
                .collect::<Vec<_>>(),
            ["2", "10"]
        );
        assert_eq!(a.episodes[0].url, "https://k/2");
        assert_eq!(d.translations[1].name, "—");
    }
}
