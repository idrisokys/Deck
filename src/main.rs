// Deck — koyu temalı, tamamen Türkçe resim görüntüleyici (Rust + egui).

mod app;
mod gallery;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // Komut satırından verilen ilk geçerli resim dosyası (yoksa boş açılır)
    let start: Option<PathBuf> = std::env::args().skip(1).map(PathBuf::from).find(|p| p.is_file());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Deck")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Deck",
        options,
        Box::new(move |cc| Ok(Box::new(app::DeckApp::new(cc, start)))),
    )
}
