fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon/musiccraft-1024.png");
    println!("cargo:rerun-if-changed=packaging/windows.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Derive an ICO from the original project asset, rather than introducing an external icon.
    let image = image::open("assets/app-icon/musiccraft-1024.png")
        .expect("project icon")
        .resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("PNG icon");
    let png = png.into_inner();
    let mut ico = vec![0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 32, 0];
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(&png);
    let path =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("musiccraft.ico");
    std::fs::write(&path, ico).expect("write ICO");
    winresource::WindowsResource::new()
        .set_icon(path.to_str().unwrap())
        .set("ProductName", "MusicCraft")
        .set("FileDescription", "MusicCraft music player")
        .set("OriginalFilename", "musiccraft.exe")
        .set(
            "LegalCopyright",
            "Copyright 2026 MusicCraft contributors. MIT.",
        )
        .set_manifest_file("packaging/windows.manifest")
        .compile()
        .expect("Windows resources");
}
