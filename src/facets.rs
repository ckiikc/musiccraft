//! A cached, source-scoped faceted browser. It never reads audio or network data.
use crate::library::{Scope, Track};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Field {
    Genre,
    Artist,
    Album,
    Year,
    Format,
    Folder,
}
impl Field {
    pub const ALL: [Self; 6] = [
        Self::Genre,
        Self::Artist,
        Self::Album,
        Self::Year,
        Self::Format,
        Self::Folder,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Genre => tr!("Genre"),
            Self::Artist => tr!("Artist"),
            Self::Album => tr!("Album"),
            Self::Year => tr!("Year"),
            Self::Format => tr!("Format"),
            Self::Folder => tr!("Folder"),
        }
    }
    fn value(self, t: &Track) -> (String, String, String) {
        let raw = match self {
            Self::Genre => t.genre.trim().to_owned(),
            Self::Artist => t.artist.trim().to_owned(),
            Self::Album => t.album.trim().to_owned(),
            Self::Year => t.year.map(|y| y.to_string()).unwrap_or_default(),
            Self::Format => t.codec.to_uppercase(),
            Self::Folder => {
                let path = if let Ok(mut u) = url::Url::parse(&t.source) {
                    u.set_fragment(None);
                    if let Ok(mut segments) = u.path_segments_mut() {
                        segments.pop();
                    }
                    crate::library::source_key(u.as_str())
                } else {
                    std::path::Path::new(&t.source)
                        .parent()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default()
                };
                let label = path
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or_default();
                return (
                    path.clone(),
                    percent_encoding::percent_decode_str(label)
                        .decode_utf8_lossy()
                        .into_owned(),
                    path,
                );
            }
        };
        let detail = if self == Self::Album {
            if t.album_artist.is_empty() {
                t.artist.clone()
            } else {
                t.album_artist.clone()
            }
        } else {
            String::new()
        };
        let key = if self == Self::Album && !raw.is_empty() {
            format!("{}\0{}", detail.trim().to_lowercase(), raw.to_lowercase())
        } else {
            raw.to_lowercase()
        };
        (
            key,
            if raw.is_empty() {
                tr!("Unavailable").into()
            } else {
                raw
            },
            detail,
        )
    }
    fn values(self, t: &Track) -> Vec<(String, String, String)> {
        if self == Self::Genre && t.genre.contains(';') {
            let mut seen = BTreeSet::new();
            t.genre
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter_map(|s| {
                    let key = s.to_lowercase();
                    seen.insert(key.clone())
                        .then(|| (key, s.to_owned(), String::new()))
                })
                .collect()
        } else {
            vec![self.value(t)]
        }
    }
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub key: String,
    pub label: String,
    pub detail: String,
    pub count: usize,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
    Original,
    #[default]
    Album,
    Title,
    Artist,
    Duration,
}
impl Sort {
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => tr!("Original order"),
            Self::Album => tr!("Album order"),
            Self::Title => tr!("Title"),
            Self::Artist => tr!("Artist"),
            Self::Duration => tr!("Duration"),
        }
    }
}
type CacheKey = (
    u64,
    usize,
    Option<String>,
    String,
    [Field; 3],
    [BTreeSet<String>; 3],
    Sort,
    [String; 3],
);
#[derive(Default)]
pub struct Browser {
    pub selected: [BTreeSet<String>; 3],
    pub entries: [Vec<Entry>; 3],
    pub totals: [usize; 3],
    pub searches: [String; 3],
    pub indices: Vec<usize>,
    pub sort: Sort,
    pub selected_track: Option<String>,
    pub reset_scroll: bool,
    anchors: [Option<String>; 3],
    key: Option<CacheKey>,
    pub rebuilds: usize,
}
impl Browser {
    pub fn clear(&mut self) {
        self.selected = Default::default();
        self.searches = Default::default();
        self.anchors = Default::default();
        self.key = None;
    }
    pub fn active(&self) -> bool {
        self.selected.iter().any(|s| !s.is_empty())
            || self.searches.iter().any(|s| !s.trim().is_empty())
    }
    pub fn select(&mut self, col: usize, key: &str, add: bool, range: bool) {
        let Some(index) = self.entries[col].iter().position(|e| e.key == key) else {
            return;
        };
        if let Some(a) = self.anchors[col]
            .as_ref()
            .filter(|_| range)
            .and_then(|anchor| self.entries[col].iter().position(|e| &e.key == anchor))
        {
            if !add {
                self.selected[col].clear();
            }
            for e in &self.entries[col][a.min(index)..=a.max(index)] {
                self.selected[col].insert(e.key.clone());
            }
        } else {
            if !add {
                self.selected[col].clear();
            }
            if !self.selected[col].remove(key) {
                self.selected[col].insert(key.into());
            }
            self.anchors[col] = Some(key.into());
        }
        for next in col + 1..3 {
            self.selected[next].clear();
            self.searches[next].clear();
            self.anchors[next] = None;
        }
        self.key = None;
    }
    pub fn refresh(
        &mut self,
        tracks: &[Track],
        scope: Scope,
        source: Option<String>,
        query: &str,
        fields: [Field; 3],
        revision: u64,
    ) -> bool {
        let query = query.trim().to_lowercase();
        let mut searches = self.searches.clone().map(|s| s.trim().to_lowercase());
        if let Some(previous) = &self.key
            && let Some(col) = (0..3).find(|&col| previous.7[col] != searches[col])
        {
            // Typing replaces this column's old choice. Invisible selections must not
            // keep the track table stuck on an artist from the previous search.
            for (next, search) in searches.iter_mut().enumerate().skip(col) {
                self.selected[next].clear();
                self.anchors[next] = None;
                if next > col {
                    self.searches[next].clear();
                    search.clear();
                }
            }
        }
        let key = (
            revision,
            tracks.len(),
            source,
            query.clone(),
            fields,
            self.selected.clone(),
            self.sort,
            searches.clone(),
        );
        if self.key.as_ref() == Some(&key) {
            return false;
        }
        self.reset_scroll |= self.key.as_ref().is_none_or(|old| {
            old.2 != key.2
                || old.3 != key.3
                || old.4 != key.4
                || old.5 != key.5
                || old.6 != key.6
                || old.7 != key.7
        });
        let mut indices: Vec<_> = tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| scope.contains(&t.source))
            .filter(|(_, t)| {
                query.is_empty()
                    || format!(
                        "{} {} {} {} {}",
                        t.title, t.artist, t.album, t.genre, t.source
                    )
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        for (col, field) in fields.into_iter().enumerate() {
            let search = &searches[col];
            let mut matching = BTreeSet::new();
            let mut groups: BTreeMap<String, Entry> = BTreeMap::new();
            for &index in &indices {
                for (key, label, detail) in field.values(&tracks[index]) {
                    if !search.is_empty()
                        && !format!("{label} {detail}").to_lowercase().contains(search)
                    {
                        continue;
                    }
                    if !search.is_empty() {
                        matching.insert(index);
                    }
                    groups
                        .entry(key.clone())
                        .or_insert(Entry {
                            key,
                            label,
                            detail,
                            count: 0,
                        })
                        .count += 1;
                }
            }
            if !search.is_empty() {
                indices.retain(|index| matching.contains(index));
            }
            self.totals[col] = indices.len();
            self.entries[col] = groups.into_values().collect();
            self.entries[col].sort_by_cached_key(|e| {
                (
                    e.key.is_empty(),
                    e.label.to_lowercase(),
                    e.detail.to_lowercase(),
                )
            });
            // Drop vanished values after synchronization or a source/metadata change.
            let available: BTreeSet<_> = self.entries[col].iter().map(|e| e.key.clone()).collect();
            self.selected[col].retain(|k| available.contains(k));
            if !self.selected[col].is_empty() {
                indices.retain(|&i| {
                    field
                        .values(&tracks[i])
                        .iter()
                        .any(|(k, _, _)| self.selected[col].contains(k))
                });
            }
        }
        indices.sort_by_cached_key(|&i| {
            let t = &tracks[i];
            let artist = if t.album_artist.is_empty() {
                &t.artist
            } else {
                &t.album_artist
            };
            match self.sort {
                Sort::Original => (String::new(), String::new(), 0, i as u32, String::new()),
                Sort::Album => (
                    artist.to_lowercase(),
                    t.album.to_lowercase(),
                    t.disc,
                    t.number,
                    t.title.to_lowercase(),
                ),
                Sort::Title => (
                    t.title.to_lowercase(),
                    t.artist.to_lowercase(),
                    0,
                    0,
                    String::new(),
                ),
                Sort::Artist => (
                    t.artist.to_lowercase(),
                    t.album.to_lowercase(),
                    t.disc,
                    t.number,
                    t.title.to_lowercase(),
                ),
                Sort::Duration => (
                    String::new(),
                    String::new(),
                    0,
                    (t.duration.max(0.0) * 1000.0).min(u32::MAX as f64) as u32,
                    t.title.to_lowercase(),
                ),
            }
        });
        self.indices = indices;
        self.key = Some((
            revision,
            tracks.len(),
            key.2,
            query,
            fields,
            self.selected.clone(),
            self.sort,
            searches,
        ));
        self.rebuilds += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(path: &str, artist: &str, album: &str, genre: &str, number: u32) -> Track {
        let mut t = Track::placeholder(path.into());
        t.artist = artist.into();
        t.album = album.into();
        t.genre = genre.into();
        t.number = number;
        t
    }
    fn data() -> Vec<Track> {
        vec![
            track("/phone/A/02.flac", "Alpha", "Sessions", "Jazz; Ambient", 2),
            track("/phone/A/01.flac", "Alpha", "Sessions", "Jazz", 1),
            track("/phone/B/01.flac", "Beta", "Sessions", "Rock", 1),
            track("/nas/C/01.flac", "Gamma", "Elsewhere", "Jazz", 1),
        ]
    }
    fn refresh(b: &mut Browser, t: &[Track], rev: u64) {
        b.refresh(
            t,
            Scope::Local("/phone".into()),
            Some("phone".into()),
            "",
            [Field::Genre, Field::Artist, Field::Album],
            rev,
        );
    }
    #[test]
    fn alternate_ftp_encodings_have_one_folder_identity_and_label() {
        let a = Track::placeholder("ftp://phone/Album%20%28FLAC%20%2B%20CUE%29/01.flac".into());
        let b = Track::placeholder("ftp://phone/Album%20(FLAC%20+%20CUE)/02.flac".into());
        assert_eq!(Field::Folder.value(&a), Field::Folder.value(&b));
        assert_eq!(Field::Folder.value(&a).1, "Album (FLAC + CUE)");
    }
    #[test]
    fn cascades_are_source_scoped_multivalue_and_clear_downstream() {
        let t = data();
        let mut b = Browser::default();
        refresh(&mut b, &t, 0);
        assert_eq!(b.indices, vec![1, 0, 2]);
        assert_eq!(b.totals[0], 3);
        assert_eq!(
            b.entries[0]
                .iter()
                .find(|e| e.label == "Jazz")
                .unwrap()
                .count,
            2
        );
        assert!(!b.entries[1].iter().any(|e| e.label == "Gamma"));
        b.select(0, "jazz", false, false);
        refresh(&mut b, &t, 0);
        assert_eq!(b.indices, vec![1, 0]);
        assert_eq!(b.entries[1].len(), 1);
        b.select(1, "alpha", false, false);
        refresh(&mut b, &t, 0);
        b.select(0, "rock", true, false);
        refresh(&mut b, &t, 0);
        assert!(b.selected[1].is_empty());
        assert_eq!(b.indices.len(), 3);
        b.select(0, "rock", true, false);
        refresh(&mut b, &t, 0);
        assert_eq!(b.indices.len(), 2);
        b.clear();
        refresh(&mut b, &t, 0);
        b.select(0, "ambient", false, false);
        refresh(&mut b, &t, 0);
        assert_eq!(b.indices, vec![0]);
    }
    #[test]
    fn shift_selection_and_album_identity_preserve_distinct_artists() {
        let t = data();
        let mut b = Browser::default();
        refresh(&mut b, &t, 0);
        b.select(0, "ambient", false, false);
        b.select(0, "rock", false, true);
        refresh(&mut b, &t, 0);
        assert_eq!(b.selected[0].len(), 3);
        assert_eq!(b.indices.len(), 3);
        assert_eq!(b.entries[2].len(), 2); // Identical album titles do not merge different artists.
    }
    #[test]
    fn cache_rebuilds_only_for_relevant_changes_and_finds_new_tracks() {
        let mut t = data();
        let mut b = Browser::default();
        refresh(&mut b, &t, 0);
        let rebuilt = b.rebuilds;
        for _ in 0..100 {
            refresh(&mut b, &t, 0);
        }
        assert_eq!(b.rebuilds, rebuilt);
        t.push(track("/phone/A/03.flac", "Alpha", "Sessions", "Jazz", 3));
        refresh(&mut b, &t, 1);
        assert_eq!(b.indices.len(), 4);
        t[4].artist = "Delta".into();
        refresh(&mut b, &t, 2);
        assert!(b.entries[1].iter().any(|e| e.label == "Delta"));
        b.refresh(
            &t,
            Scope::Local("/nas".into()),
            Some("nas".into()),
            "",
            [Field::Genre, Field::Artist, Field::Album],
            2,
        );
        assert_eq!(b.indices, vec![3]);
        b.refresh(
            &t,
            Scope::All,
            None,
            "not-found",
            [Field::Genre, Field::Artist, Field::Album],
            2,
        );
        assert!(b.indices.is_empty());
    }
    #[test]
    fn legacy_tracks_deserialize_missing_tags_and_folder_names_decode_unicode() {
        let json = r#"{"source":"ftp://phone.local/Music/Caf%C3%A9/01.flac","title":"01","artist":"","album":"","codec":"FLAC","rate":0,"bits":0,"channels":0,"duration":0.0,"number":0}"#;
        let t: Track = serde_json::from_str(json).unwrap();
        assert_eq!(t.year, None);
        assert!(t.genre.is_empty());
        assert_eq!(Field::Folder.value(&t).1, "Café");
        let mut b = Browser::default();
        b.refresh(
            &[t],
            Scope::All,
            None,
            "",
            [Field::Genre, Field::Artist, Field::Folder],
            0,
        );
        assert_eq!(b.entries[0][0].label, tr!("Unavailable"));
        assert_eq!(b.indices, vec![0]);
    }
    #[test]
    fn large_library_cache_does_not_rescan_on_animation_frames() {
        let tracks: Vec<_> = (0..30_000)
            .map(|i| {
                track(
                    &format!("/phone/Album{}/{i}.flac", i / 10),
                    &format!("Artist {}", i % 60),
                    &format!("Album {}", i / 10),
                    "Jazz",
                    (i % 10) as u32,
                )
            })
            .collect();
        let mut b = Browser::default();
        refresh(&mut b, &tracks, 0);
        assert_eq!(b.indices.len(), 30_000);
        for _ in 0..300 {
            refresh(&mut b, &tracks, 0);
        }
        assert_eq!(b.rebuilds, 1);
    }
    #[test]
    fn replacing_column_search_replaces_hidden_choices_and_refreshes_tracks_immediately() {
        let t = data();
        let mut b = Browser::default();
        refresh(&mut b, &t, 0);
        b.select(1, "alpha", false, false);
        refresh(&mut b, &t, 0);
        b.select(2, "alpha\0sessions", false, false);
        refresh(&mut b, &t, 0);
        assert_eq!(b.indices, vec![1, 0]);
        for (search, expected) in [
            ("beta", vec![2]),
            (" ALPHA ", vec![1, 0]),
            ("no-match", vec![]),
            ("", vec![1, 0, 2]),
        ] {
            b.searches[1] = search.into();
            refresh(&mut b, &t, 0);
            assert_eq!(b.indices, expected, "search {search:?}");
            assert!(b.selected[1].is_empty());
            assert!(b.selected[2].is_empty());
            assert!(b.reset_scroll);
            let rebuilt = b.rebuilds;
            for _ in 0..100 {
                refresh(&mut b, &t, 0);
            }
            assert_eq!(b.rebuilds, rebuilt, "animation must not redo the query");
        }
    }
    #[test]
    fn column_queries_compose_with_global_search_and_background_metadata_updates() {
        let mut t = data();
        let mut b = Browser::default();
        refresh(&mut b, &t, 0);
        b.searches[0] = "jazz".into();
        b.searches[1] = "alpha".into();
        // One changed column clears downstream inputs, like selecting its value.
        refresh(&mut b, &t, 0);
        b.searches[1] = "alpha".into();
        refresh(&mut b, &t, 0);
        assert_eq!(b.indices, vec![1, 0]);
        assert!(b.active());
        b.refresh(
            &t,
            Scope::Local("/phone".into()),
            Some("phone".into()),
            "02",
            [Field::Genre, Field::Artist, Field::Album],
            0,
        );
        assert_eq!(b.indices, vec![0]);
        b.refresh(
            &t,
            Scope::Local("/phone".into()),
            Some("phone".into()),
            "01",
            [Field::Genre, Field::Artist, Field::Album],
            0,
        );
        assert_eq!(b.indices, vec![1]);
        t.push(track(
            "/phone/D/01.flac",
            "Alpha Duo",
            "New tags",
            "Jazz",
            1,
        ));
        refresh(&mut b, &t, 1);
        assert_eq!(b.indices, vec![1, 0, 4]);
        b.clear();
        refresh(&mut b, &t, 1);
        assert_eq!(b.indices.len(), 4);
        assert!(!b.active());
    }
}
