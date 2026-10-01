use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum WatchStatus {
    Watching,
    Planned,
    OnHold,
    Dropped,
    Completed,
}

impl WatchStatus {
    pub const ALL: [WatchStatus; 5] = [
        Self::Watching,
        Self::Planned,
        Self::OnHold,
        Self::Dropped,
        Self::Completed,
    ];
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<WatchStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_episodes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watched: Vec<String>,
}

impl Entry {
    pub fn is_watched(&self, episode: &str) -> bool {
        self.watched.iter().any(|e| e == episode)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    titles: BTreeMap<String, Entry>,
}

#[derive(Debug)]
pub struct Library {
    path: PathBuf,
    data: Data,
}

impl Library {
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("yatui").join("library.json"))
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let data = match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).with_context(|| path.display().to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(e) => return Err(e).with_context(|| path.display().to_string()),
        };
        Ok(Self { path, data })
    }

    #[cfg(test)]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.data)?)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn get(&self, slug: &str) -> Option<&Entry> {
        self.data.titles.get(slug)
    }

    pub fn status(&self, slug: &str) -> Option<WatchStatus> {
        self.get(slug).and_then(|e| e.status)
    }

    fn entry(&mut self, slug: &str, name: &str) -> &mut Entry {
        let e = self.data.titles.entry(slug.to_owned()).or_default();
        if !name.is_empty() {
            e.name = name.to_owned();
        }
        e
    }

    fn prune(&mut self, slug: &str) {
        if self
            .data
            .titles
            .get(slug)
            .is_some_and(|e| e.status.is_none() && e.watched.is_empty())
        {
            self.data.titles.remove(slug);
        }
    }

    pub fn set_status(&mut self, slug: &str, name: &str, status: Option<WatchStatus>) {
        self.entry(slug, name).status = status;
        self.prune(slug);
    }

    pub fn set_total(&mut self, slug: &str, total: Option<u32>) {
        if let Some(e) = self.data.titles.get_mut(slug)
            && total.is_some()
        {
            e.total_episodes = total;
        }
    }

    pub fn mark_watched(
        &mut self,
        slug: &str,
        name: &str,
        episode: &str,
        total: Option<u32>,
        translation: &str,
    ) {
        let e = self.entry(slug, name);
        if !e.is_watched(episode) {
            e.watched.push(episode.to_owned());
        }
        if total.is_some() {
            e.total_episodes = total;
        }
        e.translation = Some(translation.to_owned());
        let finished = e
            .total_episodes
            .is_some_and(|t| e.watched.len() >= t as usize);
        e.status = match (e.status, finished) {
            (_, true) => Some(WatchStatus::Completed),
            (
                None | Some(WatchStatus::Planned | WatchStatus::OnHold | WatchStatus::Dropped),
                false,
            ) => Some(WatchStatus::Watching),
            (s, false) => s,
        };
    }

    pub fn unmark_watched(&mut self, slug: &str, episode: &str) {
        if let Some(e) = self.data.titles.get_mut(slug) {
            e.watched.retain(|w| w != episode);
            if e.status == Some(WatchStatus::Completed) {
                e.status = Some(WatchStatus::Watching);
            }
        }
        self.prune(slug);
    }

    pub fn by_status(&self, status: WatchStatus) -> Vec<(&str, &Entry)> {
        let mut out: Vec<_> = self
            .data
            .titles
            .iter()
            .filter(|(_, e)| e.status == Some(status))
            .map(|(k, e)| (k.as_str(), e))
            .collect();
        out.sort_by_key(|a| a.1.name.to_lowercase());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let l = Library::open(dir.path().join("nested/library.json")).unwrap();
        (dir, l)
    }

    #[test]
    fn missing_file_is_empty_and_save_roundtrips() {
        let (_d, mut l) = lib();
        assert!(l.by_status(WatchStatus::Watching).is_empty());
        l.set_status("a", "Alpha", Some(WatchStatus::Planned));
        l.mark_watched("b", "Beta", "1", Some(12), "AniDUB");
        l.save().unwrap();
        let r = Library::open(l.path()).unwrap();
        assert_eq!(r.status("a"), Some(WatchStatus::Planned));
        let b = r.get("b").unwrap();
        assert_eq!(
            (b.status, b.watched.as_slice(), b.total_episodes),
            (Some(WatchStatus::Watching), &["1".to_owned()][..], Some(12))
        );
        assert_eq!(b.translation.as_deref(), Some("AniDUB"));
    }

    #[test]
    fn corrupt_file_is_an_error_not_silently_reset() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("library.json");
        fs::write(&p, "{not json").unwrap();
        assert!(Library::open(&p).is_err());
    }

    #[test]
    fn watching_episode_moves_planned_on_hold_dropped_to_watching() {
        for start in [
            WatchStatus::Planned,
            WatchStatus::OnHold,
            WatchStatus::Dropped,
        ] {
            let (_d, mut l) = lib();
            l.set_status("a", "A", Some(start));
            l.mark_watched("a", "A", "1", Some(3), "t");
            assert_eq!(l.status("a"), Some(WatchStatus::Watching), "{start:?}");
        }
    }

    #[test]
    fn last_episode_completes_and_unwatch_reopens() {
        let (_d, mut l) = lib();
        l.mark_watched("a", "A", "1", Some(2), "t");
        l.mark_watched("a", "A", "1", Some(2), "t");
        assert_eq!(l.status("a"), Some(WatchStatus::Watching));
        l.mark_watched("a", "A", "2", Some(2), "t");
        assert_eq!(l.status("a"), Some(WatchStatus::Completed));
        l.unmark_watched("a", "2");
        assert_eq!(l.status("a"), Some(WatchStatus::Watching));
        assert_eq!(l.get("a").unwrap().watched, ["1"]);
    }

    #[test]
    fn unknown_total_never_auto_completes() {
        let (_d, mut l) = lib();
        for ep in ["1", "2", "3"] {
            l.mark_watched("a", "A", ep, None, "t");
        }
        assert_eq!(l.status("a"), Some(WatchStatus::Watching));
    }

    #[test]
    fn removing_status_keeps_progress_but_drops_empty_entries() {
        let (_d, mut l) = lib();
        l.mark_watched("a", "A", "1", None, "t");
        l.set_status("a", "A", None);
        assert_eq!(l.get("a").unwrap().watched, ["1"]);
        l.set_status("b", "B", Some(WatchStatus::Dropped));
        l.set_status("b", "B", None);
        assert!(l.get("b").is_none());
    }

    #[test]
    fn lists_are_filtered_and_sorted_by_name() {
        let (_d, mut l) = lib();
        l.set_status("z", "beta", Some(WatchStatus::Planned));
        l.set_status("y", "Alpha", Some(WatchStatus::Planned));
        l.set_status("x", "Gamma", Some(WatchStatus::Dropped));
        let names: Vec<_> = l
            .by_status(WatchStatus::Planned)
            .iter()
            .map(|(_, e)| e.name.as_str())
            .collect();
        assert_eq!(names, ["Alpha", "beta"]);
    }
}
