//! Named playlists are saved independently from the library and the playback snapshot.
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{
    collections::HashSet,
    io::{Read, Write},
    path::Path,
};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_LISTS: usize = 1000;
const MAX_ENTRIES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Playlist {
    pub id: u64,
    pub name: String,
    pub tracks: Arc<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<crate::playlist_rules::Rules>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Book {
    pub version: u32,
    pub lists: Vec<Playlist>,
}
impl Default for Book {
    fn default() -> Self {
        Self {
            version: 2,
            lists: Vec::new(),
        }
    }
}
impl Book {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err("Playlists unavailable.".into()),
        };
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "Playlists unavailable.")?;
        if bytes.len() > MAX_BYTES {
            return Err("Playlists unavailable.".into());
        }
        let mut book: Self =
            serde_json::from_slice(&bytes).map_err(|_| "Playlists unavailable.")?;
        if book.version == 1 && book.lists.iter().all(|p| p.rules.is_none()) {
            book.version = 2;
        }
        book.validate()?;
        Ok(book)
    }
    fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        if self.version != 2
            || self.lists.len() > MAX_LISTS
            || self.lists.iter().map(|p| p.tracks.len()).sum::<usize>() > MAX_ENTRIES
        {
            return Err("Playlists unavailable.".into());
        }
        for list in &self.lists {
            if !ids.insert(list.id)
                || !valid_name(&list.name)
                || list.tracks.iter().any(|s| !valid_source(s))
                || list
                    .rules
                    .as_ref()
                    .is_some_and(|r| !r.valid() || !list.tracks.is_empty())
            {
                return Err("Playlists unavailable.".into());
            }
        }
        Ok(())
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| "Could not save playlist.")?;
        if bytes.len() > MAX_BYTES {
            return Err("Could not save playlist.".into());
        }
        atomic_write(path, &bytes)
    }
    pub fn unique_name(&self, base: &str) -> String {
        let base: String = base.trim().chars().take(100).collect();
        let base = if base.is_empty() { "Playlist" } else { &base };
        for n in 1..=MAX_LISTS + 1 {
            let name = if n == 1 {
                base.to_string()
            } else {
                format!("{base} ({n})")
            };
            if !self
                .lists
                .iter()
                .any(|p| p.name.to_lowercase() == name.to_lowercase())
            {
                return name;
            }
        }
        base.into()
    }
    pub fn create(&mut self, name: &str, tracks: Vec<String>) -> Result<u64, String> {
        if self.lists.len() >= MAX_LISTS || !valid_name(name) {
            return Err("Could not save playlist.".into());
        }
        let mut id = rand::random::<u64>();
        while self.lists.iter().any(|p| p.id == id) {
            id = rand::random();
        }
        let mut seen = HashSet::new();
        let tracks: Vec<_> = tracks
            .into_iter()
            .filter(|s| valid_source(s) && seen.insert(s.clone()))
            .collect();
        let mut candidate = self.clone();
        candidate.lists.push(Playlist {
            id,
            name: candidate.unique_name(name),
            tracks: Arc::new(tracks),
            rules: None,
        });
        candidate.validate()?;
        *self = candidate;
        Ok(id)
    }
    pub fn get(&self, id: u64) -> Option<&Playlist> {
        self.lists.iter().find(|p| p.id == id)
    }
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let name = name.trim();
        if !valid_name(name)
            || self
                .lists
                .iter()
                .any(|p| p.id != id && p.name.to_lowercase() == name.to_lowercase())
        {
            return false;
        }
        if let Some(list) = self.lists.iter_mut().find(|p| p.id == id) {
            list.name = name.into();
            return true;
        }
        false
    }
    pub fn remove(&mut self, id: u64) {
        self.lists.retain(|p| p.id != id);
    }
    pub fn append(
        &mut self,
        id: u64,
        tracks: impl IntoIterator<Item = String>,
    ) -> Result<(), String> {
        let total = self.lists.iter().map(|p| p.tracks.len()).sum::<usize>();
        let list = self
            .lists
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Could not save playlist.")?;
        if list.rules.is_some() {
            return Err("Could not save playlist.".into());
        }
        let mut seen: HashSet<_> = list.tracks.iter().cloned().collect();
        let added: Vec<_> = tracks
            .into_iter()
            .filter(|s| valid_source(s) && seen.insert(s.clone()))
            .collect();
        if total + added.len() > MAX_ENTRIES {
            return Err("Could not save playlist.".into());
        }
        Arc::make_mut(&mut list.tracks).extend(added);
        Ok(())
    }
    pub fn remove_track(&mut self, id: u64, index: usize) {
        if let Some(list) = self.lists.iter_mut().find(|p| p.id == id)
            && list.rules.is_none()
            && index < list.tracks.len()
        {
            Arc::make_mut(&mut list.tracks).remove(index);
        }
    }
    pub fn move_track(&mut self, id: u64, index: usize, target: usize) {
        if let Some(list) = self.lists.iter_mut().find(|p| p.id == id)
            && list.rules.is_none()
            && index < list.tracks.len()
            && target < list.tracks.len()
        {
            Arc::make_mut(&mut list.tracks).swap(index, target);
        }
    }
    /// Preserve unavailable entries; synchronization only changes verified aliases.
    pub fn remap(&mut self, mut map: impl FnMut(&str) -> Option<String>) -> bool {
        let mut changed = false;
        for list in &mut self.lists {
            for source in Arc::make_mut(&mut list.tracks).iter_mut() {
                if let Some(new) = map(source)
                    && *source != new
                    && valid_source(&new)
                {
                    *source = new;
                    changed = true;
                }
            }
            let mut seen = HashSet::new();
            let previous = list.tracks.len();
            Arc::make_mut(&mut list.tracks).retain(|s| seen.insert(s.clone()));
            changed |= previous != list.tracks.len();
        }
        changed
    }
    #[cfg(test)]
    pub fn resolve(&self, id: u64, library: &[crate::library::Track]) -> Arc<Vec<String>> {
        self.resolve_with_stats(id, library, &Default::default())
    }
    #[cfg(test)]
    pub fn resolve_with_stats(
        &self,
        id: u64,
        library: &[crate::library::Track],
        stats: &crate::track_stats::Stats,
    ) -> Arc<Vec<String>> {
        self.resolve_with_scale(id, library, stats, musiccraft_tags::Scale::Explicit)
    }
    pub fn resolve_with_scale(
        &self,
        id: u64,
        library: &[crate::library::Track],
        stats: &crate::track_stats::Stats,
        scale: musiccraft_tags::Scale,
    ) -> Arc<Vec<String>> {
        let Some(list) = self.get(id) else {
            return Arc::default();
        };
        match &list.rules {
            Some(rules) => Arc::new(rules.select_with_scale(library, stats, scale)),
            None => list.tracks.clone(),
        }
    }
    pub fn duplicate(&mut self, id: u64) -> Result<u64, String> {
        let original = self.get(id).cloned().ok_or("Could not save playlist.")?;
        let new_id = self.create(&original.name, original.tracks.as_ref().clone())?;
        self.set_rules(new_id, original.rules)?;
        Ok(new_id)
    }
    pub fn set_rules(
        &mut self,
        id: u64,
        rules: Option<crate::playlist_rules::Rules>,
    ) -> Result<(), String> {
        if rules.as_ref().is_some_and(|r| !r.valid()) {
            return Err("Could not save playlist.".into());
        }
        let list = self
            .lists
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Could not save playlist.")?;
        if rules.is_some() {
            list.tracks = Arc::default();
        }
        list.rules = rules;
        Ok(())
    }
    #[cfg(test)]
    pub fn m3u(&self, id: u64, library: &[crate::library::Track]) -> Result<String, String> {
        self.m3u_with_stats(id, library, &Default::default())
    }
    #[cfg(test)]
    pub fn m3u_with_stats(
        &self,
        id: u64,
        library: &[crate::library::Track],
        stats: &crate::track_stats::Stats,
    ) -> Result<String, String> {
        self.m3u_with_scale(id, library, stats, musiccraft_tags::Scale::Explicit)
    }
    pub fn m3u_with_scale(
        &self,
        id: u64,
        library: &[crate::library::Track],
        stats: &crate::track_stats::Stats,
        scale: musiccraft_tags::Scale,
    ) -> Result<String, String> {
        if self.get(id).is_none() {
            return Err("Could not save playlist.".into());
        }
        let tracks = self.resolve_with_scale(id, library, stats, scale);
        let mut text = String::from("#EXTM3U\n");
        for source in tracks.iter() {
            let line = if crate::library::is_remote(source) {
                crate::library::validate_url(source)?
            } else {
                url::Url::from_file_path(source)
                    .map_err(|_| "Could not save playlist.")?
                    .to_string()
            };
            text.push_str(&line);
            text.push('\n');
        }
        Ok(text)
    }
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 160 && !name.chars().any(char::is_control)
}
pub(crate) fn valid_source(source: &str) -> bool {
    !source.is_empty()
        && source.len() <= 16384
        && if crate::library::is_remote(source) {
            crate::library::validate_url(source).is_ok()
        } else {
            Path::new(source).is_absolute()
        }
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Could not save playlist.")?;
    std::fs::create_dir_all(parent).map_err(|_| "Could not save playlist.")?;
    let temp = parent.join(format!(
        ".musiccraft-playlists-{:016x}.tmp",
        rand::random::<u64>()
    ));
    let mut created = false;
    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        created = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() && created {
        let _ = std::fs::remove_file(temp);
    }
    result.map_err(|_| "Could not save playlist.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_roundtrip_without_touching_tracks_and_export_preserves_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playlists.json");
        let mut b = Book::default();
        let a = dir
            .path()
            .join("Città #1.flac")
            .to_string_lossy()
            .into_owned();
        let c = dir.path().join("2.wav").to_string_lossy().into_owned();
        let id = b
            .create("Viaggio", vec![a.clone(), c.clone(), a.clone()])
            .unwrap();
        assert_eq!(b.get(id).unwrap().tracks.len(), 2);
        b.move_track(id, 1, 0);
        assert!(b.rename(id, "Sera"));
        let copy = b
            .create("Sera", b.get(id).unwrap().tracks.as_ref().clone())
            .unwrap();
        assert_ne!(id, copy);
        b.remove_track(copy, 0);
        b.save(&path).unwrap();
        assert_eq!(Book::load(&path).unwrap(), b);
        let m3u = dir.path().join("out.m3u8");
        atomic_write(&m3u, b.m3u(id, &[]).unwrap().as_bytes()).unwrap();
        assert_eq!(crate::library::playlist(&m3u).unwrap(), vec![c, a]);
        b.remove(id);
        assert!(b.get(copy).is_some());
    }
    #[test]
    fn failed_load_and_save_do_not_destroy_previous_data() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("playlists.json");
        std::fs::write(&p, b"{\"version\":3,\"lists\":[]}").unwrap();
        assert!(Book::load(&p).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"{\"version\":3,\"lists\":[]}");
        assert!(
            Book::default()
                .save(&dir.path().join("playlists.json/blocked"))
                .is_err()
        );
    }
    #[test]
    fn rebasing_keeps_order_missing_references_and_other_servers() {
        let mut b = Book::default();
        let id = b
            .create(
                "Phone",
                vec![
                    "https://old.local:8737/v1/audio/1.flac".into(),
                    "https://other.local/2.flac".into(),
                    "/missing/3.flac".into(),
                ],
            )
            .unwrap();
        b.remap(|s| crate::network::rebase(s, "https://old.local:8737/", "https://10.0.0.3:8737/"));
        assert_eq!(
            b.get(id).unwrap().tracks[0],
            "https://10.0.0.3:8737/v1/audio/1.flac"
        );
        assert_eq!(b.get(id).unwrap().tracks[2], "/missing/3.flac");
        assert!(!valid_source(
            "https://fixture:fixture-only@nas.invalid/a.flac"
        ));
        assert!(!valid_source("https://host/a.flac?token=secret"));
    }
}

