//! Use curl's supported no-CWD mode only for FTP basenames that servers trim as whitespace.
//! Passwords use the existing private stdin configuration, never arguments or files.
use super::*;
use std::process::{Command, Stdio};
impl Ranged {
    pub(super) fn literal_transfer(&mut self, size: usize, head: bool) -> io::Result<Vec<u8>> {
        let (source, auth) = self
            .literal
            .clone()
            .ok_or_else(|| error("Missing literal FTP source"))?;
        let mut cmd = Command::new("/usr/bin/curl");
        cmd.args([
            "--config",
            "-",
            "--silent",
            "--fail",
            "--globoff",
            "--ftp-method",
            "nocwd",
            "--connect-timeout",
            "3",
            "--max-time",
            "5",
            "--limit-rate",
            "256K",
            "--proto",
            "=ftp,ftps",
        ]);
        if head {
            cmd.arg("--head");
        } else {
            cmd.args([
                "--range",
                &format!("{}-{}", self.position, self.position + size as u64 - 1),
            ]);
        }
        cmd.args(["--url", &source])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child =
            crate::network::authenticated_spawn(&mut cmd, &auth, &source).map_err(error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| error("Missing metadata output"))?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(size as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let status = loop {
            if let Err(e) = self.check() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(e);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(e);
                }
            }
        };
        let bytes = reader
            .join()
            .map_err(|_| error("Metadata output interrupted"))??;
        if !status.success() {
            return Err(io::Error::new(
                match status.code() {
                    Some(67) => io::ErrorKind::PermissionDenied,
                    Some(6 | 7) => io::ErrorKind::NotConnected,
                    Some(28) => io::ErrorKind::TimedOut,
                    Some(78) => io::ErrorKind::NotFound,
                    _ => io::ErrorKind::Other,
                },
                "Literal FTP metadata transfer failed",
            ));
        }
        if bytes.len() > size {
            return Err(error("Metadata response exceeded its range"));
        }
        Ok(bytes)
    }
}
pub(super) fn needed(source: &str) -> bool {
    url::Url::parse(source).is_ok_and(|u| {
        matches!(u.scheme(), "ftp" | "ftps")
            && u.path_segments()
                .and_then(|mut p| p.next_back())
                .is_some_and(|s| {
                    percent_encoding::percent_decode_str(s)
                        .decode_utf8_lossy()
                        .starts_with(char::is_whitespace)
                })
    })
}
