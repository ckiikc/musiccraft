//! Opt-in, read-only library diagnosis. Reports and downloaded artwork stay in an isolated directory.
use super::*;
use std::{collections::BTreeMap, io::Write, path::PathBuf, sync::atomic::AtomicU64};

#[derive(Deserialize)]
struct Profile {
    tracks: Vec<Track>,
}

pub(super) fn record_response(provider: usize, t: &Track, response: &Result<Option<Value>>) {
    let Some(output) = std::env::var_os("MUSICCRAFT_AUDIT_OUTPUT") else {
        return;
    };
    let artist = if t.album_artist.is_empty() {
        &t.artist
    } else {
        &t.album_artist
    };
    let root = PathBuf::from(output).join("provider-results");
    std::fs::create_dir_all(&root).unwrap();
    let value = match response {
        Ok(Some(value)) => value.clone(),
        Ok(None) => serde_json::json!({"status":"not_found"}),
        Err(e) => serde_json::json!({"status":format!("{e:?}")}),
    };
    std::fs::write(
        root.join(format!("{}-{provider}.json", key(artist, &t.album))),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "Explicit private-library audit; requires MUSICCRAFT_AUDIT_PROFILE and MUSICCRAFT_AUDIT_OUTPUT"]
fn inventory_library_covers() {
    let profile = PathBuf::from(std::env::var_os("MUSICCRAFT_AUDIT_PROFILE").unwrap());
    let output = PathBuf::from(std::env::var_os("MUSICCRAFT_AUDIT_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let outer: HashMap<String, String> =
        ron::from_str(&std::fs::read_to_string(&profile).unwrap()).unwrap();
    let saved: Profile = ron::from_str(&outer["aurea"]).unwrap();
    let cache = profile.parent().unwrap().join("cache");
    let mut groups = BTreeMap::<(String, String, String), Vec<Track>>::new();
    let mut remote_unindexed = 0;
    for mut t in saved.tracks {
        if library::is_remote(&t.source) {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            t.source.hash(&mut h);
            let local = cache.join(format!("{:016x}.audio", h.finish()));
            if !local.is_file() {
                remote_unindexed += 1;
                continue;
            }
            t.update_metadata(&local);
            t.source = local.to_string_lossy().into_owned();
        }
        let folder = Path::new(&t.source)
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let artist = if t.album_artist.is_empty() {
            &t.artist
        } else {
            &t.album_artist
        };
        groups
            .entry((folder, artist.clone(), t.album.clone()))
            .or_default()
            .push(t);
    }
    println!(
        "AUDIT groups={} remote_without_cached_audio={remote_unindexed}",
        groups.len()
    );
    let existing: std::collections::HashSet<String> =
        std::fs::read_to_string(output.join("inventory.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|r| r["track"]["source"].as_str().map(str::to_owned))
            .collect();
    let groups: Vec<_> = groups
        .into_values()
        .filter(|g| !existing.contains(&g[0].source))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let report = Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(output.join("inventory.jsonl"))
            .unwrap(),
    );
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                let auth = crate::network::AuthStore::default();
                let generation = AtomicU64::new(1);
                let mut folders = HashMap::<PathBuf, Option<(bool, Vec<String>)>>::new();
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(tracks) = groups.get(i) else { break };
                    let t = &tracks[0];
                    let folder = Path::new(&t.source).parent().unwrap();
                    let state = folders.entry(folder.to_owned()).or_insert_with(|| {
                        let entries = std::fs::read_dir(folder).ok()?;
                        let images = entries.filter_map(std::result::Result::ok).filter_map(|e| {
                            let p = e.path();let ext = p.extension()?.to_str()?.to_ascii_lowercase();
                            ["jpg", "jpeg", "png", "webp", "bmp"].contains(&ext.as_str()).then(|| e.file_name().to_string_lossy().into_owned())
                        }).collect();
                        let found = crate::jobs::artwork(Path::new("/synthetic/no-audio"), &t.source, &output, &auth, &generation, 1).is_some();
                        Some((found, images))
                    });
                    let mut status = "unreachable";
                    let mut tag_errors = 0;
                    let mut images = Vec::new();
                    if let Some((folder_art, names)) = state {
                        images = names.clone();
                        status = if *folder_art { "folder" } else { "missing" };
                        if !*folder_art {
                            for track in tracks {
                                if let Some(bytes) = crate::audio::cover(Path::new(&track.source)) {
                                    if library::decode_art(&bytes).is_some() { status = "embedded"; break; }
                                    tag_errors += 1;
                                }
                            }
                        }
                    }
                    let artist = if t.album_artist.is_empty() { &t.artist } else { &t.album_artist };
                    let row = serde_json::json!({"index":i,"artist":artist,"album":t.album,"track":t,"tracks":tracks.len(),"status":status,"folder_images":images,"invalid_embedded_images":tag_errors,"key":key(artist,&t.album)});
                    let mut file = report.lock().unwrap();
                    writeln!(file,"{row}").unwrap();file.flush().unwrap();
                    if i.is_multiple_of(25) { println!("AUDIT inventory {i}/{}",groups.len()); }
                }
            });
        }
    });
}

