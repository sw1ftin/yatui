use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use percent_encoding::percent_decode_str;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, REFERER, USER_AGENT};
use serde_json::{Map, Value};

use crate::source::{StreamSet, Subtitle, Variant};
use crate::strings;

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36";
const DEFAULT_ENDPOINT: &str = "/ftor";
const REQUIRED_PARAMS: [&str; 6] = ["d", "d_sign", "pd", "pd_sign", "ref", "ref_sign"];
const SUBTITLE_EXTS: [&str; 4] = [".vtt", ".srt", ".ass", ".ssa"];

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerContext {
    pub host: String,
    pub kind: String,
    pub id: String,
    pub hash: String,
    pub params: Map<String, Value>,
}

impl PlayerContext {
    fn param(&self, key: &str) -> &str {
        self.params.get(key).and_then(Value::as_str).unwrap_or("")
    }

    pub fn form(&self) -> Vec<(&'static str, String)> {
        let reference = percent_decode_str(self.param("ref"))
            .decode_utf8_lossy()
            .into_owned();
        vec![
            ("hash", self.hash.clone()),
            ("id", self.id.clone()),
            ("type", self.kind.clone()),
            ("d", self.param("d").to_owned()),
            ("d_sign", self.param("d_sign").to_owned()),
            ("pd", self.param("pd").to_owned()),
            ("pd_sign", self.param("pd_sign").to_owned()),
            ("ref", reference),
            ("ref_sign", self.param("ref_sign").to_owned()),
            ("bad_user", "true".to_owned()),
            ("cdn_is_working", "true".to_owned()),
        ]
    }

    pub fn candidate_hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = Vec::new();
        let mut push = |h: Option<String>| {
            if let Some(h) = h
                && !hosts.contains(&h)
            {
                hosts.push(h);
            }
        };
        push(host_url(self.param("d")));
        push(Some(self.host.clone()));
        push(host_url(self.param("pd")));
        push(Some("https://kodik.cc".into()));
        push(Some("https://kodik.info".into()));
        hosts
    }
}

pub fn normalize_url(url: &str) -> String {
    let url = url.trim();
    match url.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_owned(),
    }
}

fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| format!("{scheme}://{host}"))
}

fn host_url(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if value.contains("://") {
        origin(value)
    } else {
        origin(&format!("https://{value}"))
    }
}

pub fn quoted_after<'a>(source: &'a str, anchor: &str) -> Option<&'a str> {
    let start = source.find(anchor)? + anchor.len();
    let rest = source[start..].trim_start();
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = &rest[1..];
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some(&body[..i]);
        }
    }
    None
}

fn parse_params(raw: &str) -> Option<Map<String, Value>> {
    let candidates = [
        raw.trim().to_owned(),
        raw.replace("\\/", "/"),
        raw.replace("\\\"", "\"").replace("\\/", "/"),
        raw.replace("\\\\", "\\")
            .replace("\\\"", "\"")
            .replace("\\/", "/"),
        raw.replace("&quot;", "\"").replace("\\/", "/"),
    ];
    let mut parsed = candidates
        .iter()
        .find_map(|c| serde_json::from_str::<Map<String, Value>>(c.trim()).ok())?;
    let normalized = raw.replace("\\\"", "\"").replace("\\/", "/");
    for key in REQUIRED_PARAMS {
        let blank = parsed
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty);
        if blank && let Some(v) = json_string_value(&normalized, key) {
            parsed.insert(key.to_owned(), Value::String(v.to_owned()));
        }
    }
    Some(parsed)
}

fn json_string_value<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\":\"");
    let from = raw.find(&marker)? + marker.len();
    let len = raw[from..].find('"')?;
    Some(&raw[from..from + len])
}

