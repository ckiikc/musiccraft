//! Paced, read-only discovery within the computer’s attached private IPv4 subnets.
use crate::network::{self, AuthStore, FtpSource};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lan {
    pub address: Ipv4Addr,
    pub mask: Ipv4Addr,
}
impl Lan {
    pub fn new(address: Ipv4Addr, mask: Ipv4Addr) -> Option<Self> {
        let mask_bits = u32::from(mask);
        let prefix = mask_bits.leading_ones();
        if !(8..=31).contains(&prefix) || mask_bits != u32::MAX << (32 - prefix) {
            return None;
        }
        let lan = Self { address, mask };
        let (first, last) = lan.bounds();
        // A wide mask must still describe a wholly private subnet.
        (Ipv4Addr::from(first).is_private() && Ipv4Addr::from(last).is_private()).then_some(lan)
    }
    fn bounds(&self) -> (u32, u32) {
        let mask = u32::from(self.mask);
        let first = u32::from(self.address) & mask;
        (first, first | !mask)
    }
    fn host_range(&self) -> std::ops::RangeInclusive<u32> {
        let (first, last) = self.bounds();
        // Both addresses are usable on a /31.
        let reserved = u32::from(last - first > 1);
        first + reserved..=last - reserved
    }
    fn has_host(&self, ip: Ipv4Addr) -> bool {
        ip != self.address && self.host_range().contains(&u32::from(ip))
    }
    fn hosts(&self) -> impl Iterator<Item = Ipv4Addr> {
        let local = u32::from(self.address);
        let range = self.host_range();
        (local + 1..=*range.end())
            .chain(*range.start()..local)
            .map(Ipv4Addr::from)
    }
}
pub fn lans() -> Vec<Lan> {
    let mut result: Vec<_> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| i.is_oper_up() && !i.is_p2p())
        .filter(|i| !cfg!(target_os = "macos") || i.name.starts_with("en"))
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) => Lan::new(v4.ip, v4.netmask),
            _ => None,
        })
        .collect();
    result.sort();
    result.dedup();
    result
}
fn mac(value: &str) -> Option<String> {
    let bytes: Vec<_> = value
        .split(':')
        .map(|s| u8::from_str_radix(s, 16))
        .collect::<Result<_, _>>()
        .ok()?;
    if bytes.len() != 6 || bytes[0] & 1 != 0 || bytes.iter().all(|&b| b == 0) {
        return None;
    }
    Some(
        bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}
fn parse_neighbors(text: &str) -> Vec<(Ipv4Addr, String)> {
    text.lines()
        .filter_map(|line| {
            let ip = line.split_once('(')?.1.split_once(')')?.0.parse().ok()?;
            let address = mac(line.split_once(" at ")?.1.split_whitespace().next()?)?;
            Some((ip, address))
        })
        .collect()
}
fn neighbors() -> Vec<(Ipv4Addr, String)> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/sbin/arp")
            .arg("-an")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| parse_neighbors(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Relative file names from several album directories; no audio download or private hash log.
#[derive(Default)]
pub struct Fingerprint {
    groups: Vec<(String, HashSet<String>)>,
}
impl Fingerprint {
    pub fn from_library(root: &str, tracks: impl Iterator<Item = impl AsRef<str>>) -> Self {
        let mut groups = BTreeMap::<String, Vec<String>>::new();
        for track in tracks {
            let Some(relative) = track.as_ref().strip_prefix(root) else {
                continue;
            };
            let (parent, name) = relative
                .rsplit_once('/')
                .map_or(("", relative), |(p, n)| (p, n));
            if name.is_empty() {
                continue;
            }
            let Ok(name) = percent_encoding::percent_decode_str(name).decode_utf8() else {
                continue;
            };
            let names = groups
                .entry(if parent.is_empty() {
                    String::new()
                } else {
                    format!("{parent}/")
                })
                .or_default();
            if names.len() < 3 {
                names.push(name.into_owned());
            }
        }
        let groups: Vec<_> = groups.into_iter().collect();
        let mut selected = HashSet::new();
        let mut result = Vec::new();
        for i in [0, groups.len() / 2, groups.len().saturating_sub(1)] {
            if selected.insert(i)
                && let Some((parent, names)) = groups.get(i)
            {
                result.push((parent.clone(), names.iter().cloned().collect()));
            }
        }
        Self { groups: result }
    }
    fn matches(&self, source: &FtpSource, cancel: &AtomicBool, deadline: Instant) -> bool {
        if self.groups.is_empty() || cancel.load(Ordering::Relaxed) {
            return false;
        }
        let Ok(root) = source
            .url()
            .and_then(|s| url::Url::parse(&s).map_err(|_| "URL non valido".into()))
        else {
            return false;
        };
        let auth = AuthStore::default();
        if auth.insert(source).is_err() {
            return false;
        }
        let mut session = network::ListingSession::with_timeout(Duration::from_secs(2));
        let mut matches = 0;
        let mut matching_groups = 0;
        for (parent, expected) in &self.groups {
            if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
                return false;
            }
            // LIST fallback can perform two requests: bound both by the shared budget.
            session.set_timeout(
                deadline
                    .saturating_duration_since(Instant::now())
                    .div_f32(2.0)
                    .min(Duration::from_secs(2))
                    .max(Duration::from_millis(1)),
            );
            let Ok(directory) = root.join(parent) else {
                return false;
            };
            let Ok(entries) = session.list(directory.as_str(), &auth, cancel) else {
                return false;
            };
            let count = entries
                .iter()
                .filter(|e| !e.directory && expected.contains(&e.name))
                .count();
            matches += count;
            matching_groups += usize::from(count > 0);
        }
        let samples: usize = self.groups.iter().map(|(_, names)| names.len()).sum();
        matches >= samples.min(3) && matching_groups == self.groups.len()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    NotFound,
    Ambiguous,
    NoReference,
    NoLan,
    Cancelled,
}
pub enum Outcome {
    Found(Box<FtpSource>),
    Unchanged,
    Missing(Reason),
}
fn select(
    source: &FtpSource,
    candidates: &[Ipv4Addr],
    fingerprint: &Fingerprint,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Option<FtpSource>, Reason> {
    if candidates.len() > 4 {
        return Err(Reason::Ambiguous);
    }
    let mut found = None;
    for ip in candidates {
        if cancel.load(Ordering::Relaxed) {
            return Err(Reason::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Reason::NotFound);
        }
        let mut candidate = source.clone();
        candidate.host = ip.to_string();
        if fingerprint.matches(&candidate, cancel, deadline) {
            if found.is_some() {
                return Err(Reason::Ambiguous);
            }
            found = Some(candidate);
        }
    }
    Ok(found)
}
const LAN_PROBE_TIMEOUT: Duration = Duration::from_millis(700);
const MAX_PROBES: usize = 8;
const SEARCH_BUDGET: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScanPace {
    #[default]
    Fast,
    Conservative,
    Ms50,
    Ms5,
}
impl ScanPace {
    pub fn interval(self) -> Duration {
        Duration::from_millis(match self {
            Self::Conservative => 1000,
            Self::Fast => 350,
            Self::Ms50 => 50,
            Self::Ms5 => 5,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Conservative => "1 s",
            Self::Fast => "350 ms",
            Self::Ms50 => "50 ms",
            Self::Ms5 => "5 ms",
        }
    }
}

pub struct SearchNetwork {
    pub lans: Vec<Lan>,
    pub pace: ScanPace,
}

// One shared budget across priority and fallback, never a second burst per phase.
struct ProbeBudget {
    remaining: usize,
    pace: ScanPace,
    last_probe: Option<Instant>,
    deadline: Instant,
}
impl ProbeBudget {
    fn new(pace: ScanPace) -> Self {
        let now = Instant::now();
        Self {
            remaining: MAX_PROBES,
            pace,
            last_probe: None,
            deadline: now + SEARCH_BUDGET,
        }
    }
    fn probe(&mut self, hosts: &[Ipv4Addr], port: u16, cancel: &AtomicBool) -> Vec<Ipv4Addr> {
        self.probe_with(hosts, port, cancel, |ip, port, timeout| {
            TcpStream::connect_timeout(&SocketAddr::from((ip, port)), timeout).is_ok()
        })
    }
    fn probe_with(
        &mut self,
        hosts: &[Ipv4Addr],
        port: u16,
        cancel: &AtomicBool,
        mut connect: impl FnMut(Ipv4Addr, u16, Duration) -> bool,
    ) -> Vec<Ipv4Addr> {
        let mut open = Vec::new();
        for ip in hosts {
            if self.remaining == 0
                || Instant::now() >= self.deadline
                || cancel.load(Ordering::Relaxed)
            {
                break;
            }
            let next = self.last_probe.map(|start| start + self.pace.interval());
            // Small sleep slices keep cancellation responsive. No detached socket workers.
            while let Some(next) = next.filter(|next| Instant::now() < *next) {
                if cancel.load(Ordering::Relaxed) || Instant::now() >= self.deadline {
                    return open;
                }
                std::thread::sleep(
                    next.saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(20)),
                );
            }
            let now = Instant::now();
            if now >= self.deadline {
                break;
            }
            self.remaining -= 1;
            self.last_probe = Some(now);
            if connect(
                *ip,
                port,
                (self.deadline - now).min(if self.pace == ScanPace::Conservative {
                    self.pace.interval()
                } else {
                    LAN_PROBE_TIMEOUT
                }),
            ) {
                open.push(*ip);
            }
        }
        open
    }
}

fn priority_hosts(
    source: &FtpSource,
    networks: &[Lan],
    neighbors: &[(Ipv4Addr, String)],
) -> Vec<Ipv4Addr> {
    let mut seen = HashSet::new();
    neighbors
        .iter()
        .filter(|(ip, mac)| {
            networks.iter().any(|lan| lan.has_host(*ip)) && source.hardware_ids.contains(mac)
        })
        .map(|(ip, _)| *ip)
        .chain(
            std::iter::once(&source.host)
                .chain(source.recent_hosts.iter().rev())
                .filter_map(|s| s.parse::<Ipv4Addr>().ok()),
        )
        .filter(|ip| on_link(*ip, networks) && seen.insert(*ip))
        .take(4)
        .collect()
}
fn on_link(ip: Ipv4Addr, networks: &[Lan]) -> bool {
    networks.iter().any(|lan| lan.has_host(ip)) && !networks.iter().any(|lan| lan.address == ip)
}

fn fallback_hosts(
    priority: &[Ipv4Addr],
    networks: &[Lan],
    neighbors: &[(Ipv4Addr, String)],
) -> Vec<Ipv4Addr> {
    let mut seen = HashSet::new();
    // A nearby address must still belong to the same actual subnet.
    let nearby = (1u32..=8).flat_map(|distance| {
        priority.iter().flat_map(move |ip| {
            let address = u32::from(*ip);
            [address.checked_sub(distance), address.checked_add(distance)]
                .into_iter()
                .flatten()
                .map(Ipv4Addr::from)
                .filter(move |candidate| {
                    networks
                        .iter()
                        .any(|lan| lan.has_host(*ip) && lan.has_host(*candidate))
                })
        })
    });
    // Share the blind probes between Wi-Fi and Ethernet when both are attached.
    let mut ranges: VecDeque<_> = networks.iter().map(Lan::hosts).collect();
    let blind = nearby.chain(std::iter::from_fn(|| {
        while let Some(mut range) = ranges.pop_front() {
            if let Some(ip) = range.next() {
                ranges.push_back(range);
                return Some(ip);
            }
        }
        None
    }));
    // Prefer addresses already seen on-link, even when DHCP moved the phone
    // far from its last address. Keep blind probes for an empty or stale ARP cache.
    let mut observed: Vec<_> = neighbors
        .iter()
        .map(|(ip, _)| *ip)
        .filter(|ip| !priority.contains(ip) && on_link(*ip, networks))
        .collect();
    observed.sort_unstable_by_key(|ip| {
        let value = u32::from(*ip);
        let distance = priority
            .iter()
            .map(|known| u32::from(*known))
            .filter(|known| {
                networks
                    .iter()
                    .any(|lan| lan.has_host((*known).into()) && lan.has_host(*ip))
            })
            .map(|known| known.abs_diff(value))
            .min()
            .unwrap_or(u32::MAX);
        (distance, value)
    });
    observed.dedup();
    let observed_set: HashSet<_> = observed.iter().copied().collect();
    let mut observed = observed.into_iter();
    let mut blind = blind.filter(|ip| !observed_set.contains(ip));
    let mut prefer_observed = true;
    std::iter::from_fn(|| {
        prefer_observed = !prefer_observed;
        if prefer_observed {
            blind.next().or_else(|| observed.next())
        } else {
            observed.next().or_else(|| blind.next())
        }
    })
    .filter(|ip| !priority.contains(ip) && on_link(*ip, networks) && seen.insert(*ip))
    .take(4094)
    .collect()
}
// Rotate small pages between attempts so a missing host does not trigger a full sweep.
// Session-local bookkeeping only; tags and persisted source settings are untouched.
fn next_page(id: &str, hosts: &[Ipv4Addr], limit: usize) -> Vec<Ipv4Addr> {
    static CURSORS: OnceLock<Mutex<BTreeMap<String, usize>>> = OnceLock::new();
    if hosts.is_empty() {
        return Vec::new();
    }
    let Ok(mut cursors) = CURSORS.get_or_init(Default::default).lock() else {
        return Vec::new();
    };
    if cursors.len() >= 64 && !cursors.contains_key(id) {
        cursors.clear();
    }
    let cursor = cursors.entry(id.to_owned()).or_default();
    let page = hosts
        .iter()
        .cycle()
        .skip(*cursor % hosts.len())
        .take(limit.min(hosts.len()))
        .copied()
        .collect();
    *cursor = (*cursor + limit.min(hosts.len())) % hosts.len();
    page
}
pub fn find(
    source: &FtpSource,
    known: &HashSet<String>,
    networks: &[Lan],
    pace: ScanPace,
    cancel: &AtomicBool,
) -> Outcome {
    if cancel.load(Ordering::Relaxed) {
        return Outcome::Missing(Reason::Cancelled);
    }
    if networks.is_empty() {
        return Outcome::Missing(Reason::NoLan);
    }
    let Ok(root) = source.url() else {
        return Outcome::Missing(Reason::NotFound);
    };
    let fingerprint = Fingerprint::from_library(&root, known.iter());
    if fingerprint.groups.is_empty() {
        return Outcome::Missing(Reason::NoReference);
    }
    let Ok(port) = source.port.parse::<u16>() else {
        return Outcome::Missing(Reason::NotFound);
    };
    let observed = neighbors();
    let priority = priority_hosts(source, networks, &observed);
    let mut budget = ProbeBudget::new(pace);
    let prioritized = budget.probe(&priority, port, cancel);
    let first = select(source, &prioritized, &fingerprint, cancel, budget.deadline);
    let selected = match first {
        Ok(Some(found)) => Ok(Some(found)),
        Err(error) => Err(error),
        Ok(None) => {
            let hosts = fallback_hosts(&priority, networks, &observed);
            let scope = format!("{}:{networks:?}", source.id);
            let page = next_page(&scope, &hosts, budget.remaining);
            let candidates = budget.probe(&page, port, cancel);
            select(source, &candidates, &fingerprint, cancel, budget.deadline)
        }
    };
    match selected {
        Ok(Some(mut found)) => {
            if found.host == source.host {
                return Outcome::Unchanged;
            }
            for host in [&source.host, &found.host] {
                if !found.recent_hosts.contains(host) {
                    found.recent_hosts.push(host.clone());
                }
            }
            if found.recent_hosts.len() > 4 {
                found.recent_hosts.drain(..found.recent_hosts.len() - 4);
            }
            if let Some((_, address)) = neighbors()
                .into_iter()
                .find(|(ip, _)| ip.to_string() == found.host)
                && !found.hardware_ids.contains(&address)
            {
                found.hardware_ids.push(address);
            }
            if found.hardware_ids.len() > 4 {
                found.hardware_ids.remove(0);
            }
            Outcome::Found(Box::new(found))
        }
        Ok(None) => Outcome::Missing(if cancel.load(Ordering::Relaxed) {
            Reason::Cancelled
        } else {
            Reason::NotFound
        }),
        Err(error) => Outcome::Missing(error),
    }
}
pub fn start(
    source: FtpSource,
    known: HashSet<String>,
    network: SearchNetwork,
    cache: std::path::PathBuf,
    cancel: Arc<AtomicBool>,
    request: u64,
    (tx, ctx): (
        std::sync::mpsc::Sender<crate::jobs::Job>,
        eframe::egui::Context,
    ),
) {
    std::thread::spawn(move || {
        let current = lans();
        let mut outcome = if current == network.lans {
            find(&source, &known, &current, network.pace, &cancel)
        } else {
            Outcome::Missing(Reason::Cancelled)
        };
        if network.lans != lans() || cancel.load(Ordering::Relaxed) {
            outcome = Outcome::Missing(Reason::Cancelled);
        }
        if let Outcome::Found(ref found) = outcome {
            crate::sync::rebase_index(&cache, &source, found);
        }
        let _ = tx.send(crate::jobs::Job::Rediscovered { request, outcome });
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_uses_attached_subnets_and_ignores_addresses_from_the_old_network() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        let wifi = Lan::new(ip("10.50.8.44"), ip("255.255.254.0")).unwrap();
        let ethernet = Lan::new(ip("192.168.10.44"), ip("255.255.255.0")).unwrap();
        let source = FtpSource {
            host: "10.50.16.57".into(),
            recent_hosts: vec!["10.50.8.254".into(), "10.50.9.255".into(), "8.8.8.8".into()],
            hardware_ids: vec!["02:00:00:00:00:01".into()],
            ..FtpSource::smartphone()
        };
        let networks = [wifi, ethernet];
        let observed = [(ip("10.50.16.54"), source.hardware_ids[0].clone())];
        let priority = priority_hosts(&source, &networks, &observed);
        assert_eq!(priority, [ip("10.50.8.254")]);
        let hosts = fallback_hosts(&priority, &networks, &observed);
        assert!(hosts[..4].contains(&ip("10.50.9.0")));
        assert!(hosts.iter().all(|&ip| on_link(ip, &networks)));
        for excluded in [
            "10.50.8.0",
            "10.50.9.255",
            "10.50.8.44",
            "10.50.16.54",
            "10.50.16.57",
        ] {
            assert!(!hosts.contains(&ip(excluded)));
        }
        assert_eq!(hosts.iter().collect::<HashSet<_>>().len(), hosts.len());
        assert_eq!(
            next_page("lan-page-regression", &hosts, MAX_PROBES),
            hosts[..MAX_PROBES]
        );
        assert_eq!(
            next_page("lan-page-regression", &hosts, MAX_PROBES),
            hosts[MAX_PROBES..2 * MAX_PROBES]
        );

        let hosts = fallback_hosts(&[], &networks, &[]);
        assert_eq!(hosts[..2], [ip("10.50.8.45"), ip("192.168.10.45")]);
    }
    #[test]
    fn observed_dhcp_move_gets_a_probe_without_starving_unseen_hosts() {
        let ip = |n| Ipv4Addr::new(192, 168, 1, n);
        let lan = Lan::new(ip(10), Ipv4Addr::new(255, 255, 255, 0)).unwrap();
        let mut phone = FtpSource::smartphone();
        phone.host = ip(53).to_string();
        phone.recent_hosts = [50, 51, 52].map(|n| ip(n).to_string()).to_vec();
        let seen = [(ip(92), "02:00:00:00:00:01".into())];
        let priority = priority_hosts(&phone, std::slice::from_ref(&lan), &seen);
        let fallback = fallback_hosts(&priority, std::slice::from_ref(&lan), &seen);
        assert_eq!(fallback[0], ip(92));
        assert_eq!(fallback[1], ip(54));
        assert_eq!(fallback.iter().filter(|&&addr| addr == ip(92)).count(), 1);

        phone.hardware_ids.push(seen[0].1.clone());
        let priority = priority_hosts(&phone, &[lan], &seen);
        assert_eq!(priority[0], ip(92));
    }
    #[test]
    fn probes_are_serial_paced_share_a_cap_and_stop_on_cancellation_or_deadline() {
        let port = 8736;
        let cancel = AtomicBool::new(false);
        let mut budget = ProbeBudget::new(ScanPace::Conservative);
        budget.remaining = 2;
        let now = Instant::now();
        assert_eq!(
            budget
                .probe_with(&[Ipv4Addr::LOCALHOST], port, &cancel, |_, _, _| true)
                .len(),
            1
        );
        assert_eq!(
            budget
                .probe_with(&[Ipv4Addr::LOCALHOST; 3], port, &cancel, |_, _, _| { true })
                .len(),
            1
        );
        assert!(now.elapsed() >= ScanPace::Conservative.interval());
        assert_eq!(budget.remaining, 0);
        assert!(
            budget
                .probe_with(&[Ipv4Addr::LOCALHOST], port, &cancel, |_, _, _| true)
                .is_empty()
        );
        let mut budget = ProbeBudget::new(ScanPace::Conservative);
        budget.deadline = Instant::now();
        assert!(
            budget
                .probe_with(&[Ipv4Addr::LOCALHOST], port, &cancel, |_, _, _| true)
                .is_empty()
        );
        let mut budget = ProbeBudget::new(ScanPace::Conservative);
        budget.last_probe = Some(Instant::now());
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(40));
                cancel.store(true, Ordering::Relaxed);
            });
            let now = Instant::now();
            assert!(
                budget
                    .probe_with(&[Ipv4Addr::LOCALHOST], port, &cancel, |_, _, _| true)
                    .is_empty()
            );
            assert!(now.elapsed() < Duration::from_millis(500));
        });
        assert!(matches!(
            select(
                &FtpSource::smartphone(),
                &[Ipv4Addr::LOCALHOST],
                &Fingerprint::default(),
                &AtomicBool::new(false),
                Instant::now()
            ),
            Err(Reason::NotFound)
        ));
    }
    #[test]
    fn scan_intervals_are_persistable_and_applied_to_serial_probes() {
        for (pace, millis) in [
            (ScanPace::Ms5, 5),
            (ScanPace::Ms50, 50),
            (ScanPace::Fast, 350),
            (ScanPace::Conservative, 1000),
        ] {
            let encoded = serde_json::to_string(&pace).unwrap();
            let decoded: ScanPace = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.interval(), Duration::from_millis(millis));
            let cancel = AtomicBool::new(false);
            let mut budget = ProbeBudget::new(decoded);
            let mut attempts = Vec::new();
            budget.probe_with(&[Ipv4Addr::LOCALHOST; 2], 8736, &cancel, |_, _, timeout| {
                attempts.push((Instant::now(), timeout));
                false
            });
            assert_eq!(attempts.len(), 2);
            assert!(attempts[1].0 - attempts[0].0 + Duration::from_millis(1) >= decoded.interval());
            assert_eq!(
                attempts[0].1,
                if millis == 1000 {
                    decoded.interval()
                } else {
                    LAN_PROBE_TIMEOUT
                }
            );
        }
    }
    #[test]
    fn lan_scope_is_private_and_contiguous_and_arp_normalizes_mac() {
        for (ip, mask) in [
            ("8.8.8.8", "255.255.255.0"),
            ("127.0.0.1", "255.255.255.0"),
            ("10.0.0.1", "254.0.0.0"),
            ("172.17.0.1", "255.224.0.0"),
            ("192.168.1.2", "255.0.255.0"),
        ] {
            assert!(Lan::new(ip.parse().unwrap(), mask.parse().unwrap()).is_none());
        }
        let lan = Lan::new(
            "10.42.16.4".parse().unwrap(),
            "255.255.240.0".parse().unwrap(),
        )
        .unwrap();
        let hosts: Vec<_> = lan.hosts().collect();
        assert_eq!(hosts.len(), 4093);
        assert!(hosts.contains(&"10.42.31.254".parse().unwrap()));
        assert!(!hosts.contains(&lan.address));
        assert!(!lan.has_host("10.42.32.1".parse().unwrap()));
        let wide = Lan::new("10.42.16.4".parse().unwrap(), "255.0.0.0".parse().unwrap()).unwrap();
        assert_eq!(fallback_hosts(&[], &[wide], &[]).len(), 4094);
        let found = parse_neighbors(
            "? (192.168.1.7) at a:2:3:4:5:6 on en0 ifscope [ethernet]\n? (192.168.1.8) at (incomplete) on en0\n? (192.168.1.255) at ff:ff:ff:ff:ff:ff on en0",
        );
        assert_eq!(
            found,
            [("192.168.1.7".parse().unwrap(), "0a:02:03:04:05:06".into())]
        );
    }
    #[test]
    fn reference_uses_only_the_selected_root_and_legacy_phone_defaults_are_optional() {
        let root = "ftp://old:8736/device/Music/";
        let fingerprint = Fingerprint::from_library(
            root,
            [
                format!("{root}Album%20%C3%A8/01.flac"),
                "ftp://other/Music/foreign.flac".into(),
            ]
            .iter(),
        );
        assert_eq!(fingerprint.groups.len(), 1);
        assert_eq!(fingerprint.groups[0].0, "Album%20%C3%A8/");
        assert!(fingerprint.groups[0].1.contains("01.flac"));
        let mut phone = FtpSource::smartphone();
        phone.host = "phone.local".into();
        assert!(phone.discovery_enabled());
        phone.rediscover = Some(false);
        assert!(!phone.discovery_enabled());
        phone.password = "synthetic-secret".into();
        let mut persisted = serde_json::to_value(&phone).unwrap();
        assert!(!persisted.to_string().contains("synthetic-secret"));
        persisted.as_object_mut().unwrap().remove("rediscover");
        persisted.as_object_mut().unwrap().remove("recent_hosts");
        persisted.as_object_mut().unwrap().remove("hardware_ids");
        assert!(
            serde_json::from_value::<FtpSource>(persisted)
                .unwrap()
                .discovery_enabled()
        );
        let nas = FtpSource {
            name: "NAS".into(),
            root: "/Music/".into(),
            ..Default::default()
        };
        assert!(!nas.discovery_enabled());
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            find(&phone, &HashSet::new(), &[], ScanPace::Fast, &cancel),
            Outcome::Missing(Reason::Cancelled)
        ));
    }
    #[test]
    #[ignore = "requires scripts/test_ftp.py fixture server"]
    fn live_ftp_rediscovery_verifies_library_rejects_wrong_credentials_and_ambiguity() {
        let base = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let mut actual = FtpSource::smartphone();
        actual.set_url(&base).unwrap();
        actual.id = "fixture-identity".into();
        actual.username = "fixture".into();
        actual.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&actual).unwrap();
        let report = crate::sync::scan(
            crate::sync::Target::Ftp {
                id: actual.id.clone(),
                root: base.clone(),
            },
            &auth,
            &HashSet::new(),
            &mut crate::sync::Index::default(),
            true,
            &AtomicBool::new(false),
        );
        assert_eq!(report.stats.errors, 0);
        let mut old = actual.clone();
        old.host = "127.0.0.2".into();
        let old_root = old.url().unwrap();
        let known: HashSet<_> = report
            .tracks
            .iter()
            .map(|t| network::rebase(&t.source, &base, &old_root).unwrap())
            .collect();
        let fingerprint = Fingerprint::from_library(&old_root, known.iter());
        let cancel = AtomicBool::new(false);
        let metrics_path = std::path::PathBuf::from(std::env::var("AUREA_TEST_FTP_ROOT").unwrap())
            .join(".sync-metrics");
        let downloads = || {
            std::fs::read_to_string(&metrics_path)
                .unwrap_or_default()
                .matches("download")
                .count()
        };
        let before = downloads();
        let net = Lan {
            address: "127.0.0.6".parse().unwrap(),
            mask: "255.255.255.248".parse().unwrap(),
        };
        let outcome = find(
            &old,
            &known,
            std::slice::from_ref(&net),
            ScanPace::Fast,
            &cancel,
        );
        let Outcome::Found(found) = outcome else {
            panic!("changed IP was not rediscovered");
        };
        assert_eq!(found.host, actual.host);
        assert_eq!(found.id, old.id);
        assert_eq!(found.username, old.username);
        assert_eq!(found.password, old.password);
        assert_eq!(found.port, old.port);
        assert_eq!(found.root, old.root);
        assert!(found.recent_hosts.contains(&actual.host));
        assert_eq!(downloads(), before);
        let ip = actual.host.parse::<Ipv4Addr>().unwrap();
        let mut wrong = old.clone();
        wrong.password = "incorrect-fixture".into();
        assert!(
            select(
                &wrong,
                &[ip],
                &fingerprint,
                &cancel,
                Instant::now() + SEARCH_BUDGET
            )
            .unwrap()
            .is_none()
        );
        let unrelated = Fingerprint::from_library(
            &old_root,
            [format!("{old_root}Absent/Not-this-library.flac")].iter(),
        );
        assert!(
            select(
                &old,
                &[ip],
                &unrelated,
                &cancel,
                Instant::now() + SEARCH_BUDGET
            )
            .unwrap()
            .is_none()
        );
        assert!(matches!(
            select(
                &old,
                &[ip, ip],
                &fingerprint,
                &cancel,
                Instant::now() + SEARCH_BUDGET
            ),
            Err(Reason::Ambiguous)
        ));
        assert!(matches!(
            find(&old, &HashSet::new(), &[net], ScanPace::Fast, &cancel),
            Outcome::Missing(Reason::NoReference)
        ));
        assert_eq!(downloads(), before);
        println!(
            "Rediscovery: changed loopback IP found on the configured port; source ID, root and credentials retained; wrong library, wrong password, ambiguity and cancellation rejected; no media downloads"
        );
    }
}
