//! Dosya işleri: klasör tarama, resim çözme, önbellek, arka plan işçileri, kaydetme, çöp kutusu.

use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::str::Chars;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;
use image::codecs::jpeg::JpegEncoder;
use image::metadata::Orientation;
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Rgba, RgbaImage};

/// "Resim aç" diyaloğunda gösterilen uzantılar
pub const EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "ico", "tga", "qoi", "pnm", "pbm", "pgm", "ppm",
    "hdr", "exr", "dds",
];

/// Arka plan işçilerinden arayüze gelen iletiler
pub enum Msg {
    Full(PathBuf, Result<Arc<RgbaImage>, String>),
    Thumb(PathBuf, egui::ColorImage),
    Siblings(Vec<PathBuf>),
    Dir(PathBuf, Result<Vec<Entry>, String>),
    /// Tarama bitti: (tarama numarası, resim içeren klasörler ve resim sayıları)
    Scan(u64, Vec<ScanItem>),
}

/// Taramada bulunan, resim içeren bir klasör
#[derive(Clone)]
pub struct ScanItem {
    pub dir: PathBuf,
    pub count: usize,
    pub first: PathBuf, // ön izleme için klasördeki bir resim
}

/// Klasör gezgininde bir satır: alt klasör ya da resim
#[derive(Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// macOS dosya adlarını ayrık (NFD) verebilir: "g" + birleştirici işaret gibi. egui bunları düzgün çizemez;
/// Türkçe harfleri tek karakterli (NFC) biçime çevirir. Yalnızca görüntü için kullanılır, yol değişmez.
pub fn nfc_tr(s: &str) -> String {
    if !s.chars().any(|c| matches!(c, '\u{0306}' | '\u{0327}' | '\u{0308}' | '\u{0307}')) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        let comp = match (c, it.peek().copied()) {
            ('g', Some('\u{0306}')) => Some('ğ'),
            ('G', Some('\u{0306}')) => Some('Ğ'),
            ('s', Some('\u{0327}')) => Some('ş'),
            ('S', Some('\u{0327}')) => Some('Ş'),
            ('c', Some('\u{0327}')) => Some('ç'),
            ('C', Some('\u{0327}')) => Some('Ç'),
            ('o', Some('\u{0308}')) => Some('ö'),
            ('O', Some('\u{0308}')) => Some('Ö'),
            ('u', Some('\u{0308}')) => Some('ü'),
            ('U', Some('\u{0308}')) => Some('Ü'),
            ('I', Some('\u{0307}')) => Some('İ'),
            _ => None,
        };
        match comp {
            Some(r) => {
                it.next();
                out.push(r);
            }
            None => out.push(c),
        }
    }
    out
}

pub fn is_image(p: &Path) -> bool {
    ImageFormat::from_path(p).map(|f| f.reading_enabled()).unwrap_or(false)
}

// ---------- Çözme ----------

/// EXIF yönü uygulanmış olarak çözer.
fn decode_dyn(path: &Path) -> Result<DynamicImage, String> {
    let mut reader = ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    reader.no_limits();
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    Ok(img)
}

pub fn decode(path: &Path) -> Result<RgbaImage, String> {
    decode_dyn(path).map(|d| d.to_rgba8())
}

fn make_thumb(path: &Path) -> Option<egui::ColorImage> {
    let img = decode_dyn(path).ok()?.thumbnail(256, 256).to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

// ---------- Arka plan işçileri ----------

pub struct Workers {
    pub full_tx: Sender<PathBuf>,
    pub thumb_tx: Sender<PathBuf>,
    tx: Sender<Msg>,
    pub ctx: egui::Context,
}

fn spawn_pool<F>(n: usize, rx: Receiver<PathBuf>, f: F)
where
    F: Fn(PathBuf) + Send + Sync + 'static,
{
    let rx = Arc::new(Mutex::new(rx));
    let f = Arc::new(f);
    for _ in 0..n {
        let rx = rx.clone();
        let f = f.clone();
        thread::spawn(move || loop {
            let job = rx.lock().unwrap().recv();
            match job {
                Ok(p) => f(p),
                Err(_) => break,
            }
        });
    }
}

impl Workers {
    pub fn start(ctx: &egui::Context, tx: Sender<Msg>) -> Workers {
        let (full_tx, full_rx) = channel::<PathBuf>();
        let (thumb_tx, thumb_rx) = channel::<PathBuf>();

        {
            let tx = tx.clone();
            let ctx = ctx.clone();
            spawn_pool(2, full_rx, move |p| {
                let r = decode(&p).map(Arc::new);
                let _ = tx.send(Msg::Full(p, r));
                ctx.request_repaint();
            });
        }
        {
            let tx = tx.clone();
            let ctx = ctx.clone();
            spawn_pool(4, thumb_rx, move |p| {
                if let Some(ci) = make_thumb(&p) {
                    let _ = tx.send(Msg::Thumb(p, ci));
                    ctx.request_repaint();
                }
            });
        }
        Workers { full_tx, thumb_tx, tx, ctx: ctx.clone() }
    }

    /// Klasörün alt klasörlerini ve resimlerini arka planda listeler (gezgin için).
    pub fn list_dir(&self, dir: PathBuf) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let r = list_dir(&dir);
            let _ = tx.send(Msg::Dir(dir, r));
            ctx.request_repaint();
        });
    }

    /// `root` ve tüm alt dizinlerini tarar; resim içeren klasörleri sıralı döndürür.
    pub fn scan(&self, id: u64, root: PathBuf, skip: Vec<PathBuf>, cancel: Arc<AtomicBool>, found: Arc<AtomicUsize>) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let r = scan_tree(&root, &skip, &cancel, &found, &ctx);
            if !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = tx.send(Msg::Scan(id, r));
                ctx.request_repaint();
            }
        });
    }

    /// Klasördeki diğer resimleri arka planda listeler.
    pub fn list(&self, start: PathBuf) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(Msg::Siblings(siblings(&start)));
            ctx.request_repaint();
        });
    }
}