pub fn parse_player_context(page: &str, iframe_url: &str) -> Result<PlayerContext> {
    let kind = quoted_after(page, "vInfo.type =").or_else(|| quoted_after(page, "var type ="));
    let hash = quoted_after(page, "vInfo.hash =");
    let id = quoted_after(page, "vInfo.id =").or_else(|| quoted_after(page, "var videoId ="));
    let params = quoted_after(page, "var urlParams =")
        .or_else(|| quoted_after(page, "urlParams ="))
        .and_then(parse_params);
    let host = origin(iframe_url);
    match (kind, hash, id, params, host) {
        (Some(kind), Some(hash), Some(id), Some(params), Some(host))
            if !kind.is_empty() && !hash.is_empty() && !id.is_empty() =>
        {
            let ctx = PlayerContext {
                host,
                kind: kind.to_owned(),
                id: id.to_owned(),
                hash: hash.to_owned(),
                params,
            };
            if let Some(missing) = REQUIRED_PARAMS.iter().find(|k| ctx.param(k).is_empty()) {
                bail!("{}: urlParams.{missing}", strings::ERR_KODIK_CONTEXT);
            }
            Ok(ctx)
        }
        _ => bail!(strings::ERR_KODIK_CONTEXT),
    }
}

pub fn player_script_path(page: &str) -> Option<&str> {
    let start = page.find("/assets/js/app.player_single.")?;
    let end = page[start..].find(".js")? + start + 3;
    Some(&page[start..end])
}

fn between<'a>(source: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let from = source.find(prefix)? + prefix.len();
    let len = source[from..].find(suffix)?;
    Some(&source[from..from + len])
}

fn decode_b64(value: &str) -> Option<String> {
    let bytes = STANDARD_NO_PAD.decode(value.trim_end_matches('=')).ok()?;
    String::from_utf8(bytes).ok()
}

pub fn endpoint_from_script(script: &str) -> Option<String> {
    between(script, "url:atob(\"", "\")")
        .or_else(|| between(script, "url:atob('", "')"))
        .and_then(decode_b64)
}

fn rot(value: &str, shift: u8) -> String {
    value
        .chars()
        .map(|c| match c {
            'a'..='z' => (b'a' + (c as u8 - b'a' + shift) % 26) as char,
            'A'..='Z' => (b'A' + (c as u8 - b'A' + shift) % 26) as char,
            _ => c,
        })
        .collect()
}

pub fn decode_source(src: &str) -> Option<String> {
    if src.starts_with("http://") || src.starts_with("https://") {
        return Some(src.to_owned());
    }
    (0..26).find_map(|shift| {
        decode_b64(&rot(src, shift)).filter(|d| {
            d.starts_with("http://")
                || d.starts_with("https://")
                || d.starts_with("//")
                || d.contains("mp4:hls:manifest")
        })
    })
}

fn quality_of(key: &str) -> u32 {
    key.trim_end_matches(['p', 'P']).parse().unwrap_or(0)
}

pub fn parse_links(response: &Value) -> Vec<Variant> {
    let Some(links) = response.get("links").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut variants: Vec<Variant> = links
        .iter()
        .filter_map(|(key, sources)| {
            let src = sources.as_array()?.first()?.get("src")?.as_str()?;
            Some(Variant {
                quality: quality_of(key),
                url: normalize_url(&decode_source(src)?),
            })
        })
        .collect();
    variants.sort_by_key(|v| std::cmp::Reverse(v.quality));
    variants
}

fn is_subtitle_url(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    SUBTITLE_EXTS.iter().any(|ext| path.ends_with(ext))
}

fn push_subtitle(out: &mut Vec<Subtitle>, url: &str, label: Option<&str>) {
    let url = normalize_url(&url.replace("\\/", "/"));
    if is_subtitle_url(&url) && !out.iter().any(|s| s.url == url) {
        out.push(Subtitle {
            url,
            label: label.filter(|l| !l.is_empty()).map(str::to_owned),
        });
    }
}

fn collect_json_subtitles(value: &Value, out: &mut Vec<Subtitle>) {
    match value {
        Value::String(s) => push_subtitle(out, s, None),
        Value::Array(items) => items.iter().for_each(|v| collect_json_subtitles(v, out)),
        Value::Object(map) => {
            let url = ["src", "url", "file", "link"]
                .iter()
                .find_map(|k| map.get(*k)?.as_str());
            let label = ["label", "lang", "title", "name"]
                .iter()
                .find_map(|k| map.get(*k)?.as_str());
            match url {
                Some(url) => push_subtitle(out, url, label),
                None => map.values().for_each(|v| collect_json_subtitles(v, out)),
            }
        }
        _ => {}
    }
}