#[test]
#[ignore = "Explicit online cover audit; requires MUSICCRAFT_AUDIT_OUTPUT"]
fn lookup_missing_library_covers() {
    let output = PathBuf::from(std::env::var_os("MUSICCRAFT_AUDIT_OUTPUT").unwrap());
    let mut report = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(output.join("online.jsonl"))
        .unwrap();
    let already: std::collections::HashSet<String> =
        std::fs::read_to_string(output.join("online.jsonl"))
            .unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|r| r["found"] == true)
            .filter_map(|r| r["key"].as_str().map(str::to_owned))
            .collect();
    let inventory = std::fs::read_to_string(output.join("inventory-rechecked.jsonl")).unwrap();
    let mut seen = already;
    for line in inventory.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        if row["status"] != "missing" {
            continue;
        }
        let t: Track = serde_json::from_value(row["track"].clone()).unwrap();
        if t.album.is_empty() || (t.artist.is_empty() && t.album_artist.is_empty()) {
            continue;
        }
        let k = row["key"].as_str().unwrap();
        if !seen.insert(k.to_owned()) {
            continue;
        }
        let art = online(&t, &output.join("cache"), &|| false);
        let lookup = lookup_track(&t, &output.join("cache"));
        let lookup_artist = if lookup.album_artist.is_empty() {
            &lookup.artist
        } else {
            &lookup.album_artist
        };
        let result = serde_json::json!({"key":k,"artist":row["artist"],"album":t.album,"found":art.is_some(),"origin":art.as_ref().and_then(|a|a.origin.clone()),"size":art.map(|a|a.size),"lookup_artist":lookup_artist,"lookup_album":lookup.album,"lookup_key":key(lookup_artist,&lookup.album)});
        writeln!(report, "{result}").unwrap();
        report.flush().unwrap();
        println!("AUDIT online {}", seen.len());
    }
}

#[test]
#[ignore = "Explicit local recheck after matching fixes; requires MUSICCRAFT_AUDIT_OUTPUT"]
fn recheck_local_covers() {
    let output = PathBuf::from(std::env::var_os("MUSICCRAFT_AUDIT_OUTPUT").unwrap());
    let input = std::fs::read_to_string(output.join("inventory.jsonl")).unwrap();
    let mut records = Vec::new();
    let auth = crate::network::AuthStore::default();
    let generation = AtomicU64::new(1);
    for line in input.lines() {
        let mut row: Value = serde_json::from_str(line).unwrap();
        let track: Track = serde_json::from_value(row["track"].clone()).unwrap();
        let artist = if track.album_artist.is_empty() {
            &track.artist
        } else {
            &track.album_artist
        };
        row["key"] = key(artist, &track.album).into();
        if row["status"] == "missing"
            && crate::jobs::artwork_for_track(
                Path::new("/synthetic/no-audio"),
                &track,
                &output.join("cache"),
                &auth,
                &generation,
                1,
            )
            .is_some()
        {
            row["status"] = "folder_recovered".into();
        }
        records.push(row);
    }
    let mut file = std::fs::File::create(output.join("inventory-rechecked.jsonl")).unwrap();
    for row in records {
        writeln!(file, "{row}").unwrap();
    }
}

#[test]
#[ignore = "Explicit provider diagnostics for audited misses; requires MUSICCRAFT_AUDIT_OUTPUT"]
fn diagnose_missing_matches() {
    let output = PathBuf::from(std::env::var_os("MUSICCRAFT_AUDIT_OUTPUT").unwrap());
    let inventory: Vec<Value> = std::fs::read_to_string(output.join("inventory.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for line in std::fs::read_to_string(output.join("online.jsonl"))
        .unwrap()
        .lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        if row["found"] == true {
            continue;
        }
        let Some(record) = inventory.iter().find(|r| r["key"] == row["key"]) else {
            continue;
        };
        let t: Track = serde_json::from_value(record["track"].clone()).unwrap();
        for provider in 0..3 {
            let file = output
                .join("provider-results")
                .join(format!("{}-{provider}.json", row["key"].as_str().unwrap()));
            if !file.exists() {
                let _ = candidates(provider, &t, &|| false);
            }
        }
        println!("AUDIT diagnosed {}", row["key"]);
    }
}