// ---------- Önbellek ----------

const CACHE_LIMIT: usize = 768 << 20;

pub struct Cache {
    map: HashMap<PathBuf, Arc<RgbaImage>>,
    order: VecDeque<PathBuf>,
    bytes: usize,
}

impl Cache {
    pub fn new() -> Self {
        Cache { map: HashMap::new(), order: VecDeque::new(), bytes: 0 }
    }

    pub fn get(&self, p: &Path) -> Option<Arc<RgbaImage>> {
        self.map.get(p).cloned()
    }

    pub fn remove(&mut self, p: &Path) {
        if let Some(img) = self.map.remove(p) {
            self.bytes = self.bytes.saturating_sub(cost(&img));
            self.order.retain(|q| q != p);
        }
    }

    pub fn insert(&mut self, p: PathBuf, img: Arc<RgbaImage>) {
        self.remove(&p);
        self.bytes += cost(&img);
        self.order.push_back(p.clone());
        self.map.insert(p, img);
        while self.bytes > CACHE_LIMIT && self.order.len() > 1 {
            if let Some(old) = self.order.pop_front() {
                if let Some(img) = self.map.remove(&old) {
                    self.bytes = self.bytes.saturating_sub(cost(&img));
                }
            }
        }
    }
}

fn cost(img: &RgbaImage) -> usize {
    img.width() as usize * img.height() as usize * 4
}

// ---------- Klasör tarama ----------

fn take_num(it: &mut Peekable<Chars<'_>>) -> u128 {
    let mut n: u128 = 0;
    while let Some(&c) = it.peek() {
        if let Some(d) = c.to_digit(10) {
            n = n.saturating_mul(10).saturating_add(d as u128);
            it.next();
        } else {
            break;
        }
    }
    n
}

/// Sayıya duyarlı, büyük/küçük harf gözetmeyen sıralama ("img2" < "img10")
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                if x.is_ascii_digit() && y.is_ascii_digit() {
                    let na = take_num(&mut ai);
                    let nb = take_num(&mut bi);
                    if na != nb {
                        return na.cmp(&nb);
                    }
                } else if x != y {
                    return x.cmp(&y);
                } else {
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

/// Aynı klasördeki resimler (ada göre sıralı). `start` her zaman listededir.
pub fn siblings(start: &Path) -> Vec<PathBuf> {
    let dir = match start.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let me = start.file_name().map(|s| s.to_os_string());
    let mut v: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let p = e.path();
            let is_me = Some(e.file_name()) == me;
            if is_me || (is_image(&p) && p.is_file()) {
                v.push((name, p));
            }
        }
    }
    v.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    let list: Vec<PathBuf> = v.into_iter().map(|(_, p)| p).collect();
    if list.is_empty() {
        vec![start.to_path_buf()]
    } else {
        list
    }
}