pub fn extract_subtitles(response: &Value, page: &str) -> Vec<Subtitle> {
    let mut out = Vec::new();
    if let Some(map) = response.as_object() {
        for (key, value) in map {
            let key = key.to_ascii_lowercase();
            if key.contains("subtitle") || key == "tracks" || key == "captions" {
                collect_json_subtitles(value, &mut out);
            }
        }
    }
    for token in page.split(['"', '\'', ' ', '<', '>', '(', ')']) {
        if (token.starts_with("http") || token.starts_with("//")) && is_subtitle_url(token) {
            push_subtitle(&mut out, token, None);
        }
    }
    out
}

pub struct Kodik<'a> {
    client: &'a Client,
}

impl<'a> Kodik<'a> {
    pub fn new(client: &'a Client) -> Self {
        Self { client }
    }

    fn get(&self, url: &str, referer: &str) -> Result<String> {
        Ok(self
            .client
            .get(url)
            .header(USER_AGENT, UA)
            .header(REFERER, referer)
            .send()?
            .error_for_status()?
            .text()?)
    }

    fn endpoint(&self, page: &str, iframe: &str) -> String {
        let Some(path) = player_script_path(page) else {
            return DEFAULT_ENDPOINT.into();
        };
        let url = if path.starts_with("http") {
            path.to_owned()
        } else {
            format!("{}{path}", origin(iframe).unwrap_or_default())
        };
        self.get(&url, iframe)
            .ok()
            .and_then(|s| endpoint_from_script(&s))
            .unwrap_or_else(|| DEFAULT_ENDPOINT.into())
    }

    fn request_links(&self, ctx: &PlayerContext, endpoint: &str, iframe: &str) -> Result<Value> {
        let form = ctx.form();
        let mut last_err = anyhow!(strings::ERR_KODIK_LINKS);
        for host in ctx.candidate_hosts() {
            let url = if endpoint.starts_with("http") {
                endpoint.to_owned()
            } else if endpoint.starts_with('/') {
                format!("{host}{endpoint}")
            } else {
                format!("{host}/{endpoint}")
            };
            let resp = self
                .client
                .post(&url)
                .header(USER_AGENT, UA)
                .header(REFERER, iframe)
                .header("Origin", origin(iframe).unwrap_or_default())
                .header("X-Requested-With", "XMLHttpRequest")
                .header(ACCEPT, "application/json, text/javascript, */*; q=0.01")
                .header(
                    CONTENT_TYPE,
                    "application/x-www-form-urlencoded; charset=UTF-8",
                )
                .form(&form)
                .send()
                .and_then(|r| r.text());
            match resp {
                Ok(body) if body.trim_start().starts_with('{') => match serde_json::from_str(&body)
                {
                    Ok(v) => return Ok(v),
                    Err(e) => last_err = e.into(),
                },
                Ok(_) => last_err = anyhow!("{}: {url}", strings::ERR_KODIK_LINKS),
                Err(e) => last_err = e.into(),
            }
        }
        Err(last_err)
    }

