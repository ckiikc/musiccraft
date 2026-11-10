//! SRP-6a (RFC 5054 3072-bit group, SHA-256) using Bouncy Castle padded evidence.
//! Bootstrap TLS carries PAKE messages only. The returned identity/token is AES-GCM protected.
use super::*;
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use num_bigint::BigUint;
use sha2::{Digest, Sha256};
use srp::{client::SrpClient, groups::G_3072};
use subtle::ConstantTimeEq;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nearby {
    pub name: String,
    pub host: String,
    pub port: u16,
}
fn dns_lines(args: Vec<String>, cancel: &AtomicBool, seconds: u64) -> Vec<String> {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        sync::mpsc,
        time::Instant,
    };
    let Ok(mut child) = Command::new("/usr/bin/dns-sd")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return vec![];
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return vec![];
    };
    let (tx, rx) = mpsc::sync_channel(32);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut lines = Vec::new();
    while Instant::now() < deadline && !cancel.load(Ordering::Relaxed) && lines.len() < 32 {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(100)) {
            lines.push(line)
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    lines
}
pub fn nearby(cancel: &AtomicBool) -> Vec<Nearby> {
    let lines = dns_lines(
        vec!["-B".into(), "_musiccraft._tcp".into(), "local.".into()],
        cancel,
        3,
    );
    let mut names = HashSet::new();
    for line in lines {
        if line.contains(" Add ")
            && let Some((_, name)) = line.split_once("_musiccraft._tcp.")
        {
            let name = name.trim();
            if name.starts_with("MusicCraft-") && name.len() < 80 {
                names.insert(name.to_owned());
            }
        }
    }
    let mut devices = Vec::new();
    for name in names.into_iter().take(8) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        #[cfg(target_os = "macos")]
        if let Some((host, port)) = super::address::service_endpoint(&name, cancel) {
            for ip in super::address::local_ipv4(&host, cancel) {
                devices.push(Nearby {
                    name: name.clone(),
                    host: ip.to_string(),
                    port,
                });
            }
        }
        #[cfg(not(target_os = "macos"))]
        for line in dns_lines(
            vec![
                "-L".into(),
                name.clone(),
                "_musiccraft._tcp".into(),
                "local.".into(),
            ],
            cancel,
            2,
        ) {
            if let Some((_, address)) = line.split_once("can be reached at ")
                && let Some(endpoint) = address.split_whitespace().next()
                && let Some((host, port)) = endpoint.rsplit_once(':')
            {
                let host = host.trim_end_matches('.');
                if host.ends_with(".local")
                    && host
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
                    && let Ok(port) = port.parse::<u16>()
                    && port > 0
                {
                    devices.push(Nearby {
                        name: name.clone(),
                        host: host.into(),
                        port,
                    });
                    break;
                }
            }
        }
    }
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices.dedup();
    devices
}
fn post(
    peer: &Nearby,
    path: &str,
    body: &serde_json::Value,
    cancel: &AtomicBool,
) -> Result<(u32, serde_json::Value), String> {
    if !(peer.host.ends_with(".local")
        || peer
            .host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|a| a.is_private() || a.is_loopback()))
        || peer.port == 0
    {
        return Err("Invalid local pairing address".into());
    }
    let mut easy = curl::easy::Easy::new();
    let data = serde_json::to_vec(body).map_err(|_| "Invalid pairing request")?;
    let configure = (|| -> Result<(), curl::Error> {
        easy.url(&format!("https://{}:{}{path}", peer.host, peer.port))?;
        easy.post(true)?;
        easy.post_fields_copy(&data)?;
        easy.ssl_verify_peer(false)?;
        easy.ssl_verify_host(false)?;
        easy.follow_location(false)?;
        easy.connect_timeout(Duration::from_secs(3))?;
        easy.timeout(Duration::from_secs(8))?;
        easy.progress(true)?;
        easy.signal(false)?;
        let mut headers = curl::easy::List::new();
        headers.append("Content-Type: application/json")?;
        easy.http_headers(headers)?;
        Ok(())
    })();
    configure.map_err(|_| "Pairing connection unavailable")?;
    let mut bytes = Vec::new();
    {
        let mut transfer = easy.transfer();
        transfer
            .write_function(|data| {
                if bytes.len() + data.len() > 8192 || cancel.load(Ordering::Relaxed) {
                    return Ok(0);
                }
                bytes.extend_from_slice(data);
                Ok(data.len())
            })
            .map_err(|_| "Pairing unavailable")?;
        transfer
            .progress_function(|_, _, _, _| !cancel.load(Ordering::Relaxed))
            .map_err(|_| "Pairing unavailable")?;
        transfer
            .perform()
            .map_err(|_| "Phone unavailable: open pairing on the phone")?;
    }
    let code = easy.response_code().map_err(|_| "Pairing unavailable")?;
    if !matches!(code, 200 | 202) {
        return Err(tr!("Pairing code incorrect or expired. Reopen pairing on the phone.").into());
    }
    Ok((
        code,
        serde_json::from_slice(&bytes).map_err(|_| "Invalid pairing response")?,
    ))
}
fn pad(value: &BigUint) -> Vec<u8> {
    let bytes = value.to_bytes_be();
    let mut out = vec![0; 384];
    out[384 - bytes.len()..].copy_from_slice(&bytes);
    out
}
fn hash(parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p)
    }
    h.finalize().to_vec()
}
fn decoded(value: &serde_json::Value, key: &str, max: usize) -> Result<Vec<u8>, String> {
    let text = value[key].as_str().ok_or("Incomplete pairing response")?;
    if text.len() > max * 2 {
        return Err("Pairing response too large".into());
    }
    let bytes = STANDARD
        .decode(text)
        .map_err(|_| "Invalid pairing encoding")?;
    if bytes.len() > max {
        return Err("Pairing response too large".into());
    }
    Ok(bytes)
}
pub fn pair(
    peer: &Nearby,
    code: &str,
    cancel: &AtomicBool,
    waiting: impl Fn(),
) -> Result<FtpSource, String> {
    if code.len() != 4 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Enter the four digits shown on the phone".into());
    }
    let client = SrpClient::<Sha256>::new(&G_3072);
    let a: [u8; 64] = rand::random();
    let a_pub = BigUint::from_bytes_be(&client.compute_public_ephemeral(&a));
    let ap = pad(&a_pub);
    let (_, challenge) = post(
        peer,
        "/v1/pair/start",
        &serde_json::json!({"a":STANDARD.encode(&ap)}),
        cancel,
    )?;
    let salt = decoded(&challenge, "salt", 32)?;
    if salt.len() != 32 {
        return Err("Invalid pairing salt".into());
    }
    let b = BigUint::from_bytes_be(&decoded(&challenge, "b", 384)?);
    if b == BigUint::default() || b >= G_3072.n {
        return Err("Invalid pairing server value".into());
    }
    let bp = pad(&b);
    let u = BigUint::from_bytes_be(&hash(&[&ap, &bp]));
    if u == BigUint::default() {
        return Err("Invalid pairing transcript".into());
    }
    let k = BigUint::from_bytes_be(&hash(&[&pad(&G_3072.n), &pad(&G_3072.g)]));
    let identity = SrpClient::<Sha256>::compute_identity_hash(b"musiccraft-pair", code.as_bytes());
    let x = SrpClient::<Sha256>::compute_x(&identity, &salt);
    let secret = pad(&client.compute_premaster_secret(&b, &k, &x, &BigUint::from_bytes_be(&a), &u));
    let m1 = hash(&[&ap, &bp, &secret]);
    let m2 = hash(&[&ap, &pad(&BigUint::from_bytes_be(&m1)), &secret]);
    let session = challenge["session"]
        .as_str()
        .filter(|s| s.len() == 36)
        .ok_or("Invalid pairing session")?;
    let body = serde_json::json!({"session":session,"proof":STANDARD.encode(&m1)});
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let mut notified = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Pairing cancelled".into());
        }
        let (status, response) = post(peer, "/v1/pair/finish", &body, cancel)?;
        if status == 202 {
            if !notified {
                waiting();
                notified = true
            }
            if std::time::Instant::now() > deadline {
                return Err("Phone approval timed out".into());
            }
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        let proof = decoded(&response, "proof", 32)?;
        if proof.len() != 32 || proof.ct_eq(&m2).unwrap_u8() != 1 {
            return Err("Phone pairing proof rejected".into());
        }
        let nonce = decoded(&response, "nonce", 12)?;
        if nonce.len() != 12 {
            return Err("Invalid pairing nonce".into());
        }
        let encrypted = decoded(&response, "payload", 4096)?;
        let key = hash(&[&secret]);
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "Pairing key unavailable")?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &encrypted,
                    aad: b"musiccraft-pair-v1",
                },
            )
            .map_err(|_| "Pairing identity rejected")?;
        let mut source =
            super::pairing(std::str::from_utf8(&plaintext).map_err(|_| "Invalid phone identity")?)?;
        source.host = peer.host.clone();
        source.port = peer.port.to_string();
        let auth = AuthStore::default();
        auth.insert(&source)?;
        online(&source.url()?, &auth, cancel, Duration::from_secs(5))?;
        return Ok(source);
    }
}
