//! Pure tag rules; evaluation never reads files or changes the listening queue.
use crate::library::Track;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tag {
    #[default]
    Genre,
    Artist,
    Album,
    Title,
    Year,
    Rating,
    PlayCount,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparison {
    #[default]
    Contains,
    Equals,
    NotContains,
    NotEquals,
    AtLeast,
    AtMost,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub tag: Tag,
    pub comparison: Comparison,
    pub value: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub all: bool,
    pub conditions: Vec<Rule>,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            all: true,
            conditions: vec![Rule::default()],
        }
    }
}
fn folded(s: &str) -> String {
    s.trim().nfkc().flat_map(char::to_lowercase).collect()
}
impl Rule {
    pub fn valid(&self) -> bool {
        let value = self.value.trim();
        if value.is_empty() || value.chars().count() > 256 || value.chars().any(char::is_control) {
            return false;
        }
        match self.tag {
            Tag::Rating | Tag::PlayCount => {
                matches!(
                    self.comparison,
                    Comparison::Equals | Comparison::AtLeast | Comparison::AtMost
                ) && value
                    .parse::<u64>()
                    .is_ok_and(|n| self.tag != Tag::Rating || n <= 5)
            }
            Tag::Year => {
                matches!(
                    self.comparison,
                    Comparison::Equals | Comparison::AtLeast | Comparison::AtMost
                ) && value.parse::<u32>().is_ok_and(|y| (1..=9999).contains(&y))
            }
            _ => matches!(
                self.comparison,
                Comparison::Contains
                    | Comparison::Equals
                    | Comparison::NotContains
                    | Comparison::NotEquals
            ),
        }
    }
}
impl Rules {
    pub fn valid(&self) -> bool {
        !self.conditions.is_empty()
            && self.conditions.len() <= 24
            && self.conditions.iter().all(Rule::valid)
    }
    #[cfg(test)]
    pub fn select(&self, library: &[Track]) -> Vec<String> {
        self.select_with_stats(library, &Default::default())
    }
    pub fn uses_stats(&self) -> bool {
        self.conditions
            .iter()
            .any(|r| matches!(r.tag, Tag::Rating | Tag::PlayCount))
    }
    #[cfg(test)]
    pub fn select_with_stats(
        &self,
        library: &[Track],
        stats: &crate::track_stats::Stats,
    ) -> Vec<String> {
        self.select_with_scale(library, stats, musiccraft_tags::Scale::Explicit)
    }
    pub fn select_with_scale(
        &self,
        library: &[Track],
        stats: &crate::track_stats::Stats,
        scale: musiccraft_tags::Scale,
    ) -> Vec<String> {
        if !self.valid() {
            return vec![];
        }
        let prepared: Vec<_> = self
            .conditions
            .iter()
            .map(|r| (r, folded(&r.value)))
            .collect();
        let matches = |t: &Track, rule: &Rule, value: &str| {
            use Comparison::*;
            if matches!(rule.tag, Tag::Rating | Tag::PlayCount) {
                let entry = stats.effective(t, scale);
                let n = if rule.tag == Tag::Rating {
                    let Some(rating) = stats.rating_for(t, scale) else {
                        return false;
                    };
                    u64::from(rating)
                } else {
                    entry.plays
                };
                let value: u64 = value.parse().unwrap();
                return match rule.comparison {
                    Equals => n == value,
                    AtLeast => n >= value,
                    AtMost => n <= value,
                    _ => false,
                };
            }
            if rule.tag == Tag::Year {
                let Some(year) = t.year.filter(|y| (1..=9999).contains(y)) else {
                    return false;
                };
                let n: u32 = value.parse().unwrap();
                return match rule.comparison {
                    Equals => year == n,
                    AtLeast => year >= n,
                    AtMost => year <= n,
                    _ => false,
                };
            }
            let raw = match rule.tag {
                Tag::Genre => &t.genre,
                Tag::Artist => &t.artist,
                Tag::Album => &t.album,
                Tag::Title => &t.title,
                Tag::Year | Tag::Rating | Tag::PlayCount => unreachable!(),
            };
            if raw.trim().is_empty() {
                return false;
            }
            let text = folded(raw);
            let equal = if rule.tag == Tag::Genre {
                text.split(';').any(|s| s.trim() == value)
            } else {
                text == value
            };
            match rule.comparison {
                Contains => text.contains(value),
                Equals => equal,
                NotContains => !text.contains(value),
                NotEquals => !equal,
                _ => false,
            }
        };
        let mut seen = HashSet::new();
        library
            .iter()
            .filter(|t| {
                if self.all {
                    prepared.iter().all(|(r, v)| matches(t, r, v))
                } else {
                    prepared.iter().any(|(r, v)| matches(t, r, v))
                }
            })
            .filter(|t| seen.insert(t.source.as_str()))
            .map(|t| t.source.clone())
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn rule(tag: Tag, comparison: Comparison, value: &str) -> Rule {
        Rule {
            tag,
            comparison,
            value: value.into(),
        }
    }
    #[test]
    fn combined_tags_year_bounds_multigenres_unicode_and_missing_values() {
        let mut a = Track::placeholder("/a.flac".into());
        a.artist = "QUEEN".into();
        a.genre = "Rock; Pop".into();
        a.year = Some(1980);
        let mut b = a.clone();
        b.source = "/b.flac".into();
        b.year = Some(1990);
        let mut c = a.clone();
        c.source = "/c.flac".into();
        c.year = None;
        let mut d = a.clone();
        d.source = "/d.flac".into();
        d.artist = "Other".into();
        d.genre = String::new();
        let lib = vec![a.clone(), b, c, d];
        let mut r = Rules {
            all: true,
            conditions: vec![
                rule(Tag::Genre, Comparison::Equals, " pop "),
                rule(Tag::Artist, Comparison::Equals, "Queen"),
                rule(Tag::Year, Comparison::AtLeast, "1980"),
                rule(Tag::Year, Comparison::AtMost, "1989"),
            ],
        };
        assert_eq!(r.select(&lib), vec!["/a.flac"]);
        r.all = false;
        assert_eq!(r.select(&lib).len(), 4);
        r.conditions = vec![rule(Tag::Genre, Comparison::NotContains, "Rock")];
        assert!(
            r.select(&lib).is_empty(),
            "missing tags never satisfy exclusions"
        );
        a.title = "Café".into();
        r.conditions = vec![rule(Tag::Title, Comparison::Equals, "Cafe\u{301}")];
        assert_eq!(r.select(&[a.clone(), a]).len(), 1);
    }
    #[test]
    fn invalid_rules_never_match_everything_accidentally() {
        for r in [
            Rules::default(),
            Rules {
                all: true,
                conditions: vec![],
            },
            Rules {
                all: false,
                conditions: vec![rule(Tag::Year, Comparison::AtLeast, "19xx")],
            },
            Rules {
                all: true,
                conditions: vec![rule(Tag::Year, Comparison::Contains, "1980")],
            },
            Rules {
                all: true,
                conditions: vec![rule(Tag::Artist, Comparison::Equals, &"a".repeat(257))],
            },
        ] {
            assert!(!r.valid());
            assert!(r.select(&[Track::placeholder("/a.wav".into())]).is_empty());
        }
    }
}

#[cfg(test)]
mod stats_tests {
    use super::*;
    #[test]
    fn rating_and_play_count_criteria_include_zero_and_refresh_after_another_play() {
        let lib: Vec<_> = ["/a.wav", "/b.wav"]
            .into_iter()
            .map(|s| Track::placeholder(s.into()))
            .collect();
        let mut stats = crate::track_stats::Stats::default();
        stats.rate("/a.wav", 4, 1);
        let make = |tag, comparison, value: &str| Rule {
            tag,
            comparison,
            value: value.into(),
        };
        let rules = Rules {
            all: true,
            conditions: vec![
                make(Tag::Rating, Comparison::AtLeast, "4"),
                make(Tag::PlayCount, Comparison::AtMost, "0"),
            ],
        };
        assert_eq!(rules.select_with_stats(&lib, &stats), vec!["/a.wav"]);
        stats.played("/a.wav");
        assert!(rules.select_with_stats(&lib, &stats).is_empty());
        let unheard = Rules {
            all: true,
            conditions: vec![make(Tag::PlayCount, Comparison::Equals, "0")],
        };
        assert_eq!(unheard.select_with_stats(&lib, &stats), vec!["/b.wav"]);
        assert!(!make(Tag::Rating, Comparison::Equals, "6").valid());
        assert!(!make(Tag::PlayCount, Comparison::AtLeast, "-1").valid());
    }
}