    pub fn streams(&self, iframe_url: &str) -> Result<StreamSet> {
        let iframe = normalize_url(iframe_url);
        let page = self
            .get(&iframe, &iframe)
            .context(strings::ERR_KODIK_CONTEXT)?;
        let ctx = parse_player_context(&page, &iframe)?;
        let endpoint = self.endpoint(&page, &iframe);
        let response = self.request_links(&ctx, &endpoint, &iframe)?;
        let variants = parse_links(&response);
        if variants.is_empty() {
            bail!(strings::ERR_KODIK_LINKS);
        }
        Ok(StreamSet {
            referer: iframe,
            user_agent: UA.to_owned(),
            variants,
            subtitles: extract_subtitles(&response, &page),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../tests/fixtures/kodik_page.html");
    const FTOR: &str = include_str!("../tests/fixtures/kodik_ftor.json");
    const SCRIPT: &str = include_str!("../tests/fixtures/kodik_player.js");
    const IFRAME: &str = "https://kodikplayer.com/season/94759/627b449e71ccb06740b8e976b8f751b5/720p?translations=false&only_episode=true&only_season=true&episode=1";

    #[test]
    fn player_context_from_real_page() {
        let ctx = parse_player_context(PAGE, IFRAME).unwrap();
        assert_eq!(ctx.host, "https://kodikplayer.com");
        assert_eq!(ctx.kind, "seria");
        assert_eq!(ctx.id, "1211476");
        assert_eq!(ctx.hash, "627b449e71ccb06740b8e976b8f751b5");
        let form = ctx.form();
        let reference = &form.iter().find(|(k, _)| *k == "ref").unwrap().1;
        assert!(
            reference.starts_with("https://kodikplayer.com/season/94759/"),
            "{reference}"
        );
        assert_eq!(ctx.candidate_hosts()[0], "https://kodikplayer.com");
        assert!(
            ctx.candidate_hosts()
                .contains(&"https://kodik.info".to_owned())
        );
    }

    #[test]
    fn player_context_rejects_page_without_params() {
        let page = "vInfo.type = 'seria'; vInfo.hash = 'h'; vInfo.id = '1';";
        assert!(parse_player_context(page, IFRAME).is_err());
        let page = r#"vInfo.type = 'seria'; vInfo.hash = 'h'; vInfo.id = '1'; var urlParams = '{"d":"x"}';"#;
        assert!(parse_player_context(page, IFRAME).is_err());
    }

    #[test]
    fn endpoint_decoded_from_player_script() {
        let path = player_script_path(PAGE).unwrap();
        assert!(path.starts_with("/assets/js/app.player_single.") && path.ends_with(".js"));
        assert_eq!(endpoint_from_script(SCRIPT).as_deref(), Some("/ftor"));
        assert_eq!(endpoint_from_script("nothing"), None);
    }

    #[test]
    fn links_decoded_to_hls_urls_sorted_by_quality() {
        let response: Value = serde_json::from_str(FTOR).unwrap();
        let variants = parse_links(&response);
        assert_eq!(
            variants.iter().map(|v| v.quality).collect::<Vec<_>>(),
            [720, 480, 360]
        );
        for v in &variants {
            assert!(
                v.url.starts_with("https://cloud.solodcdn.com/"),
                "{}",
                v.url
            );
            assert!(
                v.url
                    .ends_with(&format!("{}.mp4:hls:manifest.m3u8", v.quality)),
                "{}",
                v.url
            );
        }
    }

    #[test]
    fn source_decoding_handles_plain_and_rotated() {
        assert_eq!(
            decode_source("https://a/b.m3u8").as_deref(),
            Some("https://a/b.m3u8")
        );
        let encoded = STANDARD_NO_PAD.encode("//cdn.example/720.mp4:hls:manifest.m3u8");
        let rotated = rot(&encoded, 26 - 18);
        assert_eq!(
            decode_source(&rotated).as_deref(),
            Some("//cdn.example/720.mp4:hls:manifest.m3u8")
        );
        assert_eq!(decode_source("!!!"), None);
    }

    #[test]
    fn real_kodik_response_has_no_external_subtitles() {
        let response: Value = serde_json::from_str(FTOR).unwrap();
        assert!(extract_subtitles(&response, PAGE).is_empty());
    }

    #[test]
    fn subtitles_extracted_from_response_and_page() {
        let response: Value = serde_json::json!({
            "links": {},
            "subtitles": [
                {"src": "//s.example/ep1.ass", "label": "Russian"},
                "https://s.example/ep1.vtt?sig=1",
                {"src": "https://s.example/poster.jpg"}
            ]
        });
        let page = r#"<track src="https://s.example/ep1.srt"> <a href="//s.example/ep1.ass">"#;
        let subs = extract_subtitles(&response, page);
        assert_eq!(
            subs,
            [
                Subtitle {
                    url: "https://s.example/ep1.ass".into(),
                    label: Some("Russian".into())
                },
                Subtitle {
                    url: "https://s.example/ep1.vtt?sig=1".into(),
                    label: None
                },
                Subtitle {
                    url: "https://s.example/ep1.srt".into(),
                    label: None
                },
            ]
        );
    }
}