/// Klasörün içeriği: önce alt klasörler, sonra resimler (ikisi de ada göre, sayıya duyarlı sıralı).
pub fn list_dir(dir: &Path) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut dirs: Vec<Entry> = Vec::new();
    let mut imgs: Vec<Entry> = Vec::new();
    for e in rd.filter_map(|e| e.ok()) {
        let raw = e.file_name().to_string_lossy().to_string();
        if raw.starts_with('.') {
            continue;
        }
        let name = nfc_tr(&raw);
        let path = e.path();
        if path.is_dir() {
            if name.to_lowercase().ends_with(".photoslibrary") {
                continue; // Fotoğraflar kitaplığı bir paket; içine girilmez
            }
            if name == "Applications" {
                continue; // Uygulamalar klasörü gezginde gösterilmez
            }
            dirs.push(Entry { path, name, is_dir: true });
        } else if is_image(&path) && path.is_file() {
            imgs.push(Entry { path, name, is_dir: false });
        }
    }
    dirs.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    imgs.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    dirs.extend(imgs);
    Ok(dirs)
}

/// Klasör ağacını yinelemesiz dolaşır (sembolik bağlı klasörlere, `skip` listesindekilere ve .app paketlerine girmez);
/// resim içeren klasörleri sıralı döndürür.
pub fn scan_tree(
    root: &Path,
    skip: &[PathBuf],
    cancel: &AtomicBool,
    found: &AtomicUsize,
    ctx: &egui::Context,
) -> Vec<ScanItem> {
    use std::sync::atomic::Ordering::Relaxed;
    let mut out: Vec<ScanItem> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        if cancel.load(Relaxed) {
            break;
        }
        visited += 1;
        if visited % 40 == 0 {
            ctx.request_repaint();
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut n = 0usize;
        let mut first: Option<PathBuf> = None;
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                let lower = name.to_lowercase();
                let is_pkg = [".photoslibrary", ".app", ".framework", ".bundle", ".plugin", ".kext"]
                    .iter()
                    .any(|ext| lower.ends_with(ext));
                if is_pkg || skip.iter().any(|s| *s == path) {
                    continue;
                }
                stack.push(path);
            } else if (ft.is_file() || ft.is_symlink()) && is_image(&path) && path.is_file() {
                n += 1;
                if first.is_none() {
                    first = Some(path);
                }
            }
        }
        if let Some(first) = first {
            found.fetch_add(n, Relaxed);
            out.push(ScanItem { dir, count: n, first });
        }
    }
    out.sort_by(|a, b| natural_cmp(&a.dir.to_string_lossy(), &b.dir.to_string_lossy()));
    out
}

// ---------- Kaydetme / çöp ----------

/// Resmi sormadan "ad kopya.uzantı" olarak aynı klasöre kaydeder.
pub fn save_copy(src: &Path, img: RgbaImage) -> Result<PathBuf, String> {
    let ext0 = src.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let ext = match ext0.as_str() {
        "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp" | "gif" => ext0.clone(),
        _ => "png".to_string(), // webp/ico/hdr... → png
    };
    let base = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "resim".to_string());
    let dir = src.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut dest = dir.join(format!("{base} kopya.{ext}"));
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{base} kopya {n}.{ext}"));
        n += 1;
    }
    match ext.as_str() {
        "jpg" | "jpeg" => {
            let rgb = DynamicImage::ImageRgba8(img).to_rgb8();
            let file = std::io::BufWriter::new(std::fs::File::create(&dest).map_err(|e| e.to_string())?);
            JpegEncoder::new_with_quality(file, 95)
                .write_image(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
                .map_err(|e| e.to_string())?;
        }
        _ => img.save(&dest).map_err(|e| e.to_string())?,
    }
    Ok(dest)
}

/// Dosyayı onay sormadan Çöp Kutusu'na atar.
pub fn trash(p: &Path) -> Result<(), String> {
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    ctx.delete(p).map_err(|e| e.to_string())
}

// ---------- Kalem ----------

fn stamp(img: &mut RgbaImage, cx: f32, cy: f32, r: f32) {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let x0 = ((cx - r).floor() as i64).max(0);
    let x1 = ((cx + r).ceil() as i64).min(w - 1);
    let y0 = ((cy - r).floor() as i64).max(0);
    let y1 = ((cy + r).ceil() as i64).min(h - 1);
    if x0 > x1 || y0 > y1 {
        return;
    }
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x as u32, y as u32, Rgba([255, 0, 0, 255]));
            }
        }
    }
}

/// Kırmızı çizgiyi (resim piksel koordinatında) resme işler.
pub fn draw_stroke(img: &mut RgbaImage, pts: &[(f32, f32)], width: f32) {
    let r = (width / 2.0).max(0.5);
    match pts.len() {
        0 => {}
        1 => stamp(img, pts[0].0, pts[0].1, r),
        _ => {
            for seg in pts.windows(2) {
                let (a, b) = (seg[0], seg[1]);
                let dist = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                let steps = (dist / (r * 0.5).max(0.5)).ceil().max(1.0) as u32;
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    stamp(img, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, r);
                }
            }
        }
    }
}
