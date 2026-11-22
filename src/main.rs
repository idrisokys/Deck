// Deck — koyu temalı, tamamen Türkçe resim görüntüleyici (Rust + egui).

mod app;
mod gallery;
mod lang;
mod macos_open;
mod thumbs;

use std::path::PathBuf;

/// Uygulama simgesi (Deck-icon-1024.png derlemeye gömülür)
fn app_icon() -> Option<eframe::egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../Deck-icon-1024.png")).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(eframe::egui::IconData { rgba: img.into_raw(), width, height })
}

fn main() -> eframe::Result<()> {
    // Komut satırından verilen ilk geçerli resim dosyası (yoksa boş açılır)
    let start: Option<PathBuf> = std::env::args().skip(1).map(PathBuf::from).find(|p| p.is_file());

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Deck")
        .with_inner_size([1000.0, 700.0])
        .with_min_inner_size([640.0, 420.0]);
    // Dock simgesi: eframe aksi halde kendi varsayılan simgesini koyar
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };

    // Finder uygulama kapalıyken belge açma Apple Event'ini ilk pencere kurulmadan yakala.
    macos_open::install_open_event_handler();

    eframe::run_native(
        "Deck",
        options,
        Box::new(move |cc| Ok(Box::new(app::DeckApp::new(cc, start)))),
    )
}
