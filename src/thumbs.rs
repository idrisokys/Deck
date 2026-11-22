//! Küçük resimler: JPEG'lerde ölçekli (düşük çözünürlüklü) çözme, disk önbelleği,
//! görünmeyenleri atlayan kuyruk ve eski önbelleği otomatik temizleme.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::metadata::Orientation;
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageReader, RgbImage, RgbaImage};

use crate::gallery::decode_dyn;

/// Küçük resmin en uzun kenarı (piksel)
const THUMB_PX: u32 = 256;
/// Önbellek biçimi değişirse artırın; eski kayıtlar kendiliğinden geçersiz olur
const CACHE_VER: &str = "v1";
/// Bu süredir kullanılmayan önbellek dosyaları silinir
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// Önbellek bu boyutu aşarsa en eski dosyalar silinir
const MAX_BYTES: u64 = 400 * 1024 * 1024;

// ---------- Görünür olanlara göre kuyruk ----------

#[derive(Default)]
struct State {
    /// Kare sayacı (her çizimde artar)
    gen: u64,
    /// Kuyrukta ya da işlenmekte olan resimler → en son hangi karede görünür olduğu
    queued: HashMap<PathBuf, u64>,
    /// Çözülemeyenler (tekrar tekrar denenmesin)
    failed: HashSet<PathBuf>,
}

#[derive(Default)]
pub struct Queue {
    st: Mutex<State>,
}

impl Queue {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Her kare çiziminin başında çağrılır.
    pub fn begin_frame(&self) {
        self.lock().gen += 1;
    }

    /// Resim şu karede görünüyor. Kuyruğa yeni eklenmesi gerekiyorsa `true` döner.
    pub fn want(&self, p: &Path) -> bool {
        let mut st = self.lock();
        if st.failed.contains(p) {
            return false;
        }
        let g = st.gen;
        if let Some(last) = st.queued.get_mut(p) {
            *last = g;
            false
        } else {
            st.queued.insert(p.to_path_buf(), g);
            true
        }
    }

    /// İşçi işe başlamadan önce sorar: resim hâlâ (en fazla bir kare önce) görünür müydü?
    pub fn still_wanted(&self, p: &Path) -> bool {
        let mut st = self.lock();
        let g = st.gen;
        match st.queued.get(p).copied() {
            Some(last) if last + 1 >= g => true,
            Some(_) => {
                st.queued.remove(p);
                false
            }
            None => false,
        }
    }

    pub fn done(&self, p: &Path, ok: bool) {
        let mut st = self.lock();
        st.queued.remove(p);
        if !ok {
            st.failed.insert(p.to_path_buf());
        }
    }
}

// ---------- Disk önbelleği ----------

fn cache_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let mac = home.join("Library");
    Some(if mac.is_dir() { mac.join("Caches").join("Deck").join("thumbs") } else { home.join(".cache").join("deck").join("thumbs") })
}

/// FNV-1a: Rust sürümünden bağımsız, kararlı özet
fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Anahtar: yol + boyut + değişiklik zamanı → dosya değişirse eski küçük resim kullanılmaz
fn cache_key(path: &Path, meta: &fs::Metadata) -> String {
    let mt = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    let s = format!("{CACHE_VER}|{THUMB_PX}|{}|{}|{mt}", path.to_string_lossy(), meta.len());
    format!("{:016x}", fnv(s.as_bytes()))
}

/// Kullanılan kaydın zamanını (en çok günde bir) tazeler; temizlik buna bakar.
fn touch(f: &Path) {
    let Ok(mt) = fs::metadata(f).and_then(|m| m.modified()) else { return };
    if SystemTime::now().duration_since(mt).unwrap_or_default() > Duration::from_secs(24 * 3600) {
        if let Ok(h) = fs::OpenOptions::new().write(true).open(f) {
            let _ = h.set_modified(SystemTime::now());
        }
    }
}

fn cache_load(dir: &Path, key: &str) -> Option<RgbaImage> {
    for ext in ["jpg", "png"] {
        let f = dir.join(format!("{key}.{ext}"));
        if let Ok(bytes) = fs::read(&f) {
            match image::load_from_memory(&bytes) {
                Ok(img) => {
                    touch(&f);
                    return Some(img.to_rgba8());
                }
                Err(_) => {
                    let _ = fs::remove_file(&f); // bozuk kayıt
                }
            }
        }
    }
    None
}

