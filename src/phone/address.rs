//! Resolve Bonjour names with the OS mDNS API, not the unicast curl resolver.
use std::{
    net::Ipv4Addr,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn local_ipv4(host: &str, cancel: &AtomicBool) -> Vec<Ipv4Addr> {
    if cancel.load(Ordering::Relaxed) {
        return vec![];
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return vec![ip].into_iter().filter(allowed).collect();
    }
    if !host
        .trim_end_matches('.')
        .to_ascii_lowercase()
        .ends_with(".local")
        || !host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
    {
        return vec![];
    }
    #[cfg(target_os = "macos")]
    {
        native::resolve(host, cancel)
    }
    #[cfg(not(target_os = "macos"))]
    {
        super::mdns::resolve(host, cancel)
    }
}
#[cfg(target_os = "macos")]
pub(super) fn service_endpoint(name: &str, cancel: &AtomicBool) -> Option<(String, u16)> {
    if cancel.load(Ordering::Relaxed) || !name.starts_with("MusicCraft-") || name.len() > 80 {
        return None;
    }
    native::endpoint(name, cancel)
}
fn allowed(ip: &Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback()
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::{
        ffi::{CString, c_char, c_int, c_void},
        ptr,
        time::{Duration, Instant},
    };
    type Service = *mut c_void;
    #[repr(C)]
    struct Address {
        len: u8,
        family: u8,
        port: u16,
        ip: u32,
        padding: [u8; 8],
    }
    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: i16,
        revents: i16,
    }
    type Reply = unsafe extern "C" fn(
        Service,
        u32,
        u32,
        i32,
        *const c_char,
        *const Address,
        u32,
        *mut c_void,
    );
    type ServiceReply = unsafe extern "C" fn(
        Service,
        u32,
        u32,
        i32,
        *const c_char,
        *const c_char,
        u16,
        u16,
        *const u8,
        *mut c_void,
    );
    #[link(name = "System")]
    unsafe extern "C" {
        fn DNSServiceResolve(
            service: *mut Service,
            flags: u32,
            interface: u32,
            name: *const c_char,
            kind: *const c_char,
            domain: *const c_char,
            reply: ServiceReply,
            context: *mut c_void,
        ) -> i32;
        fn DNSServiceGetAddrInfo(
            service: *mut Service,
            flags: u32,
            interface: u32,
            protocol: u32,
            host: *const c_char,
            reply: Reply,
            context: *mut c_void,
        ) -> i32;
        fn DNSServiceRefSockFD(service: Service) -> c_int;
        fn DNSServiceProcessResult(service: Service) -> i32;
        fn DNSServiceRefDeallocate(service: Service);
        fn poll(fds: *mut PollFd, count: u32, timeout: c_int) -> c_int;
    }
    struct Owned(Service);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { DNSServiceRefDeallocate(self.0) }
            }
        }
    }
    unsafe extern "C" fn reply(
        _: Service,
        flags: u32,
        _: u32,
        error: i32,
        _: *const c_char,
        address: *const Address,
        _: u32,
        context: *mut c_void,
    ) {
        if error != 0 || flags & 2 == 0 || address.is_null() || context.is_null() {
            return;
        }
        // DNS-SD supplies sockaddr; IPv4 has Darwin AF_INET=2 and sizeof(sockaddr_in)=16.
        let address = unsafe { &*address };
        if address.family != 2 || address.len < 16 {
            return;
        }
        let ip = Ipv4Addr::from(u32::from_be(address.ip));
        if allowed(&ip) {
            // ProcessResult invokes callbacks synchronously. This Vec outlives Owned,
            // and deallocation cancels all further callbacks before it is dropped.
            let found = unsafe { &mut *context.cast::<Vec<Ipv4Addr>>() };
            if found.len() < 16 && !found.contains(&ip) {
                found.push(ip)
            }
        }
    }
    unsafe extern "C" fn service_reply(
        _: Service,
        _: u32,
        _: u32,
        error: i32,
        _: *const c_char,
        host: *const c_char,
        port: u16,
        _: u16,
        _: *const u8,
        context: *mut c_void,
    ) {
        if error != 0 || host.is_null() || context.is_null() {
            return;
        }
        // DNS-SD owns this NUL-terminated callback string; copy it before returning.
        let Ok(host) = unsafe { std::ffi::CStr::from_ptr(host) }.to_str() else {
            return;
        };
        let host = host.trim_end_matches('.');
        let port = u16::from_be(port);
        if port > 0
            && host.to_ascii_lowercase().ends_with(".local")
            && host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
        {
            unsafe { *context.cast::<Option<(String, u16)>>() = Some((host.to_owned(), port)) };
        }
    }
    pub(super) fn endpoint(name: &str, cancel: &AtomicBool) -> Option<(String, u16)> {
        let name = CString::new(name).ok()?;
        let mut found: Option<(String, u16)> = None;
        let mut service = Owned(ptr::null_mut());
        let code = unsafe {
            DNSServiceResolve(
                &mut service.0,
                0,
                0,
                name.as_ptr(),
                c"_musiccraft._tcp".as_ptr(),
                c"local.".as_ptr(),
                service_reply,
                (&mut found as *mut Option<(String, u16)>).cast(),
            )
        };
        if code != 0 || service.0.is_null() {
            service.0 = ptr::null_mut();
            return None;
        }
        let fd = unsafe { DNSServiceRefSockFD(service.0) };
        if fd < 0 {
            return None;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cancel.load(Ordering::Relaxed) && Instant::now() < deadline {
            let mut ready = PollFd {
                fd,
                events: 1,
                revents: 0,
            };
            let n = unsafe { poll(&mut ready, 1, 50) };
            if n < 0 {
                break;
            }
            if ready.revents & 1 != 0 {
                if unsafe { DNSServiceProcessResult(service.0) } != 0 {
                    break;
                }
                if found.is_some() {
                    break;
                }
            } else if ready.revents != 0 {
                break;
            }
        }
        drop(service);
        found
    }
    pub(super) fn resolve(host: &str, cancel: &AtomicBool) -> Vec<Ipv4Addr> {
        let Ok(host) = CString::new(host) else {
            return vec![];
        };
        let mut found: Vec<Ipv4Addr> = vec![];
        let mut service = Owned(ptr::null_mut());
        // ANY interface, IPv4 only, no global DNS settings or hosts-file changes.
        let code = unsafe {
            DNSServiceGetAddrInfo(
                &mut service.0,
                0,
                0,
                1,
                host.as_ptr(),
                reply,
                (&mut found as *mut Vec<Ipv4Addr>).cast(),
            )
        };
        if code != 0 || service.0.is_null() {
            service.0 = ptr::null_mut();
            return vec![];
        }
        let fd = unsafe { DNSServiceRefSockFD(service.0) };
        if fd < 0 {
            return vec![];
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cancel.load(Ordering::Relaxed) && Instant::now() < deadline {
            let mut ready = PollFd {
                fd,
                events: 1,
                revents: 0,
            };
            let n = unsafe { poll(&mut ready, 1, 50) };
            if n < 0 {
                break;
            }
            if ready.revents & 1 != 0 {
                if unsafe { DNSServiceProcessResult(service.0) } != 0 {
                    break;
                }
                if !found.is_empty() {
                    break;
                }
            } else if ready.revents != 0 {
                break;
            }
        }
        drop(service);
        found.sort();
        found
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit native Bonjour probe; supply a service name and private output file"]
    fn live_native_bonjour_endpoint_is_numeric() {
        let name = std::env::var("MUSICCRAFT_QA_DNS_SERVICE").unwrap();
        let cancel = AtomicBool::new(false);
        let (host, port) =
            service_endpoint(&name, &cancel).expect("Bonjour service lookup did not answer");
        let ips = local_ipv4(&host, &cancel);
        if let Some(output) = std::env::var_os("MUSICCRAFT_QA_DNS_OUTPUT") {
            std::fs::write(output,serde_json::json!({"service_host":host,"port":port,"addresses":ips.iter().map(ToString::to_string).collect::<Vec<_>>()} ).to_string()).unwrap();
        }
        assert!(!ips.is_empty(), "Bonjour supplied no local IPv4 address");
    }
    #[test]
    fn accepts_only_local_addresses_and_honors_cancellation() {
        let cancel = AtomicBool::new(false);
        assert_eq!(
            local_ipv4("192.168.2.3", &cancel),
            vec![Ipv4Addr::new(192, 168, 2, 3)]
        );
        for bad in [
            "8.8.8.8",
            "example.com",
            "host.local/elsewhere",
            "bad\0.local",
            "192.168.1.2:8737",
        ] {
            assert!(local_ipv4(bad, &cancel).is_empty())
        }
        cancel.store(true, Ordering::Relaxed);
        assert!(local_ipv4("127.0.0.1", &cancel).is_empty());
        assert!(local_ipv4("phone.local", &cancel).is_empty());
    }
}
