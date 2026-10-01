use crate::library::WatchStatus;

pub const APP_NAME: &str = "yatui";
pub const ABOUT: &str = "Watch anime from YummyAnime in your terminal";

pub const HELP_SEARCH: &str = "Search titles by name";
pub const HELP_INFO: &str = "Show translations and episodes of a title";
pub const HELP_PLAY: &str = "Play an episode in the player";
pub const HELP_LIST: &str = "Show local watch lists";
pub const HELP_STATUS: &str = "Set or clear the list status of a title";
pub const HELP_QUERY: &str = "Title name to search for";
pub const HELP_INDEX: &str = "Which search result to use (1-based)";
pub const HELP_EPISODE: &str = "Episode number (default: next unwatched)";
pub const HELP_TRANSLATION: &str = "Translation name or part of it (default: first available)";
pub const HELP_QUALITY: &str = "Maximum video quality, e.g. 720 (default: best)";
pub const HELP_PRINT_CMD: &str = "Print the player command instead of running it";
pub const HELP_LIST_STATUS: &str = "Only show this list";
pub const HELP_SLUG: &str = "Title slug as shown by `search`";
pub const HELP_NEW_STATUS: &str = "New status; omit to remove the title from lists";
pub const HELP_TOKEN: &str = "Manage YummyAnime API tokens";
pub const HELP_TOKEN_SET: &str =
    "Save your private user token (sent as `Authorization: Yummy <token>`)";
pub const HELP_TOKEN_APP: &str =
    "Save your own application token instead of the built-in public one";
pub const HELP_TOKEN_STATUS: &str = "Show which tokens are used and verify the private token";
pub const HELP_TOKEN_REFRESH: &str = "Refresh the private token via /profile/token and save it";
pub const HELP_TOKEN_CLEAR: &str = "Remove saved tokens and fall back to the public token";
pub const HELP_TOKEN_VALUE: &str = "Token value";

pub const ERR_NO_RESULTS: &str = "Nothing found";
pub const ERR_NO_TRANSLATIONS: &str = "No supported (Kodik) translations for this title";
pub const ERR_NO_STREAMS: &str = "No playable streams found";
pub const ERR_KODIK_CONTEXT: &str = "Could not parse Kodik player page";
pub const ERR_KODIK_LINKS: &str = "Kodik did not return stream links";
pub const ERR_BAD_RESPONSE: &str = "Unexpected API response";
pub const ERR_PLAYER_FAILED: &str = "Player exited with an error";
pub const ERR_NO_USER_TOKEN: &str = "No private token set; run `yatui token set <TOKEN>`";

pub fn err_result_index(n: usize, total: usize) -> String {
    format!("Result #{n} does not exist ({total} results)")
}
pub fn err_no_translation(query: &str) -> String {
    format!("No translation matching \"{query}\"")
}
pub fn err_no_episode(ep: &str) -> String {
    format!("Episode {ep} is not available in this translation")
}
pub fn err_spawn_player(player: &str, err: &str) -> String {
    format!("Failed to start {player}: {err}")
}

pub const EPISODES: &str = "Episodes";
pub const TRANSLATIONS: &str = "Translations";
pub const QUALITY: &str = "Quality";
pub const SEARCH: &str = "Search";
pub const LIBRARY: &str = "Library";
pub const RESULTS: &str = "Results";
pub const DETAILS: &str = "Details";
pub const NOT_IN_LIST: &str = "Not in a list";
pub const LIST_EMPTY: &str = "Lists are empty";
pub const LOADING: &str = "Loading...";
pub const SEARCH_PROMPT: &str = "Type a title and press Enter";
pub const NO_SUBTITLES: &str = "no external subtitles";

pub fn status_label(s: WatchStatus) -> &'static str {
    match s {
        WatchStatus::Watching => "Watching",
        WatchStatus::Planned => "Plan to Watch",
        WatchStatus::OnHold => "On Hold",
        WatchStatus::Dropped => "Dropped",
        WatchStatus::Completed => "Completed",
    }
}

pub fn episode_label(number: &str) -> String {
    format!("Episode {number}")
}
pub fn episode_count(n: usize) -> String {
    if n == 1 {
        "1 episode".into()
    } else {
        format!("{n} episodes")
    }
}
pub fn quality_label(q: u32) -> String {
    format!("{q}p")
}
pub fn year_label(y: u32) -> String {
    format!("Year: {y}")
}
pub fn type_label(t: &str) -> String {
    format!("Type: {t}")
}
pub fn episodes_progress(aired: Option<u32>, total: Option<u32>) -> String {
    match (aired, total) {
        (Some(a), Some(t)) if t > 0 => format!("Episodes: {a}/{t}"),
        (Some(a), _) => format!("Episodes: {a}/?"),
        (None, Some(t)) => format!("Episodes: {t}"),
        (None, None) => "Episodes: ?".into(),
    }
}
pub fn also_known_as(names: &str) -> String {
    format!("Also known as: {names}")
}
pub fn list_status(s: Option<WatchStatus>) -> String {
    format!("List: {}", s.map_or(NOT_IN_LIST, status_label))
}
pub fn watched_progress(watched: usize, total: Option<u32>) -> String {
    match total {
        Some(t) if t > 0 => format!("{watched}/{t}"),
        _ => format!("{watched}/?"),
    }
}
pub fn subtitles_count(n: usize) -> String {
    format!("{n} subtitle track(s)")
}
pub fn status_set(title: &str, s: Option<WatchStatus>) -> String {
    match s {
        Some(s) => format!("\"{title}\" moved to {}", status_label(s)),
        None => format!("\"{title}\" removed from lists"),
    }
}
pub fn media_title(title: &str, ep: &str) -> String {
    format!("{title} — {}", episode_label(ep))
}
pub fn now_playing(title: &str, ep: &str) -> String {
    format!("Playing {title} — {}", episode_label(ep))
}
pub fn marked_watched(ep: &str) -> String {
    format!("{} marked as watched", episode_label(ep))
}
pub fn marked_unwatched(ep: &str) -> String {
    format!("{} marked as unwatched", episode_label(ep))
}

pub const TOKEN_SAVED: &str = "Token saved";
pub const TOKEN_CLEARED: &str = "Saved tokens removed";
pub const TOKEN_REFRESHED: &str = "Private token refreshed and saved";
pub const TOKEN_PUBLIC: &str = "built-in public token";
pub const TOKEN_NONE: &str = "not set";
pub fn token_app_line(value: &str) -> String {
    format!("Application token: {value}")
}
pub fn token_user_line(value: &str) -> String {
    format!("Private token: {value}")
}
pub fn token_logged_in(nickname: &str) -> String {
    format!("Authorized as {nickname}")
}
pub fn token_env_override(var: &str) -> String {
    format!("Note: {var} is set and overrides the saved value")
}
pub fn config_path(path: &str) -> String {
    format!("Config: {path}")
}

pub const HINT_SEARCH_EDIT: &str = "Enter search · Esc results · Tab library · Ctrl-C quit";
pub const HINT_SEARCH: &str = "Enter open · / search · Tab library · q quit";
pub const HINT_TITLE: &str = "Enter episodes · 1-5 set list · x remove · Esc back · q quit";
pub const HINT_EPISODES: &str = "Enter play · w toggle watched · Esc back · q quit";
pub const HINT_QUALITY: &str = "Enter play · Esc back · q quit";
pub const HINT_LIBRARY: &str = "←/→ list · Enter open · Tab search · q quit";