#[cfg(test)]
mod dynamic_tests {
    use super::*;
    use crate::{
        library::Track,
        playlist_rules::{Comparison, Rule, Rules, Tag},
    };
    #[test]
    fn legacy_manual_lists_upgrade_and_rules_roundtrip_without_derived_tracks() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("playlists.json");
        let legacy = br#"{"version":1,"lists":[{"id":42,"name":"Manual","tracks":["/old.wav"]}]}"#;
        std::fs::write(&path, legacy).unwrap();
        let mut book = Book::load(&path).unwrap();
        assert_eq!(book.version, 2);
        assert_eq!(book.get(42).unwrap().tracks.as_slice(), ["/old.wav"]);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            legacy,
            "loading never rewrites old data"
        );
        let id = book.create("Rock", vec![]).unwrap();
        let rules = Rules {
            all: true,
            conditions: vec![Rule {
                tag: Tag::Genre,
                comparison: Comparison::Equals,
                value: "Rock".into(),
            }],
        };
        book.set_rules(id, Some(rules.clone())).unwrap();
        let copy = book.duplicate(id).unwrap();
        assert_eq!(book.get(copy).unwrap().rules, Some(rules));
        let mut t = Track::placeholder("/new.wav".into());
        t.genre = "Rock".into();
        assert_eq!(book.resolve(id, &[t.clone()]).as_slice(), ["/new.wav"]);
        assert!(book.get(id).unwrap().tracks.is_empty());
        assert!(book.append(id, ["/unwanted.wav".into()]).is_err());
        book.save(&path).unwrap();
        let reopened = Book::load(&path).unwrap();
        assert_eq!(book, reopened);
        assert!(reopened.m3u(id, &[t]).unwrap().contains("file:///new.wav"));
        assert!(reopened.resolve(id, &[]).is_empty());
    }
}