/// Saydam piksel varsa PNG, yoksa küçük boyutlu JPEG
fn encode(img: &RgbaImage) -> Option<(Vec<u8>, &'static str)> {
    let (w, h) = img.dimensions();
    let mut buf = Vec::new();
    if img.pixels().any(|p| p[3] != 255) {
        PngEncoder::new(&mut buf).write_image(img.as_raw(), w, h, ExtendedColorType::Rgba8).ok()?;
        Some((buf, "png"))
    } else {
        let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
        JpegEncoder::new_with_quality(&mut buf, 85).write_image(rgb.as_raw(), w, h, ExtendedColorType::Rgb8).ok()?;
        Some((buf, "jpg"))
    }
}

fn cache_store(dir: &Path, key: &str, img: &RgbaImage) {
    let Some((bytes, ext)) = encode(img) else { return };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!("{key}.tmp"));
    if fs::write(&tmp, bytes).is_ok() && fs::rename(&tmp, dir.join(format!("{key}.{ext}"))).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

/// Eski ve fazla önbelleği temizler: 30 günden eski dosyalar, sonra boyut sınırı (en eskiden başlayarak).
fn clean_cache(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let now = SystemTime::now();
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let mut total: u64 = 0;
    for e in rd.flatten() {
        let Ok(m) = e.metadata() else { continue };
        if !m.is_file() {
            continue;
        }
        let p = e.path();
        let mt = m.modified().unwrap_or(now);
        let age = now.duration_since(mt).unwrap_or_default();
        let is_tmp = p.extension().map_or(false, |x| x == "tmp");
        if (is_tmp && age > Duration::from_secs(3600)) || age > MAX_AGE {
            let _ = fs::remove_file(&p);
            continue;
        }
        if is_tmp {
            continue;
        }
        total += m.len();
        files.push((mt, m.len(), p));
    }
    if total > MAX_BYTES {
        files.sort_by_key(|f| f.0);
        let target = MAX_BYTES / 4 * 3;
        for (_, len, p) in files {
            if total <= target {
                break;
            }
            if fs::remove_file(&p).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

/// Uygulama başladıktan kısa süre sonra arka planda bir kez temizler.
pub fn clean_cache_later() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(8));
        if let Some(d) = cache_dir() {
            clean_cache(&d);
        }
    });
}

// ---------- Hızlı çözme ----------

fn read_orientation(path: &Path) -> Orientation {
    ImageReader::open(path)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .and_then(|r| r.into_decoder().ok())
        .and_then(|mut d| d.orientation().ok())
        .unwrap_or(Orientation::NoTransforms)
}

/// JPEG: DCT ölçekleme ile 1/2, 1/4 ya da 1/8 boyutta çözer; tam çözünürlük hiç belleğe alınmaz.
/// Desteklenmeyen JPEG türlerinde (CMYK, aritmetik kodlama…) `None` döner.
fn thumb_jpeg_scaled(path: &Path) -> Option<RgbaImage> {
    let mut f = fs::File::open(path).ok()?;
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic).ok()?;
    if magic != [0xFF, 0xD8] {
        return None;
    }
    f.rewind().ok()?;

    let mut dec = jpeg_decoder::Decoder::new(BufReader::new(f));
    dec.read_info().ok()?;
    let (w, h) = dec.scale(THUMB_PX as u16, THUMB_PX as u16).ok()?;
    let (w, h) = (w as u32, h as u32);
    let px = dec.decode().ok()?;
    let fmt = dec.info()?.pixel_format;

    let rgb: RgbImage = match fmt {
        jpeg_decoder::PixelFormat::RGB24 => RgbImage::from_raw(w, h, px)?,
        jpeg_decoder::PixelFormat::L8 => {
            if px.len() != (w * h) as usize {
                return None;
            }
            RgbImage::from_raw(w, h, px.iter().flat_map(|&v| [v, v, v]).collect())?
        }
        _ => return None,
    };
    let mut img = DynamicImage::ImageRgb8(rgb);
    img.apply_orientation(read_orientation(path));
    Some(img.thumbnail(THUMB_PX, THUMB_PX).to_rgba8())
}

fn to_color(img: &RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

/// Önce disk önbelleğine bakar; yoksa hızlı yolla (JPEG) ya da tam çözerek üretir ve diske yazar.
pub fn make_thumb(path: &Path) -> Option<egui::ColorImage> {
    let dir = cache_dir();
    let key = fs::metadata(path).ok().map(|m| cache_key(path, &m));

    if let (Some(d), Some(k)) = (&dir, &key) {
        if let Some(img) = cache_load(d, k) {
            return Some(to_color(&img));
        }
    }

    let img = thumb_jpeg_scaled(path).or_else(|| decode_dyn(path).ok().map(|d| d.thumbnail(THUMB_PX, THUMB_PX).to_rgba8()))?;

    if let (Some(d), Some(k)) = (&dir, &key) {
        cache_store(d, k, &img);
    }
    Some(to_color(&img))
}
