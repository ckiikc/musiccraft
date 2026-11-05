use crate::network::FtpSource;
const SERVICE: &str = "org.example.musiccraft.ftp";
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
pub fn load(_: &FtpSource) -> Option<String> {
    None
}
#[cfg(not(target_os = "macos"))]
pub fn save(_: &FtpSource) -> Result<(), String> {
    Err("Portachiavi disponibile nella build macOS".into())
}

#[cfg(target_os = "macos")]
pub fn forget(source: &FtpSource) {
    let _ = security_framework::passwords::delete_generic_password(SERVICE, &account(source));
}
#[cfg(not(target_os = "macos"))]
pub fn forget(_: &FtpSource) {}
