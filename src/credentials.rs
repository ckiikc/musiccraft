use crate::network::FtpSource;
const SERVICE: &str = "io.github.ckiikc.musiccraft.ftp";
fn account(source: &FtpSource) -> String {
    format!("{}:{}", source.id, source.username)
}

#[cfg(target_os = "macos")]
pub fn load(source: &FtpSource) -> Option<String> {
    security_framework::passwords::get_generic_password(SERVICE, &account(source))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
}
#[cfg(target_os = "macos")]
pub fn save(source: &FtpSource) -> Result<(), String> {
    security_framework::passwords::set_generic_password(
        SERVICE,
        &account(source),
        source.password.as_bytes(),
    )
    .map_err(|_| "Impossibile salvare nel Portachiavi: la password resta solo in memoria".into())
}
#[cfg(not(target_os = "macos"))]
pub fn load(source: &FtpSource) -> Option<String> {
    keyring::Entry::new(SERVICE, &account(source))
        .ok()?
        .get_password()
        .ok()
}
#[cfg(not(target_os = "macos"))]
pub fn save(source: &FtpSource) -> Result<(), String> {
    keyring::Entry::new(SERVICE, &account(source))
        .and_then(|entry| entry.set_password(&source.password))
        .map_err(|_| "Secure credential store unavailable or locked; the password remains in memory. Unlock Windows Credential Manager or the Linux Secret Service and retry.".into())
}

#[cfg(target_os = "macos")]
pub fn forget(source: &FtpSource) {
    let _ = security_framework::passwords::delete_generic_password(SERVICE, &account(source));
}
#[cfg(not(target_os = "macos"))]
pub fn forget(source: &FtpSource) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, &account(source)) {
        let _ = entry.delete_credential();
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires an isolated OS credential-store session, never the user's profile"]
    fn live_native_credential_store_round_trip() {
        let source = FtpSource {
            id: format!("qa-{:032x}", rand::random::<u128>()),
            username: "fixture".into(),
            password: format!("fixture-{:032x}", rand::random::<u128>()),
            ..Default::default()
        };
        assert!(load(&source).is_none());
        save(&source).expect("isolated native credential store");
        let loaded = load(&source);
        forget(&source);
        assert_eq!(loaded.as_deref(), Some(source.password.as_str()));
        assert!(load(&source).is_none());
    }
}
