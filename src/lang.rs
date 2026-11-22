//! Arayüz dili / UI language (Türkçe · English).
//! İlk açılışta sistem diline bakılır (Türkçe ise Türkçe, değilse English); üst çubuktaki TR/EN düğmesi
//! seçimi değiştirir ve bir sonraki açılış için saklar.

use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Tr,
    En,
}

impl Lang {
    /// Geçerli dile göre iki metinden birini seçer.
    pub fn t(self, tr: &'static str, en: &'static str) -> &'static str {
        match self {
            Lang::Tr => tr,
            Lang::En => en,
        }
    }

    pub fn other(self) -> Lang {
        match self {
            Lang::Tr => Lang::En,
            Lang::En => Lang::Tr,
        }
    }

    /// Kayıtlı seçim varsa onu, yoksa sistem dilini kullanır.
    pub fn load() -> Lang {
        if let Ok(s) = std::fs::read_to_string(config_file()) {
            match s.trim() {
                "tr" => return Lang::Tr,
                "en" => return Lang::En,
                _ => {}
            }
        }
        system_lang()
    }

    pub fn save(self) {
        let p = config_file();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, if self == Lang::Tr { "tr" } else { "en" });
    }
}

fn config_file() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    let mac = home.join("Library");
    if mac.is_dir() {
        mac.join("Application Support").join("Deck").join("lang")
    } else {
        home.join(".config").join("deck").join("lang")
    }
}

/// macOS: `defaults read -g AppleLanguages` (ilk dil); olmazsa LANG ortam değişkeni.
fn system_lang() -> Lang {
    let out = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let s = if out.trim().is_empty() { std::env::var("LANG").unwrap_or_default() } else { out };
    let code: String = s.chars().skip_while(|c| !c.is_ascii_alphabetic()).take(2).collect::<String>().to_lowercase();
    if code == "tr" {
        Lang::Tr
    } else {
        Lang::En
    }
}
