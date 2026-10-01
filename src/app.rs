use anyhow::{Result, anyhow};

use crate::library::{Entry, Library};
use crate::player::{PlayRequest, Player};
use crate::source::{Episode, Source, StreamSet, Title, TitleDetails, Translation, pick_variant};
use crate::strings;

pub fn next_episode<'a>(
    translation: &'a Translation,
    entry: Option<&Entry>,
) -> Option<&'a Episode> {
    translation
        .episodes
        .iter()
        .find(|e| entry.is_none_or(|en| !en.is_watched(&e.number)))
        .or_else(|| translation.episodes.first())
}

pub fn find_episode<'a>(translation: &'a Translation, number: &str) -> Result<&'a Episode> {
    translation
        .episodes
        .iter()
        .find(|e| e.number == number)
        .ok_or_else(|| anyhow!(strings::err_no_episode(number)))
}

pub fn build_request(
    title: &Title,
    episode: &Episode,
    streams: &StreamSet,
    variant: usize,
) -> Result<PlayRequest> {
    let variant = streams
        .variants
        .get(variant)
        .ok_or_else(|| anyhow!(strings::ERR_NO_STREAMS))?;
    Ok(PlayRequest {
        title: strings::media_title(&title.name, &episode.number),
        variant: variant.clone(),
        subtitles: streams.subtitles.clone(),
        referer: streams.referer.clone(),
        user_agent: streams.user_agent.clone(),
    })
}

pub fn resolve(
    source: &dyn Source,
    title: &Title,
    episode: &Episode,
    quality: Option<u32>,
) -> Result<PlayRequest> {
    let streams = source.streams(episode)?;
    let idx =
        pick_variant(&streams.variants, quality).ok_or_else(|| anyhow!(strings::ERR_NO_STREAMS))?;
    build_request(title, episode, &streams, idx)
}

pub fn play_and_record(
    player: &dyn Player,
    req: &PlayRequest,
    library: &mut Library,
    details: &TitleDetails,
    translation: &Translation,
    episode: &Episode,
) -> Result<()> {
    player.play(req)?;
    library.mark_watched(
        &details.title.slug,
        &details.title.name,
        &episode.number,
        details.title.episodes_total,
        &translation.name,
    );
    library.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(eps: &[&str]) -> Translation {
        Translation {
            name: "t".into(),
            episodes: eps
                .iter()
                .map(|n| Episode {
                    number: (*n).into(),
                    url: format!("u{n}"),
                })
                .collect(),
        }
    }

    #[test]
    fn next_episode_skips_watched_and_wraps_to_first() {
        let t = tr(&["1", "2", "3"]);
        assert_eq!(next_episode(&t, None).unwrap().number, "1");
        let mut e = Entry {
            watched: vec!["1".into(), "3".into()],
            ..Default::default()
        };
        assert_eq!(next_episode(&t, Some(&e)).unwrap().number, "2");
        e.watched.push("2".into());
        assert_eq!(next_episode(&t, Some(&e)).unwrap().number, "1");
        assert!(next_episode(&tr(&[]), None).is_none());
    }

    #[test]
    fn find_episode_reports_missing_number() {
        let t = tr(&["1", "2"]);
        assert_eq!(find_episode(&t, "2").unwrap().url, "u2");
        assert!(find_episode(&t, "7").unwrap_err().to_string().contains('7'));
    }
}
