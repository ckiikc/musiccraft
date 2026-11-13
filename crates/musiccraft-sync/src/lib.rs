//! Pure planning only. Adapters own consent, complete listing, verified transfers and recovery.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;
pub mod filesystem;

pub const MAX_FILES: usize = 100_000;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub size: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub root: String,
    pub files: BTreeMap<String, Stamp>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    #[default]
    Forward,
    Reverse,
    Both,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    A,
    B,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Copy(Side),
    Delete(Side),
    Conflict,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub path: String,
    pub action: Action,
    pub a: Option<Stamp>,
    pub b: Option<Stamp>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    pub a: Snapshot,
    pub b: Snapshot,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub before: Baseline,
    pub operations: Vec<Operation>,
}
#[derive(Deserialize)]
pub struct Request {
    pub a: Snapshot,
    pub b: Snapshot,
    pub direction: Direction,
    pub baseline: Option<Baseline>,
}
pub fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains(['\\', ':'])
        && path.split('/').all(|p| {
            !p.is_empty()
                && !p.starts_with('.')
                && p.len() <= 240
                && !p.ends_with([' ', '.'])
                && !p.chars().any(char::is_control)
                && !["CON", "PRN", "AUX", "NUL", "COM1", "LPT1"]
                    .contains(&p.to_uppercase().as_str())
        })
}
pub fn validate(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.root.is_empty() || snapshot.root.len() > 4096 || snapshot.files.len() > MAX_FILES {
        return Err("Invalid or incomplete folder snapshot".into());
    }
    let mut names = BTreeSet::new();
    for (path, stamp) in &snapshot.files {
        let key: String = path.nfc().collect::<String>().to_lowercase();
        if !valid_path(path)
            || !names.insert(key)
            || stamp.sha256.len() != 64
            || !stamp
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("Unsafe name, case collision or invalid fingerprint".into());
        }
    }
    for path in &names {
        let mut prefix = path.as_str();
        while let Some((parent, _)) = prefix.rsplit_once('/') {
            if names.contains(parent) {
                return Err("File and folder names collide".into());
            }
            prefix = parent;
        }
    }
    Ok(())
}
pub fn plan(request: Request) -> Result<Plan, String> {
    let Request {
        a,
        b,
        direction,
        baseline,
    } = request;
    validate(&a)?;
    validate(&b)?;
    if a.root == b.root {
        return Err("Choose two different folders".into());
    }
    if let Some(old) = &baseline {
        validate(&old.a)?;
        validate(&old.b)?;
        if old.a.root != a.root || old.b.root != b.root {
            return Err("Folder identity changed: start a new synchronization pair".into());
        }
    }
    // Check collisions across both roots too, even when each root alone is valid.
    let mut combined = a.clone();
    combined.files.extend(b.files.clone());
    validate(&combined)?;
    let paths: BTreeSet<_> = a
        .files
        .keys()
        .chain(b.files.keys())
        .chain(
            baseline
                .iter()
                .flat_map(|p| p.a.files.keys().chain(p.b.files.keys())),
        )
        .cloned()
        .collect();
    let mut operations = Vec::new();
    for path in paths {
        let x = a.files.get(&path);
        let y = b.files.get(&path);
        if x == y {
            continue;
        }
        let old_x = baseline.as_ref().and_then(|p| p.a.files.get(&path));
        let old_y = baseline.as_ref().and_then(|p| p.b.files.get(&path));
        let action = match direction {
            Direction::Forward | Direction::Reverse => {
                let (source, target, old_source, old_target, target_side) =
                    if direction == Direction::Forward {
                        (x, y, old_x, old_y, Side::B)
                    } else {
                        (y, x, old_y, old_x, Side::A)
                    };
                if source.is_none() {
                    Action::Delete(target_side)
                } else if target.is_some() && source != old_source && target != old_target {
                    Action::Conflict
                } else {
                    Action::Copy(target_side)
                }
            }
            Direction::Both => match (x != old_x, y != old_y) {
                (true, false) => {
                    if x.is_some() {
                        Action::Copy(Side::B)
                    } else {
                        Action::Delete(Side::B)
                    }
                }
                (false, true) => {
                    if y.is_some() {
                        Action::Copy(Side::A)
                    } else {
                        Action::Delete(Side::A)
                    }
                }
                _ => Action::Conflict,
            },
        };
        operations.push(Operation {
            path,
            action,
            a: x.cloned(),
            b: y.cloned(),
        });
    }
    Ok(Plan {
        before: Baseline { a, b },
        operations,
    })
}
impl Plan {
    /// Preserve the chosen side, including an explicitly chosen deletion.
    pub fn resolve(&mut self, index: usize, keep: Side) -> Result<(), String> {
        let op = self.operations.get_mut(index).ok_or("Missing conflict")?;
        if op.action != Action::Conflict {
            return Err("Not a conflict".into());
        }
        let (source, target) = match keep {
            Side::A => (&op.a, Side::B),
            Side::B => (&op.b, Side::A),
        };
        op.action = if source.is_some() {
            Action::Copy(target)
        } else {
            Action::Delete(target)
        };
        Ok(())
    }
    pub fn ready(&self) -> bool {
        !self.operations.iter().any(|o| o.action == Action::Conflict)
    }
    pub fn after(&self) -> Result<Baseline, String> {
        if !self.ready() {
            return Err("Resolve every conflict before applying".into());
        }
        let mut after = self.before.clone();
        for op in &self.operations {
            match op.action {
                Action::Copy(Side::A) => {
                    after
                        .a
                        .files
                        .insert(op.path.clone(), op.b.clone().ok_or("Missing source")?);
                }
                Action::Copy(Side::B) => {
                    after
                        .b
                        .files
                        .insert(op.path.clone(), op.a.clone().ok_or("Missing source")?);
                }
                Action::Delete(Side::A) => {
                    after.a.files.remove(&op.path);
                }
                Action::Delete(Side::B) => {
                    after.b.files.remove(&op.path);
                }
                Action::Conflict => unreachable!(),
            }
        }
        Ok(after)
    }
}
pub fn json(input: &str) -> String {
    let result = serde_json::from_str::<Request>(input)
        .map_err(|e| e.to_string())
        .and_then(plan);
    match result {
        Ok(p) => serde_json::to_string(&p).unwrap(),
        Err(e) => serde_json::json!({"error": e}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snap(id: &str, files: &[(&str, u8)]) -> Snapshot {
        Snapshot {
            root: id.into(),
            files: files
                .iter()
                .map(|(p, n)| {
                    (
                        p.to_string(),
                        Stamp {
                            size: 5,
                            sha256: format!("{n:064x}"),
                        },
                    )
                })
                .collect(),
        }
    }
    fn run(a: Snapshot, b: Snapshot, baseline: Option<Baseline>) -> Plan {
        plan(Request {
            a,
            b,
            baseline,
            direction: Direction::Both,
        })
        .unwrap()
    }
    #[test]
    fn first_merge_copies_both_ways_and_never_guesses_conflicts() {
        let p = run(
            snap("a", &[("one.flac", 1), ("same.flac", 2)]),
            snap("b", &[("two.flac", 3), ("same.flac", 4)]),
            None,
        );
        assert_eq!(
            p.operations
                .iter()
                .map(|o| o.action.clone())
                .collect::<Vec<_>>(),
            [
                Action::Copy(Side::B),
                Action::Conflict,
                Action::Copy(Side::A)
            ]
        );
        assert!(p.after().is_err());
    }
    #[test]
    fn deletion_propagates_only_with_baseline_and_unchanged_peer() {
        let old = Baseline {
            a: snap("a", &[("one.flac", 1)]),
            b: snap("b", &[("one.flac", 1)]),
        };
        assert_eq!(
            run(snap("a", &[]), old.b.clone(), Some(old.clone())).operations[0].action,
            Action::Delete(Side::B)
        );
        assert_eq!(
            run(snap("a", &[]), snap("b", &[("one.flac", 2)]), Some(old)).operations[0].action,
            Action::Conflict
        );
    }
    #[test]
    fn empty_first_root_is_not_a_deletion_in_both_directions() {
        assert_eq!(
            run(snap("a", &[]), snap("b", &[("one.flac", 1)]), None).operations[0].action,
            Action::Copy(Side::A)
        );
    }
    #[test]
    fn resolution_and_next_preview_are_stable() {
        let mut p = run(
            snap("a", &[("one.flac", 1)]),
            snap("b", &[("one.flac", 2)]),
            None,
        );
        p.resolve(0, Side::A).unwrap();
        let after = p.after().unwrap();
        assert_eq!(after.a.files, after.b.files);
        assert!(
            run(after.a.clone(), after.b.clone(), Some(after))
                .operations
                .is_empty()
        );
    }
    #[test]
    fn rejects_traversal_case_and_identity_changes() {
        for path in [
            "../file",
            "a/../b",
            "/file",
            "a\\b",
            "a:foo",
            "a//b",
            ".musiccraft-sync/x",
        ] {
            assert!(!valid_path(path));
        }
        assert!(
            plan(Request {
                a: snap("a", &[("A.flac", 1)]),
                b: snap("b", &[("a.flac", 1)]),
                direction: Direction::Both,
                baseline: None
            })
            .is_err()
        );
        let old = Baseline {
            a: snap("old", &[]),
            b: snap("b", &[]),
        };
        assert!(
            plan(Request {
                a: snap("new", &[]),
                b: old.b.clone(),
                direction: Direction::Both,
                baseline: Some(old)
            })
            .is_err()
        );
    }
    #[test]
    fn one_way_mirror_preview_includes_destination_only_deletes() {
        let p = plan(Request {
            a: snap("a", &[]),
            b: snap("b", &[("extra.flac", 1)]),
            direction: Direction::Forward,
            baseline: None,
        })
        .unwrap();
        assert_eq!(p.operations[0].action, Action::Delete(Side::B));
    }
}
