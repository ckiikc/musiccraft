//! In-process DNS-SD for Windows/Linux. Discovery is untrusted: TLS/SRP checks remain mandatory.
use super::pairing_code::Nearby;
use mdns_sd::{HostnameResolutionEvent, ServiceDaemon, ServiceEvent};
use std::{
    net::Ipv4Addr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
const KIND: &str = "_musiccraft._tcp.local.";
struct Session(ServiceDaemon);
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}
fn allowed(ip: &Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback()
}
fn candidate(name: &str, port: u16, ips: impl IntoIterator<Item = Ipv4Addr>) -> Vec<Nearby> {
    let Some(name) = name.strip_suffix(&format!(".{KIND}")) else {
        return vec![];
    };
    if !name.starts_with("MusicCraft-") || name.len() > 80 || port == 0 {
        return vec![];
    }
    ips.into_iter()
        .filter(allowed)
        .take(8)
        .map(|ip| Nearby {
            name: name.into(),
            host: ip.to_string(),
            port,
        })
        .collect()
}
pub(super) fn nearby(cancel: &AtomicBool) -> Vec<Nearby> {
    if cancel.load(Ordering::Relaxed) {
        return vec![];
    }
    let Ok(daemon) = ServiceDaemon::new().map(Session) else {
        return vec![];
    };
    let Ok(events) = daemon.0.browse(KIND) else {
        return vec![];
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut peers = Vec::new();
    while Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
        if let Ok(ServiceEvent::ServiceResolved(service)) =
            events.recv_timeout(Duration::from_millis(80))
        {
            peers.extend(candidate(
                service.get_fullname(),
                service.get_port(),
                service.get_addresses_v4(),
            ));
            peers.sort_by(|a, b| (&a.name, &a.host, a.port).cmp(&(&b.name, &b.host, b.port)));
            peers.dedup();
            peers.truncate(64);
        }
    }
    let _ = daemon.0.stop_browse(KIND);
    if cancel.load(Ordering::Relaxed) {
        vec![]
    } else {
        peers
    }
}
pub(super) fn resolve(host: &str, cancel: &AtomicBool) -> Vec<Ipv4Addr> {
    if cancel.load(Ordering::Relaxed) {
        return vec![];
    }
    let Ok(daemon) = ServiceDaemon::new().map(Session) else {
        return vec![];
    };
    let host = format!("{}.", host.trim_end_matches('.'));
    let Ok(events) = daemon.0.resolve_hostname(&host, Some(2500)) else {
        return vec![];
    };
    let deadline = Instant::now() + Duration::from_millis(2600);
    let mut ips = Vec::new();
    while Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
        if let Ok(HostnameResolutionEvent::AddressesFound(_, addresses)) =
            events.recv_timeout(Duration::from_millis(80))
        {
            ips.extend(
                addresses
                    .into_iter()
                    .filter_map(|ip| match ip.to_ip_addr() {
                        std::net::IpAddr::V4(ip) if allowed(&ip) => Some(ip),
                        _ => None,
                    }),
            );
            if !ips.is_empty() {
                break;
            }
        }
    }
    let _ = daemon.0.stop_resolve_hostname(&host);
    ips.sort();
    ips.dedup();
    ips.truncate(8);
    if cancel.load(Ordering::Relaxed) {
        vec![]
    } else {
        ips
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dns_sd_accepts_only_bounded_local_musiccraft_candidates() {
        let ips = [Ipv4Addr::new(192, 168, 42, 7), Ipv4Addr::new(8, 8, 8, 8)];
        let peers = candidate("MusicCraft-fixture._musiccraft._tcp.local.", 8737, ips);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].host, "192.168.42.7");
        assert!(candidate("Other._musiccraft._tcp.local.", 8737, ips).is_empty());
        assert!(candidate("MusicCraft-fixture._musiccraft._tcp.local.", 0, ips).is_empty());
        assert!(nearby(&AtomicBool::new(true)).is_empty());
        assert!(resolve("fixture.local", &AtomicBool::new(true)).is_empty());
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    #[test]
    #[ignore = "requires isolated multicast-capable LAN interfaces; publishes a synthetic service only"]
    fn live_dns_sd_discovers_a_synthetic_musiccraft_service() {
        let address = if_addrs::get_if_addrs()
            .unwrap()
            .into_iter()
            .find_map(|interface| match interface.addr {
                if_addrs::IfAddr::V4(v) if v.ip.is_private() => Some(v.ip),
                _ => None,
            })
            .expect("isolated private interface");
        let daemon = Session(ServiceDaemon::new().unwrap());
        let name = format!("MusicCraft-qa{:08x}", rand::random::<u32>());
        let service = mdns_sd::ServiceInfo::new(
            KIND,
            &name,
            "musiccraft-qa-fixture.local.",
            address.to_string().as_str(),
            38737,
            std::collections::HashMap::<String, String>::new(),
        )
        .unwrap();
        daemon.0.register(service).unwrap();
        let peers = nearby(&AtomicBool::new(false));
        assert!(
            peers.iter().any(|peer| peer.name == name
                && peer.host == address.to_string()
                && peer.port == 38737),
            "synthetic service not discovered"
        );
    }
}
