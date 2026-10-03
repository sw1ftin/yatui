use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Title {
    pub slug: String,
    pub name: String,
    pub year: Option<u32>,
    pub kind: Option<String>,
    pub other_names: Vec<String>,
    pub description: Option<String>,
    pub episodes_total: Option<u32>,
    pub episodes_aired: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub number: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Translation {
    pub name: String,
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TitleDetails {
    pub title: Title,
    pub translations: Vec<Translation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub quality: u32,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Subtitle {
    pub url: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamSet {
    pub referer: String,
    pub user_agent: String,
    pub variants: Vec<Variant>,
    pub subtitles: Vec<Subtitle>,
}

pub trait Source: Send + Sync {
    fn search(&self, query: &str) -> Result<Vec<Title>>;
    fn details(&self, slug: &str) -> Result<TitleDetails>;
    fn streams(&self, episode: &Episode) -> Result<StreamSet>;
}

pub fn episode_sort_key(number: &str) -> f64 {
    let start = number.find(|c: char| c.is_ascii_digit());
    let Some(start) = start else { return f64::MAX };
    let rest = &number[start..];
    let end = rest
        .char_indices()
        .scan(false, |dot, (i, c)| {
            if c.is_ascii_digit() {
                Some(i + 1)
            } else if c == '.' && !*dot {
                *dot = true;
                Some(i)
            } else {
                None
            }
        })
        .last()
        .unwrap_or(0);
    rest[..end].parse().unwrap_or(f64::MAX)
}

fn normalize_translation(name: &str) -> String {
    name.to_lowercase()
        .replace('ё', "е")
        .replace("озвучка", "")
        .replace("субтитры", "")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub fn pick_translation(translations: &[Translation], wanted: Option<&str>) -> Option<usize> {
    let Some(wanted) = wanted else {
        return (!translations.is_empty()).then_some(0);
    };
    if let Some(i) = translations.iter().position(|t| t.name == wanted) {
        return Some(i);
    }
    let wanted = normalize_translation(wanted);
    if wanted.is_empty() {
        return None;
    }
    translations
        .iter()
        .position(|t| normalize_translation(&t.name) == wanted)
        .or_else(|| {
            translations
                .iter()
                .position(|t| normalize_translation(&t.name).contains(&wanted))
        })
}

pub fn pick_variant(variants: &[Variant], max_quality: Option<u32>) -> Option<usize> {
    let best = |it: &mut dyn Iterator<Item = (usize, &Variant)>| {
        it.max_by_key(|(_, v)| v.quality).map(|(i, _)| i)
    };
    match max_quality {
        None => best(&mut variants.iter().enumerate()),
        Some(cap) => best(
            &mut variants
                .iter()
                .enumerate()
                .filter(|(_, v)| v.quality <= cap),
        )
        .or_else(|| {
            variants
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| v.quality)
                .map(|(i, _)| i)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(name: &str) -> Translation {
        Translation {
            name: name.into(),
            episodes: vec![],
        }
    }

    fn v(q: u32) -> Variant {
        Variant {
            quality: q,
            url: format!("u{q}"),
        }
    }

    #[test]
    fn sort_key_handles_fractional_and_text_numbers() {
        assert_eq!(episode_sort_key("12"), 12.0);
        assert_eq!(episode_sort_key("6.5"), 6.5);
        assert_eq!(episode_sort_key("OVA 3"), 3.0);
        assert_eq!(episode_sort_key("Фильм"), f64::MAX);
    }

    #[test]
    fn translation_matching_prefers_exact_then_normalized_then_partial() {
        let list = [
            tr("Озвучка AniDUB Online"),
            tr("Озвучка AniDUB"),
            tr("Субтитры Crunchyroll"),
        ];
        assert_eq!(pick_translation(&list, None), Some(0));
        assert_eq!(pick_translation(&list, Some("anidub")), Some(1));
        assert_eq!(
            pick_translation(&list, Some("Озвучка AniDUB Online")),
            Some(0)
        );
        assert_eq!(pick_translation(&list, Some("crunchy")), Some(2));
        assert_eq!(pick_translation(&list, Some("shiza")), None);
        assert_eq!(pick_translation(&[], None), None);
    }

    #[test]
    fn variant_cap_picks_best_below_cap_or_lowest_when_none_fit() {
        let list = [v(360), v(720), v(480)];
        assert_eq!(pick_variant(&list, None), Some(1));
        assert_eq!(pick_variant(&list, Some(480)), Some(2));
        assert_eq!(pick_variant(&list, Some(1080)), Some(1));
        assert_eq!(pick_variant(&list, Some(240)), Some(0));
        assert_eq!(pick_variant(&[], Some(720)), None);
    }
}
