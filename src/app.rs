//! Deck arayüzü (egui): araç çubuğu, resim alanı, kırpma, kırmızı kalem, alt şerit ve kaydırıcılar.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, Context, FontId, Key, Layout, Modifiers, Order, Pos2, Rect,
    RichText, Sense, Shape, Stroke, TextureHandle, TextureOptions, Vec2,
};
use image::{imageops, RgbaImage};

use crate::gallery::{self, Cache, Entry, Msg, ScanItem, Workers};
use crate::lang::Lang;
use crate::video::{self, VideoPlayer};

const BG: Color32 = Color32::from_rgb(23, 26, 28);
const BLUE: Color32 = Color32::from_rgb(10, 132, 255);
const PEN_RED: Color32 = Color32::from_rgb(255, 80, 80);
const SLIDE_SECS: u64 = 3;
const TOAST_MS: u64 = 2500;
const MAX_UNDO: usize = 10;
const ZOOM_BASE: f32 = 100.0; // büyütme çubuğu: %1 … %10000
const TOP_BAR_H: f32 = 58.0; // üst çubuk yüksekliği
const BAR_INNER: f32 = 46.0; // çerçeve boşluğu düşülmüş iç yükseklik
const BTN_H: f32 = 40.0; // düğme yüksekliği
const FF_RATE: f32 = 2.5; // oynat düğmesine basılı tutunca hızlı ileri sarma
const FF_HOLD_MS: u128 = 350; // basılı tutma sayılması için süre
const CTRL_H: f32 = 56.0; // video denetim çubuğu yüksekliği
const THUMB_CAP: usize = 600; // bellekte tutulan en çok küçük resim

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Browse, // klasör gezgini (açılış ekranı)
    Viewer, // resim görüntüleyici
}

/// Başlangıç klasörü: ~/Pictures (yoksa ev klasörü)
fn pictures_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    match home {
        Some(h) => {
            let p = h.join("Pictures");
            if p.is_dir() {
                p
            } else {
                h
            }
        }
        None => PathBuf::from("."),
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn parent_dir(p: &Path) -> PathBuf {
    let d = match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::canonicalize(&d).unwrap_or(d)
}

fn def_crop() -> Rect {
    Rect::from_min_max(pos2(0.1, 0.1), pos2(0.9, 0.9))
}

#[derive(Clone, Copy, Default)]
struct CropMode {
    l: bool,
    r: bool,
    t: bool,
    b: bool,
    mv: bool,
}

pub struct DeckApp {
    workers: Workers,
    rx: Receiver<Msg>,

    // klasör gezgini
    screen: Screen,
    pics: PathBuf, // başlangıç klasörü (Pictures)
    root: PathBuf, // gezinme sınırı: ana dizin (ev klasörü)
    root_canon: PathBuf,
    dir: PathBuf,
    entries: Vec<Entry>,
    listing: bool,
    dir_err: Option<String>,
    hist_back: Vec<PathBuf>,
    hist_fwd: Vec<PathBuf>,

    // tarama (alt dizinlerdeki tüm resim klasörleri)
    scanning: bool,
    scan_id: u64,
    scan_root: PathBuf,
    scan_list: Vec<ScanItem>,
    scan_cancel: Arc<AtomicBool>,
    scan_found: Arc<AtomicUsize>,
    view_scan: bool, // ana ekranda tarama sonuçları gösterilsin mi

    files: Vec<PathBuf>,
    idx: usize,

    cur: Option<Arc<RgbaImage>>, // ekrandaki resim (düzenlemelerle birlikte)
    cur_path: Option<PathBuf>,   // ekrandaki resmin dosyası (kaydetme bunu kullanır)
    tex: Option<TextureHandle>,
    tex_dirty: bool,
    failed: bool,
    cache: Cache,
    pending: HashSet<PathBuf>,
    thumbs: HashMap<PathBuf, TextureHandle>,
    thumb_req: HashSet<PathBuf>,
    scroll_cur: bool,

    // kırpma
    crop: bool,
    cn: Rect, // resme göre 0..1, sol üst başlangıçlı
    d0: Rect,
    dm: Option<CropMode>,
    drag_origin: Pos2,

    // kalem
    pen: bool,
    stroke: Vec<Pos2>,
    undo: Vec<Arc<RgbaImage>>,

    // görünüm
    zoom_v: f32, // -1..1 → 0,01x .. 100x (ortası 1x)
    rot_v: f32,  // -180..180 derece
    pan: Vec2,

    // video
    video: Option<VideoPlayer>,
    video_tex: Option<TextureHandle>,
    scrub: Option<f64>,         // ilerleme çubuğu sürüklenirken gösterilen saniye
    ff_hold: Option<Instant>,   // oynat düğmesine basılma anı
    ff: bool,                   // 2.5× ileri sarma sürüyor
    ff_resume: bool,            // ileri sarmadan önce oynuyor muydu
    ff_consumed: bool,          // bırakma tıklaması durdur/oynat sayılmasın

    play: bool,
    fullscreen: bool,
    last_slide: Instant,
    toast: Option<(String, Instant)>,
    lang: Lang,
}

impl DeckApp {
    pub fn new(cc: &eframe::CreationContext<'_>, start: Option<PathBuf>) -> Self {
        let ctx = cc.egui_ctx.clone();
        load_fonts(&ctx);
        crate::macos_open::install(&ctx); // Finder'dan çift tık / Birlikte aç
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        ctx.set_visuals(v);

        let (tx, rx) = channel();
        let workers = Workers::start(&ctx, tx);

        let mut app = DeckApp {
            workers,
            rx,
            screen: Screen::Browse,
            pics: pictures_dir(),
            root: home_dir(),
            root_canon: std::fs::canonicalize(home_dir()).unwrap_or_else(|_| home_dir()),
            dir: pictures_dir(),
            entries: Vec::new(),
            listing: true,
            dir_err: None,
            hist_back: Vec::new(),
            hist_fwd: Vec::new(),
            scanning: false,
            scan_id: 0,
            scan_root: PathBuf::new(),
            scan_list: Vec::new(),
            scan_cancel: Arc::new(AtomicBool::new(false)),
            scan_found: Arc::new(AtomicUsize::new(0)),
            view_scan: false,
            files: Vec::new(),
            idx: 0,
            cur: None,
            cur_path: None,
            tex: None,
            tex_dirty: false,
            failed: false,
            video: None,
            video_tex: None,
            scrub: None,
            ff_hold: None,
            ff: false,
            ff_resume: false,
            ff_consumed: false,
            cache: Cache::new(),
            pending: HashSet::new(),
            thumbs: HashMap::new(),
            thumb_req: HashSet::new(),
            scroll_cur: false,
            crop: false,
            cn: def_crop(),
            d0: def_crop(),
            dm: None,
            drag_origin: Pos2::ZERO,
            pen: false,
            stroke: Vec::new(),
            undo: Vec::new(),
            zoom_v: 0.0,
            rot_v: 0.0,
            pan: Vec2::ZERO,
            play: false,
            fullscreen: false,
            last_slide: Instant::now(),
            toast: None,
            lang: Lang::load(),
        };
        match start {
            Some(p) => app.open(p),
            None => app.workers.list_dir(app.dir.clone()),
        }
        app
    }

    // ---------- Yükleme ----------

    /// Resim hemen açılır; klasördeki diğer resimler arka planda listelenir.
    fn open(&mut self, path: PathBuf) {
        self.screen = Screen::Viewer;
        self.files = vec![path.clone()];
        self.idx = 0;
        self.cur = None;
        self.cur_path = None;
        self.tex = None;
        self.failed = false;
        self.reset_edit();
        self.workers.list(path);
        self.load_current();
        self.scroll_cur = true;
    }

    fn open_dialog(&mut self) {
        let all: Vec<&str> = gallery::EXTS.iter().chain(gallery::VIDEO_EXTS.iter()).copied().collect();
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(self.lang.t("Resimler ve videolar", "Images and videos"), &all)
            .add_filter(self.lang.t("Resimler", "Images"), gallery::EXTS)
            .add_filter(self.lang.t("Videolar", "Videos"), gallery::VIDEO_EXTS)
            .pick_file()
        {
            self.open(p);
        }
    }

    fn set_cur(&mut self, path: PathBuf, img: Arc<RgbaImage>) {
        self.cur = Some(img);
        self.cur_path = Some(path);
        self.tex_dirty = true;
        self.failed = false;
    }

    fn request_full(&mut self, p: PathBuf) {
        if gallery::is_video(&p) {
            return; // videolar çözülmez, oynatıcıyla açılır
        }
        if self.cache.get(&p).is_some() || self.pending.contains(&p) {
            return;
        }
        self.pending.insert(p.clone());
        let _ = self.workers.full_tx.send(p);
    }

    /// Geçerli resmi yükler; yüklenirken eski resim görünür kalır.
    fn load_current(&mut self) {
        self.stop_video();
        let Some(p) = self.files.get(self.idx).cloned() else {
            self.cur = None;
            self.cur_path = None;
            self.tex = None;
            self.failed = false;
            return;
        };
        if gallery::is_video(&p) {
            self.load_video(p);
            return;
        }
        if let Some(img) = self.cache.get(&p) {
            self.set_cur(p, img);
            self.prefetch();
        } else {
            self.failed = false;
            self.request_full(p);
        }
    }

    fn is_video_cur(&self) -> bool {
        self.files.get(self.idx).map_or(false, |p| gallery::is_video(p))
    }

    fn stop_video(&mut self) {
        if let Some(v) = &self.video {
            v.pause();
        }
        self.video = None;
        self.video_tex = None;
        self.scrub = None;
        self.ff = false;
        self.ff_hold = None;
        self.ff_consumed = false;
    }

    /// Videoyu açar ve hemen oynatmaya başlar.
    fn load_video(&mut self, p: PathBuf) {
        self.cur = None;
        self.cur_path = None;
        self.tex = None;
        self.tex_dirty = false;
        self.failed = false;
        match VideoPlayer::open(&p) {
            Some(v) => {
                v.set_rate(1.0);
                self.video = Some(v);
            }
            None => self.failed = true,
        }
        self.prefetch();
    }

    /// Oynat / duraklat (video bittiyse başa sarar).
    fn toggle_video(&mut self) {
        if let Some(v) = &mut self.video {
            if v.is_playing() {
                v.pause();
            } else {
                if v.ended() {
                    v.seek(0.0);
                }
                v.set_rate(1.0);
            }
        }
    }

    /// Önceki ve sonraki resmi arka planda çözer.
    fn prefetch(&mut self) {
        let n = self.files.len();
        if n < 2 {
            return;
        }
        for d in [1usize, n - 1] {
            let p = self.files[(self.idx + d) % n].clone();
            self.request_full(p);
        }
    }

    fn pump(&mut self, ctx: &Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Full(p, r) => {
                    self.pending.remove(&p);
                    let is_cur = self.files.get(self.idx) == Some(&p);
                    match r {
                        Ok(img) => {
                            self.cache.insert(p.clone(), img.clone());
                            if is_cur {
                                self.set_cur(p, img);
                                self.prefetch();
                            }
                        }
                        Err(_) => {
                            if is_cur {
                                self.cur = None;
                                self.cur_path = None;
                                self.tex = None;
                                self.failed = true;
                            }
                        }
                    }
                }
                Msg::Thumb(p, ci) => {
                    let t = ctx.load_texture(format!("t:{}", p.display()), ci, TextureOptions::LINEAR);
                    self.thumbs.insert(p, t);
                    if self.thumbs.len() > THUMB_CAP {
                        self.thumbs.clear();
                        self.thumb_req.clear();
                    }
                }
                Msg::Scan(id, list) => {
                    if id == self.scan_id {
                        self.scan_list = list;
                        self.scanning = false;
                        if self.scan_list.is_empty() {
                            self.workers.list_dir(self.dir.clone());
                        }
                    }
                }
                Msg::Dir(d, r) => {
                    if d == self.dir {
                        self.listing = false;
                        match r {
                            Ok(mut v) => {
                                // Tarama sonuçlarında gezerken yalnızca resimler gösterilir
                                if self.scanning || !self.scan_list.is_empty() {
                                    v.retain(|e| !e.is_dir);
                                }
                                self.entries = v;
                                self.dir_err = None;
                            }
                            Err(e) => {
                                self.entries.clear();
                                self.dir_err = Some(e);
                            }
                        }
                    }
                }
                Msg::Siblings(list) => {
                    let keep = self.files.get(self.idx).cloned();
                    // Eski bir klasörden gecikmiş liste geldiyse yok say
                    let ok = keep
                        .as_ref()
                        .map_or(false, |k| list.iter().any(|f| f.file_name() == k.file_name()));
                    if !ok {
                        continue;
                    }
                    self.files = list;
                    self.idx = keep
                        .and_then(|k| self.files.iter().position(|f| f.file_name() == k.file_name()))
                        .unwrap_or(0);
                    self.scroll_cur = true;
                    self.prefetch();
                }
            }
        }
    }

    fn rebuild_tex(&mut self, ctx: &Context) {
        self.tex_dirty = false;
        let Some(img) = self.cur.clone() else {
            self.tex = None;
            return;
        };
        let (w, h) = (img.width(), img.height());
        let max_side = ctx.input(|i| i.max_texture_side).min(8192) as u32;
        let ci = if w.max(h) > max_side {
            let k = max_side as f32 / w.max(h) as f32;
            let (nw, nh) = (((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1));
            let small = imageops::resize(&*img, nw, nh, imageops::FilterType::Triangle);
            egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw())
        } else {
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw())
        };
        self.tex = Some(ctx.load_texture("img", ci, TextureOptions::LINEAR));
    }

    // ---------- Eylemler ----------

    fn reset_view(&mut self) {
        self.zoom_v = 0.0;
        self.rot_v = 0.0;
        self.pan = Vec2::ZERO;
    }

    fn reset_edit(&mut self) {
        self.crop = false;
        self.cn = def_crop();
        self.dm = None;
        self.reset_view();
        self.undo.clear();
        self.stroke.clear();
    }

    fn show_idx(&mut self, n: usize) {
        if n >= self.files.len() {
            return;
        }
        self.reset_edit();
        self.idx = n;
        self.scroll_cur = true;
        self.load_current();
    }

    fn go(&mut self, d: isize) {
        let n = self.files.len() as isize;
        if n == 0 {
            return;
        }
        self.show_idx((((self.idx as isize + d) % n + n) % n) as usize);
    }

    fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
        self.workers.ctx.request_repaint_after(Duration::from_millis(TOAST_MS + 100));
    }

    fn toggle_lang(&mut self) {
        self.lang = self.lang.other();
        self.lang.save();
    }

    fn push_undo(&mut self) {
        if let Some(c) = &self.cur {
            self.undo.push(c.clone());
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
    }

    fn undo_last(&mut self) {
        if let Some(a) = self.undo.pop() {
            self.cur = Some(a);
            self.tex_dirty = true;
        }
    }

    /// `cw`: saat yönünde (sağa) döndür
    fn rot90(&mut self, cw: bool) {
        let Some(c) = self.cur.clone() else { return };
        self.push_undo();
        let n = if cw { imageops::rotate90(&*c) } else { imageops::rotate270(&*c) };
        self.cur = Some(Arc::new(n));
        self.tex_dirty = true;
    }

    fn flip(&mut self, horizontal: bool) {
        let Some(c) = self.cur.clone() else { return };
        self.push_undo();
        let n = if horizontal { imageops::flip_horizontal(&*c) } else { imageops::flip_vertical(&*c) };
        self.cur = Some(Arc::new(n));
        self.tex_dirty = true;
    }

    fn del(&mut self) {
        let Some(p) = self.files.get(self.idx).cloned() else { return };
        if gallery::trash(&p).is_err() {
            self.toast(self.lang.t("Çöp Kutusu'na atılamadı", "Could not move to Trash"));
            return;
        }
        self.cache.remove(&p);
        self.thumbs.remove(&p);
        self.thumb_req.remove(&p);
        self.files.remove(self.idx);
        self.reset_edit();
        if self.files.is_empty() {
            self.stop_video();
            self.idx = 0;
            self.cur = None;
            self.cur_path = None;
            self.tex = None;
            self.screen = Screen::Browse;
            self.workers.list_dir(self.dir.clone());
        } else {
            self.idx = self.idx.min(self.files.len() - 1);
            self.load_current();
        }
    }

    /// Resmin (varsa kırpma alanı, çizgiler, döndürme/çevirme dahil) kopyasını sormadan kaydeder.
    fn save(&mut self) {
        let (Some(cur), Some(path)) = (self.cur.clone(), self.cur_path.clone()) else { return };
        let part: RgbaImage = if self.crop {
            let (w, h) = (cur.width(), cur.height());
            let x = ((self.cn.min.x * w as f32).round() as u32).min(w.saturating_sub(1));
            let y = ((self.cn.min.y * h as f32).round() as u32).min(h.saturating_sub(1));
            let cw = ((self.cn.width() * w as f32).round() as u32).max(1).min(w - x);
            let ch = ((self.cn.height() * h as f32).round() as u32).max(1).min(h - y);
            imageops::crop_imm(&*cur, x, y, cw, ch).to_image()
        } else {
            (*cur).clone()
        };
        match gallery::save_copy(&path, part, self.lang.t("kopya", "copy"), self.lang.t("resim", "image")) {
            Ok(dest) => {
                self.crop = false;
                self.cn = def_crop();
                let name = dest.file_name().map(|s| gallery::nfc_tr(&s.to_string_lossy())).unwrap_or_default();
                self.toast(format!("{}: {name}", self.lang.t("Kaydedildi", "Saved")));
                self.workers.list(path);
            }
            Err(e) => self.toast(format!("{}: {e}", self.lang.t("Kaydedilemedi", "Could not save"))),
        }
    }

    fn toggle_fullscreen(&mut self, ctx: &Context) {
        self.fullscreen = !self.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
    }

    // ---------- Klasör gezgini eylemleri ----------

    /// Gezinme yalnızca ana dizin (ev klasörü) ve altında serbest
    fn in_root(&self, p: &Path) -> bool {
        p.starts_with(&self.root) || p.starts_with(&self.root_canon)
    }

    fn navigate(&mut self, dir: PathBuf, record: bool) {
        let dir = if self.in_root(&dir) { dir } else { self.root.clone() };
        if record && dir != self.dir {
            self.hist_back.push(self.dir.clone());
            self.hist_fwd.clear();
        }
        self.dir = dir.clone();
        self.view_scan = false;
        self.entries.clear();
        self.listing = true;
        self.dir_err = None;
        self.thumbs.clear();
        self.thumb_req.clear();
        self.workers.list_dir(dir);
    }

    fn go_back(&mut self) {
        if self.view_scan {
            self.view_scan = false; // tarama ekranından klasör görünümüne dön
            return;
        }
        if let Some(d) = self.hist_back.pop() {
            self.hist_fwd.push(self.dir.clone());
            self.navigate(d, false);
        }
    }

    fn go_fwd(&mut self) {
        if let Some(d) = self.hist_fwd.pop() {
            self.hist_back.push(self.dir.clone());
            self.navigate(d, false);
        }
    }

    fn go_up(&mut self) {
        if let Some(p) = self.dir.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()) {
            self.navigate(p, true);
        }
    }

    /// Ana dizini (ev klasörü) ve tüm alt dizinlerini tarar; Applications taranmaz.
    fn start_scan(&mut self) {
        let root = home_dir();
        // Sistem / uygulama klasörleri taranmaz
        let mut skip: Vec<PathBuf> = ["Applications", "Library", "AppData", "snap"].iter().map(|n| root.join(n)).collect();
        for p in ["/System", "/Library", "/Applications", "/usr", "/bin", "/sbin", "/private", "/var", "/etc", "/dev", "/opt", "/cores", "/Volumes"] {
            skip.push(PathBuf::from(p));
        }
        self.scan_cancel.store(true, Ordering::Relaxed);
        self.scan_cancel = Arc::new(AtomicBool::new(false));
        self.scan_found = Arc::new(AtomicUsize::new(0));
        self.scan_id += 1;
        self.scan_root = root.clone();
        self.scan_list.clear();
        self.scanning = true;
        self.view_scan = true;
        self.workers.list_dir(self.dir.clone());
        self.workers.scan(self.scan_id, root, skip, self.scan_cancel.clone(), self.scan_found.clone());
    }

    fn cancel_scan(&mut self) {
        self.scan_cancel.store(true, Ordering::Relaxed);
        self.scanning = false;
        if self.scan_list.is_empty() {
            self.view_scan = false;
        }
    }

    fn close_scan(&mut self) {
        self.cancel_scan();
        self.scan_list.clear();
        self.view_scan = false;
        self.workers.list_dir(self.dir.clone());
    }

    /// Görüntüleyiciden gezgine dön (açık resmin klasörüne gider, listeyi yeniler).
    fn show_browser(&mut self, ctx: &Context) {
        self.play = false;
        self.stop_video();
        if self.fullscreen {
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        self.screen = Screen::Browse;
        if let Some(p) = self.files.get(self.idx).cloned() {
            if p.parent() != Some(self.dir.as_path()) {
                let d = parent_dir(&p);
                if d != self.dir {
                    self.navigate(d, true);
                    return;
                }
            }
        }
        self.workers.list_dir(self.dir.clone());
    }

    /// Gezginde bir resme tıklanınca: klasördeki resimler şeride dizilir, tıklanan açılır.
    fn open_from_browser(&mut self, n: usize) {
        let Some(target) = self.entries.get(n).map(|e| e.path.clone()) else { return };
        let imgs: Vec<PathBuf> = self.entries.iter().filter(|e| !e.is_dir).map(|e| e.path.clone()).collect();
        let idx = imgs.iter().position(|p| *p == target).unwrap_or(0);
        self.files = imgs;
        self.idx = idx;
        self.cur = None;
        self.cur_path = None;
        self.tex = None;
        self.failed = false;
        self.reset_edit();
        self.screen = Screen::Viewer;
        self.scroll_cur = true;
        self.load_current();
    }

    // ---------- Girdiler ----------

    fn handle_drop(&mut self, ctx: &Context) {
        // Sürükle-bırak + Finder'dan çift tık / "Birlikte aç" / Dock'a sürükleme
        let mut paths: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        paths.extend(crate::macos_open::take());
        if let Some(p) = paths.into_iter().find(|p| p.is_file() || p.is_dir()) {
            if p.is_dir() {
                self.screen = Screen::Browse;
                self.navigate(p, true);
            } else {
                self.open(p);
            }
        }
    }

    fn handle_keys(&mut self, ctx: &Context) {
        if self.screen == Screen::Browse {
            let (o, bs) = ctx.input_mut(|i| {
                (i.consume_key(Modifiers::COMMAND, Key::O), i.consume_key(Modifiers::NONE, Key::Backspace))
            });
            if o {
                self.open_dialog();
            }
            if bs {
                self.go_up();
            }
            return;
        }
        let (left, right, up, down, esc, z, o, s, b) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::COMMAND, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::S),
                i.consume_key(Modifiers::COMMAND, Key::B),
            )
        });
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space)) {
            self.toggle_video();
        }
        if left {
            self.go(-1);
        }
        if right {
            self.go(1);
        }
        if up {
            self.rot90(false); // yukarı: sola döndür
        }
        if down {
            self.rot90(true); // aşağı: sağa döndür
        }
        if esc {
            if self.crop {
                self.crop = false;
            } else if self.pen {
                self.pen = false;
                self.stroke.clear();
            } else if self.fullscreen {
                self.fullscreen = false;
                self.play = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else {
                self.show_browser(ctx); // başka açık bir şey yoksa klasörlere dön
                return;
            }
        }
        if b {
            self.show_browser(ctx);
            return;
        }
        if z {
            self.undo_last();
        }
        if o {
            self.open_dialog();
        }
        if s {
            self.save();
        }
    }

    fn tick_slideshow(&mut self, ctx: &Context) {
        if self.screen != Screen::Viewer || !self.play || self.files.len() < 2 {
            return;
        }
        // Videoda slayt gösterisi, video bitince bir sonrakine geçer
        if self.video.is_some() {
            if self.video.as_ref().map_or(false, |v| v.ended()) {
                self.go(1);
                self.last_slide = Instant::now();
            }
            ctx.request_repaint_after(Duration::from_millis(300));
            return;
        }
        let d = Duration::from_secs(SLIDE_SECS);
        let el = self.last_slide.elapsed();
        if el >= d {
            self.go(1);
            self.last_slide = Instant::now();
            ctx.request_repaint_after(d);
        } else {
            ctx.request_repaint_after(d - el);
        }
    }

    // ---------- Araç çubuğu ----------

    /// Üst çubuk (58 px): solda kaydırılabilir düğmeler; sağa sabit: Aç… + büyütme / döndürme kaydırıcıları.
    fn toolbar(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        let lg = self.lang;
        const RIGHT_W: f32 = 470.0;
        let w = ui.available_width();
        ui.allocate_ui_with_layout(vec2(w, BAR_INNER), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
            let left_w = (ui.available_width() - RIGHT_W).max(120.0);
            ui.allocate_ui_with_layout(vec2(left_w, BAR_INNER), Layout::left_to_right(Align::Center), |ui| {
                egui::ScrollArea::horizontal().id_source("arac_dugmeleri").auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        if tb(ui, lg.t("Klasörler", "Folders"), lg.t("Klasörlere dön (Esc / ⌘B)", "Back to folders (Esc / ⌘B)"), false, None) {
                            self.show_browser(ctx);
                        }
                        ui.separator();
                        if tb(ui, lg.t("Önceki", "Previous"), lg.t("Önceki resim (←)", "Previous image (←)"), false, None) {
                            self.go(-1);
                        }
                        if tb(ui, lg.t("Sonraki", "Next"), lg.t("Sonraki resim (→)", "Next image (→)"), false, None) {
                            self.go(1);
                        }
                        ui.separator();
                        if tb(ui, lg.t("Sola", "Left"), lg.t("Sola döndür (↑)", "Rotate left (↑)"), false, None) {
                            self.rot90(false);
                        }
                        if tb(ui, lg.t("Sağa", "Right"), lg.t("Sağa döndür (↓)", "Rotate right (↓)"), false, None) {
                            self.rot90(true);
                        }
                        if tb(ui, lg.t("Yatay", "Flip H"), lg.t("Yatay çevir", "Flip horizontally"), false, None) {
                            self.flip(true);
                        }
                        if tb(ui, lg.t("Dikey", "Flip V"), lg.t("Dikey çevir", "Flip vertically"), false, None) {
                            self.flip(false);
                        }
                        if tb(ui, lg.t("Kırp", "Crop"), lg.t("Kırp", "Crop"), self.crop, None) {
                            self.crop = !self.crop;
                            self.cn = def_crop();
                            self.dm = None;
                            if self.crop {
                                self.pen = false;
                                self.stroke.clear();
                                self.reset_view();
                            }
                        }
                        if tb(ui, lg.t("Kalem", "Pen"), lg.t("Kırmızı kalem", "Red pen"), self.pen, Some(PEN_RED)) {
                            self.pen = !self.pen;
                            self.stroke.clear();
                            if self.pen {
                                self.crop = false;
                                self.reset_view();
                            }
                        }
                        ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
                            if tb(ui, lg.t("Geri al", "Undo"), lg.t("Geri al (⌘Z)", "Undo (⌘Z)"), false, None) {
                                self.undo_last();
                            }
                        });
                        if tb(ui, lg.t("Tam ekran", "Fullscreen"), lg.t("Tam ekran (Esc ile çık)", "Fullscreen (Esc to exit)"), self.fullscreen, None) {
                            self.toggle_fullscreen(ctx);
                        }
                        if tb(ui, lg.t("Slayt", "Slideshow"), lg.t("Slayt gösterisi (3 sn)", "Slideshow (3 s)"), self.play, None) {
                            self.play = !self.play;
                            self.last_slide = Instant::now();
                            if self.play != self.fullscreen {
                                self.toggle_fullscreen(ctx);
                            }
                        }
                        ui.separator();
                        if tb(ui, lg.t("Kaydet", "Save"), lg.t("Kopya olarak kaydet (⌘S)", "Save as copy (⌘S)"), false, None) {
                            self.save();
                        }
                        if tb(ui, lg.t("Çöp", "Trash"), lg.t("Çöp Kutusu'na at", "Move to Trash"), false, None) {
                            self.del();
                        }
                        if tb(ui, lg.t("Finder", "Finder"), lg.t("Finder'da göster", "Show in Finder"), false, None) {
                            if let Some(p) = self.files.get(self.idx) {
                                let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
                            }
                        }
                    });
                });
            });
            ui.separator();
            if tb(ui, lg.t("EN", "TR"), lg.t("İngilizceye geç (English)", "Switch to Turkish (Türkçe)"), false, None) {
                self.toggle_lang();
            }
            if tb(ui, lg.t("Aç…", "Open…"), lg.t("Resim aç (⌘O)", "Open image (⌘O)"), false, None) {
                self.open_dialog();
            }
            self.slider_ui(ui);
        });
    }

    // ---------- Alt şerit ----------

    /// Küçük resim şeridi: pencerenin tam genişliğini ve şerit panelinin tam yüksekliğini kaplar.
    fn bottom_ui(&mut self, ui: &mut egui::Ui) {
        let mut click: Option<usize> = None;
        let h = ui.available_height();
        let top = ui.max_rect().top();
        ui.painter().hline(ui.max_rect().x_range(), top, Stroke::new(1.0, Color32::from_gray(52)));
        let pad = ((h - 80.0) / 2.0 - 4.0).max(0.0);
        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(pad);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.add_space(8.0);
                for n in 0..self.files.len() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(80.0, 80.0), Sense::click());
                    if ui.is_rect_visible(rect) {
                        let p = self.files[n].clone();
                        if let Some(t) = self.thumbs.get(&p) {
                            let sz = t.size_vec2();
                            let k = (80.0 / sz.x).min(80.0 / sz.y);
                            ui.painter().image(
                                t.id(),
                                Rect::from_center_size(rect.center(), sz * k),
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        } else {
                            ui.painter().rect_filled(rect, 4.0, Color32::from_gray(40));
                            self.workers.want_thumb(&p);
                        }
                    }
                    if gallery::is_video(&self.files[n]) {
                        play_badge(ui.painter(), rect);
                    }
                    if n == self.idx {
                        ui.painter().rect_stroke(rect.expand(2.0), 6.0, Stroke::new(3.0, BLUE));
                        if self.scroll_cur {
                            ui.scroll_to_rect(rect, Some(Align::Center));
                        }
                    }
                    if resp.clicked() {
                        click = Some(n);
                    }
                }
                self.scroll_cur = false;
            });
        });
        if let Some(n) = click {
            self.show_idx(n);
        }
    }

    /// Çizgi üzerinde küçük yuvarlak: sağa çek → büyüt / sağa döndür, sola çek → küçült / sola döndür.
    fn slider_ui(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        let enabled = !(self.crop || self.pen);
        ui.vertical(|ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.spacing_mut().slider_width = 150.0;

                ui.horizontal(|ui| {
                    let label = format!("%{}", (ZOOM_BASE.powf(self.zoom_v) * 100.0).round() as i32);
                    if reset_label(ui, &label, lg.t("Sıfırla (%100)", "Reset (100%)")) {
                        self.zoom_v = 0.0;
                    }
                    ui.label(RichText::new(lg.t("küçült", "smaller")).small().weak());
                    let r = ui.add(egui::Slider::new(&mut self.zoom_v, -1.0..=1.0).show_value(false));
                    if r.changed() && self.zoom_v.abs() < 0.02 {
                        self.zoom_v = 0.0;
                    }
                    ui.label(RichText::new(lg.t("büyüt", "larger")).small().weak());
                });

                ui.horizontal(|ui| {
                    let label = format!("{}°", self.rot_v.round() as i32);
                    if reset_label(ui, &label, lg.t("Sıfırla (0°)", "Reset (0°)")) {
                        self.rot_v = 0.0;
                    }
                    ui.label(RichText::new(lg.t("sola", "left")).small().weak());
                    let r = ui.add(egui::Slider::new(&mut self.rot_v, -180.0..=180.0).show_value(false));
                    if r.changed() && self.rot_v.abs() < 3.0 {
                        self.rot_v = 0.0;
                    }
                    ui.label(RichText::new(lg.t("sağa", "right")).small().weak());
                });
            });
        });
    }

    // ---------- Klasör gezgini ----------

    /// Üstteki gezinme çubuğu (pencereye ortalı): geri / ileri / yukarı / Tara / Pictures + tıklanabilir yol.
    fn browser_bar(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        let pics = self.pics.clone();
        let mut act = 0u8; // 1 geri, 2 ileri, 3 yukarı, 4 Pictures, 5 klasör seç, 6 resim aç, 7 tara, 8 durdur, 9 dil
        let mut nav: Option<PathBuf> = None;

        // Yol çubuğu ana dizinden başlar
        let base = if self.dir.starts_with(&self.root_canon) && !self.dir.starts_with(&self.root) {
            self.root_canon.clone()
        } else {
            self.root.clone()
        };
        let mut crumbs: Vec<(String, PathBuf)> = Vec::new();
        let home_label = base
            .file_name()
            .map(|n| gallery::nfc_tr(&n.to_string_lossy()))
            .unwrap_or_else(|| lg.t("Ana dizin", "Home").to_string());
        let mut acc = base.clone();
        crumbs.push((home_label, acc.clone()));
        if let Ok(rel) = self.dir.strip_prefix(&base) {
            for c in rel.components() {
                acc.push(c.as_os_str());
                crumbs.push((gallery::nfc_tr(&c.as_os_str().to_string_lossy()), acc.clone()));
            }
        }
        let last = crumbs.len().saturating_sub(1);
        let can_back = !self.hist_back.is_empty() || self.view_scan;
        let can_fwd = !self.hist_fwd.is_empty();
        let can_up = self.dir.parent().map_or(false, |p| !p.as_os_str().is_empty() && self.in_root(p));
        let at_pics = self.dir == pics;
        let scanning = self.scanning;

        ui.vertical_centered(|ui| {
            let w = ui.available_width().min(1000.0);
            ui.allocate_ui_with_layout(vec2(w, BAR_INNER), Layout::left_to_right(Align::Center), |ui| {
                ui.add_enabled_ui(can_back, |ui| {
                    if tb(ui, lg.t("Geri", "Back"), lg.t("Önceki klasör", "Previous folder"), false, None) {
                        act = 1;
                    }
                });
                ui.add_enabled_ui(can_fwd, |ui| {
                    if tb(ui, lg.t("İleri", "Forward"), lg.t("Sonraki klasör", "Next folder"), false, None) {
                        act = 2;
                    }
                });
                ui.add_enabled_ui(can_up, |ui| {
                    if tb(ui, lg.t("Yukarı", "Up"), lg.t("Üst klasör (Backspace)", "Parent folder (Backspace)"), false, None) {
                        act = 3;
                    }
                });
                if scanning {
                    if tb(ui, lg.t("Durdur", "Stop"), lg.t("Taramayı durdur", "Stop scanning"), true, None) {
                        act = 8;
                    }
                } else if tb(ui, lg.t("Tara", "Scan"), lg.t("Ana dizindeki tüm klasörleri ve alt dizinleri tara ", "Scan all folders and subfolders in your home folder"), false, None) {
                    act = 7;
                }
                if tb(ui, lg.t("Pictures", "Pictures"), lg.t("Pictures klasörüne git", "Go to the Pictures folder"), at_pics, None) {
                    act = 4;
                }
                ui.separator();

                let cw = (ui.available_width() - 280.0).max(120.0);
                ui.allocate_ui(vec2(cw, 30.0), |ui| {
                    egui::ScrollArea::horizontal()
                        .id_source("yol_cubugu")
                        .stick_to_right(true)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (i, (label, path)) in crumbs.iter().enumerate() {
                                    if i > 0 {
                                        ui.label(RichText::new(">").weak());
                                    }
                                    let mut t = RichText::new(label.as_str()).size(15.0);
                                    if i == last {
                                        t = t.color(BLUE).strong();
                                    }
                                    if ui.add(egui::Button::new(t).frame(false)).clicked() && i != last {
                                        nav = Some(path.clone());
                                    }
                                }
                            });
                        });
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if tb(ui, lg.t("Aç…", "Open…"), lg.t("Resim aç (⌘O)", "Open image (⌘O)"), false, None) {
                        act = 6;
                    }
                    if tb(ui, lg.t("Klasör seç…", "Choose folder…"), lg.t("Ana dizin içinde başka bir klasöre git", "Go to another folder inside your home folder"), false, None) {
                        act = 5;
                    }
                    if tb(ui, lg.t("EN", "TR"), lg.t("İngilizceye geç (English)", "Switch to Turkish (Türkçe)"), false, None) {
                        act = 9;
                    }
                });
            });
        });

        match act {
            1 => self.go_back(),
            2 => self.go_fwd(),
            3 => self.go_up(),
            4 => self.navigate(pics, true),
            5 => {
                if let Some(p) = rfd::FileDialog::new().set_directory(&self.dir).pick_folder() {
                    if self.in_root(&p) {
                        self.navigate(p, true);
                    } else {
                        self.toast(self.lang.t("Yalnızca ana dizin içinde gezilebilir", "You can only browse inside your home folder"));
                    }
                }
            }
            6 => self.open_dialog(),
            7 => self.start_scan(),
            8 => self.cancel_scan(),
            9 => self.toggle_lang(),
            _ => {}
        }
        if let Some(p) = nav {
            self.navigate(p, true);
        }
    }

    /// Ana ekranın üstündeki tarama satırı: ilerleme, sonuç özeti, klasörler arasında gezinme.
    fn scan_header(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        let mut go: Option<PathBuf> = None;
        let mut show_list = false;
        let mut close = false;
        let total: usize = self.scan_list.iter().map(|x| x.count).sum();
        let cur_i = self.scan_list.iter().position(|x| x.dir == self.dir);
        let prev = cur_i.and_then(|i| i.checked_sub(1)).and_then(|i| self.scan_list.get(i)).map(|x| x.dir.clone());
        let next = match cur_i {
            Some(i) => self.scan_list.get(i + 1),
            None => self.scan_list.first(),
        }
        .map(|x| x.dir.clone());
        let scanning = self.scanning;
        let found = self.scan_found.load(Ordering::Relaxed);
        let n_dirs = self.scan_list.len();
        let in_list = self.view_scan;

        ui.horizontal(|ui| {
            if scanning {
                ui.spinner();
                ui.label(if lg == Lang::Tr { format!("Taranıyor… {found} resim bulundu") } else { format!("Scanning… {found} images found") });
            } else {
                let summary = if lg == Lang::Tr {
                    format!("Taranan klasörler: {n_dirs} klasör, {total} resim")
                } else {
                    format!("Scanned folders: {n_dirs} folders, {total} images")
                };
                ui.label(RichText::new(summary).strong());
                ui.add_enabled_ui(!in_list, |ui| {
                    if tb(ui, lg.t("Liste", "List"), lg.t("Taranan klasörlerin listesine dön", "Back to the scanned folders list"), false, None) {
                        show_list = true;
                    }
                });
                ui.add_enabled_ui(prev.is_some(), |ui| {
                    if tb(ui, lg.t("Önceki klasör", "Previous folder"), lg.t("Listede bir önceki klasör", "Previous folder in the list"), false, None) {
                        go = prev.clone();
                    }
                });
                ui.add_enabled_ui(next.is_some(), |ui| {
                    if tb(ui, lg.t("Sonraki klasör", "Next folder"), lg.t("Listede bir sonraki klasör", "Next folder in the list"), false, None) {
                        go = next.clone();
                    }
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if tb(ui, lg.t("Kapat", "Close"), lg.t("Tarama sonuçlarını kapat", "Close scan results"), false, None) {
                    close = true;
                }
            });
        });

        if close {
            self.close_scan();
        } else if show_list {
            self.view_scan = true;
        } else if let Some(p) = go {
            self.navigate(p, true);
        }
    }

    /// Ana ekranda tarama sonuçları: resim içeren klasörler; her satırda altı kart.
    fn scan_view(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        const COLS: usize = 6;
        const GAP: f32 = 14.0;
        const LABEL_H: f32 = 22.0;

        let avail = ui.available_width() - 16.0;
        let c = ((avail - GAP * (COLS as f32 - 1.0)) / COLS as f32).floor().max(48.0);
        let used = c * COLS as f32 + GAP * (COLS as f32 - 1.0);
        let row_h = c + LABEL_H + GAP;
        ui.spacing_mut().item_spacing.y = 0.0;

        let mut go: Option<PathBuf> = None;
        let scanning = self.scanning;

        egui::ScrollArea::vertical().id_source("tarama_gorunumu").auto_shrink([false, false]).show(ui, |ui| {
            let title = if lg == Lang::Tr { "Taranan klasörler" } else { "Scanned folders" };
            ui.label(RichText::new(format!("{title} ({})", self.scan_list.len())).strong().size(16.0));
            ui.add_space(8.0);
            if self.scan_list.is_empty() {
                let msg = if scanning { lg.t("Taranıyor…", "Scanning…") } else { lg.t("Resim içeren klasör bulunamadı", "No folders with images found") };
                ui.add_space(24.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(msg).size(16.0).color(Color32::GRAY)));
                return;
            }
            let rows = (self.scan_list.len() + COLS - 1) / COLS;
            for r in 0..rows {
                let (row_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                let x0 = row_rect.min.x + ((row_rect.width() - used) / 2.0).max(0.0);
                for k in 0..COLS {
                    let n = r * COLS + k;
                    let Some(it) = self.scan_list.get(n) else { break };
                    let rect = Rect::from_min_size(pos2(x0 + k as f32 * (c + GAP), row_rect.min.y), vec2(c, c + LABEL_H));
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let resp = ui
                        .interact(rect, ui.id().with(("tarama", n)), Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let tile = Rect::from_min_size(rect.min, vec2(c, c));
                    let painter = ui.painter();
                    painter.rect_filled(tile, 8.0, Color32::from_gray(if resp.hovered() { 48 } else { 34 }));
                    if let Some(t) = self.thumbs.get(&it.first) {
                        let sz = t.size_vec2();
                        let area = tile.shrink(6.0);
                        let kk = (area.width() / sz.x).min(area.height() / sz.y);
                        painter.image(
                            t.id(),
                            Rect::from_center_size(tile.center(), sz * kk),
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        draw_folder(painter, tile);
                        self.workers.want_thumb(&it.first);
                    }
                    if resp.hovered() {
                        painter.rect_stroke(tile, 8.0, Stroke::new(2.0, BLUE));
                    }
                    // resim sayısı rozeti
                    let badge_text = match (lg, it.count) {
                        (Lang::Tr, n) => format!("{n} resim"),
                        (Lang::En, 1) => "1 image".to_string(),
                        (Lang::En, n) => format!("{n} images"),
                    };
                    let g = painter.layout_no_wrap(badge_text, FontId::proportional(11.5), Color32::WHITE);
                    let sz = g.size();
                    let badge = Rect::from_min_max(
                        pos2(tile.max.x - sz.x - 14.0, tile.max.y - sz.y - 12.0),
                        pos2(tile.max.x - 6.0, tile.max.y - 6.0),
                    );
                    painter.rect_filled(badge, 6.0, Color32::from_black_alpha(190));
                    painter.galley(badge.min + vec2(4.0, 3.0), g, Color32::WHITE);

                    let name = if it.dir == self.scan_root {
                        it.dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
                    } else {
                        it.dir
                            .strip_prefix(&self.scan_root)
                            .map(|r| r.display().to_string())
                            .unwrap_or_else(|_| it.dir.display().to_string())
                    };
                    label_under(painter, tile, gallery::nfc_tr(&name), c);
                    if resp.clicked() {
                        go = Some(it.dir.clone());
                    }
                    resp.on_hover_text(gallery::nfc_tr(&it.dir.display().to_string()));
                }
            }
        });

        if let Some(p) = go {
            self.navigate(p, true);
        }
    }

    /// Klasör içeriği: her satırda altı ön izleme. Klasöre tıklayınca içine girer, resme tıklayınca açar.
    fn browser_grid(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        const COLS: usize = 6;
        const GAP: f32 = 14.0;
        const LABEL_H: f32 = 22.0;

        if self.entries.is_empty() {
            let msg = if let Some(e) = &self.dir_err {
                format!("{}: {e}", lg.t("Klasör açılamadı", "Could not open folder"))
            } else if self.listing {
                lg.t("Yükleniyor…", "Loading…").to_string()
            } else {
                lg.t("Bu klasörde resim ya da alt klasör yok", "No images or subfolders in this folder").to_string()
            };
            let rect = ui.available_rect_before_wrap();
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(18.0), Color32::GRAY);
            ui.allocate_rect(rect, Sense::hover());
            return;
        }

        let avail = ui.available_width() - 16.0; // kaydırma çubuğuna pay
        let c = ((avail - GAP * (COLS as f32 - 1.0)) / COLS as f32).floor().max(48.0);
        let used = c * COLS as f32 + GAP * (COLS as f32 - 1.0);
        let row_h = c + LABEL_H + GAP;
        let rows = (self.entries.len() + COLS - 1) / COLS;
        ui.spacing_mut().item_spacing.y = 0.0;

        let mut open_img: Option<usize> = None;
        let mut open_dir: Option<PathBuf> = None;

        egui::ScrollArea::vertical()
            .id_source(("izgara", &self.dir))
            .auto_shrink([false, false])
            .show_rows(ui, row_h, rows, |ui, range| {
                for r in range {
                    let (row_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                    let x0 = row_rect.min.x + ((row_rect.width() - used) / 2.0).max(0.0);
                    for k in 0..COLS {
                        let n = r * COLS + k;
                        let Some(e) = self.entries.get(n) else { break };
                        let rect = Rect::from_min_size(
                            pos2(x0 + k as f32 * (c + GAP), row_rect.min.y),
                            vec2(c, c + LABEL_H),
                        );
                        if !ui.is_rect_visible(rect) {
                            continue;
                        }
                        let resp = ui
                            .interact(rect, ui.id().with(("hucre", n)), Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        let hov = resp.hovered();
                        let tile = Rect::from_min_size(rect.min, vec2(c, c));
                        let painter = ui.painter();
                        painter.rect_filled(tile, 8.0, Color32::from_gray(if hov { 48 } else { 34 }));
                        if e.is_dir {
                            draw_folder(painter, tile);
                        } else if let Some(t) = self.thumbs.get(&e.path) {
                            let sz = t.size_vec2();
                            let area = tile.shrink(6.0);
                            let kk = (area.width() / sz.x).min(area.height() / sz.y);
                            painter.image(
                                t.id(),
                                Rect::from_center_size(tile.center(), sz * kk),
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        } else {
                            self.workers.want_thumb(&e.path);
                        }
                        if !e.is_dir && gallery::is_video(&e.path) {
                            play_badge(painter, tile);
                        }
                        if hov {
                            painter.rect_stroke(tile, 8.0, Stroke::new(2.0, BLUE));
                        }
                        let mut job =
                            egui::text::LayoutJob::simple(e.name.clone(), FontId::proportional(12.5), Color32::from_gray(205), c);
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        let g = painter.layout_job(job);
                        painter.galley(pos2(rect.center().x - g.size().x / 2.0, tile.max.y + 4.0), g, Color32::WHITE);

                        if resp.clicked() {
                            if e.is_dir {
                                open_dir = Some(e.path.clone());
                            } else {
                                open_img = Some(n);
                            }
                        }
                        resp.on_hover_text(e.name.clone());
                    }
                }
            });

        if let Some(d) = open_dir {
            self.navigate(d, true);
        } else if let Some(n) = open_img {
            self.open_from_browser(n);
        }
    }

    // ---------- Resim alanı ----------

    fn crop_screen(&self, disp: Rect) -> Rect {
        Rect::from_min_max(
            pos2(disp.min.x + self.cn.min.x * disp.width(), disp.min.y + self.cn.min.y * disp.height()),
            pos2(disp.min.x + self.cn.max.x * disp.width(), disp.min.y + self.cn.max.y * disp.height()),
        )
    }

    /// Köşe/kenardan tutup boyutlandır, içinden tutup taşı
    fn crop_interact(&mut self, ctx: &Context, resp: &egui::Response, disp: Rect) {
        if resp.drag_started() {
            self.dm = None;
            if let Some(p) = ctx.input(|i| i.pointer.press_origin()) {
                let c = self.crop_screen(disp);
                let tol = 16.0;
                if c.expand(tol).contains(p) {
                    let mut m = CropMode {
                        l: (p.x - c.min.x).abs() < tol,
                        r: (p.x - c.max.x).abs() < tol,
                        t: (p.y - c.min.y).abs() < tol,
                        b: (p.y - c.max.y).abs() < tol,
                        mv: false,
                    };
                    m.mv = !(m.l || m.r || m.t || m.b);
                    self.dm = Some(m);
                    self.d0 = self.cn;
                    self.drag_origin = p;
                }
            }
        }
        if resp.dragged() {
            if let (Some(m), Some(pos)) = (self.dm, resp.interact_pointer_pos()) {
                let dx = (pos.x - self.drag_origin.x) / disp.width();
                let dy = (pos.y - self.drag_origin.y) / disp.height();
                let d0 = self.d0;
                let (mut x0, mut x1, mut y0, mut y1) = (d0.min.x, d0.max.x, d0.min.y, d0.max.y);
                if m.mv {
                    x0 = (d0.min.x + dx).max(0.0).min(1.0 - d0.width());
                    y0 = (d0.min.y + dy).max(0.0).min(1.0 - d0.height());
                    x1 = x0 + d0.width();
                    y1 = y0 + d0.height();
                } else {
                    if m.l {
                        x0 += dx;
                    }
                    if m.r {
                        x1 += dx;
                    }
                    if m.t {
                        y0 += dy;
                    }
                    if m.b {
                        y1 += dy;
                    }
                    x0 = x0.max(0.0).min(x1 - 0.03);
                    x1 = x1.min(1.0).max(x0 + 0.03);
                    y0 = y0.max(0.0).min(y1 - 0.03);
                    y1 = y1.min(1.0).max(y0 + 0.03);
                }
                self.cn = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
            }
        }
        if resp.drag_stopped() {
            self.dm = None;
        }
    }

    fn pen_interact(&mut self, resp: &egui::Response, disp: Rect) {
        if resp.drag_started() {
            self.stroke.clear();
        }
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                if self.stroke.last() != Some(&p) {
                    self.stroke.push(p);
                }
            }
        }
        if resp.drag_stopped() {
            self.commit_stroke(disp);
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.stroke = vec![p];
                self.commit_stroke(disp);
            }
        }
    }

    /// Biten çizgiyi resme işler.
    fn commit_stroke(&mut self, disp: Rect) {
        let pts = std::mem::take(&mut self.stroke);
        let (w, h) = match &self.cur {
            Some(c) => (c.width() as f32, c.height() as f32),
            None => return,
        };
        if pts.is_empty() || disp.width() < 1.0 || disp.height() < 1.0 {
            return;
        }
        let ip: Vec<(f32, f32)> = pts
            .iter()
            .map(|p| ((p.x - disp.min.x) / disp.width() * w, (p.y - disp.min.y) / disp.height() * h))
            .collect();
        let width = 4.0 * w / disp.width();
        self.push_undo();
        if let Some(a) = self.cur.as_mut() {
            gallery::draw_stroke(Arc::make_mut(a), &ip, width);
        }
        self.tex_dirty = true;
    }

    fn draw_crop(&self, painter: &egui::Painter, rect: Rect, disp: Rect) {
        let c = self.crop_screen(disp);
        let dim = Color32::from_black_alpha(140);
        painter.rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, c.min.y)), 0.0, dim);
        painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, c.max.y), rect.max), 0.0, dim);
        painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, c.min.y), pos2(c.min.x, c.max.y)), 0.0, dim);
        painter.rect_filled(Rect::from_min_max(pos2(c.max.x, c.min.y), pos2(rect.max.x, c.max.y)), 0.0, dim);
        painter.rect_stroke(c, 0.0, Stroke::new(1.5, Color32::WHITE));
        let pts = [
            pos2(c.min.x, c.min.y),
            pos2(c.center().x, c.min.y),
            pos2(c.max.x, c.min.y),
            pos2(c.min.x, c.center().y),
            pos2(c.max.x, c.center().y),
            pos2(c.min.x, c.max.y),
            pos2(c.center().x, c.max.y),
            pos2(c.max.x, c.max.y),
        ];
        for p in pts {
            painter.rect_filled(Rect::from_center_size(p, vec2(10.0, 10.0)), 0.0, Color32::WHITE);
        }
        pill(painter, pos2(rect.center().x, rect.min.y + 12.0), self.lang.t("Kaydet düğmesi: kopya olarak kaydet  •  Esc: iptal", "Save button: save as copy  •  Esc: cancel"));
    }

    /// Video alanı: kare + altta denetim çubuğu (oynat/duraklat, süre, ilerleme çubuğu).
    /// Oynat düğmesine basılı tutunca `FF_RATE`× ileri sarar, bırakınca eski haline döner.
    fn canvas_video(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        let lg = self.lang;
        let avail = ui.available_size();
        let (resp, painter) = ui.allocate_painter(avail, Sense::hover());
        let rect = resp.rect;
        let video_rect = Rect::from_min_max(rect.min, pos2(rect.max.x, (rect.max.y - CTRL_H).max(rect.min.y)));
        let ctrl_rect = Rect::from_min_max(pos2(rect.min.x, video_rect.max.y), rect.max);

        // Yeni kareyi dokuya aktar
        if let Some(v) = self.video.as_mut() {
            if let Some(ci) = v.poll_frame() {
                match &mut self.video_tex {
                    Some(t) => t.set(ci, TextureOptions::LINEAR),
                    None => self.video_tex = Some(ctx.load_texture("video", ci, TextureOptions::LINEAR)),
                }
            }
            if v.failed() {
                self.failed = true;
            }
        }

        // Kareyi çiz
        if let Some(t) = &self.video_tex {
            let sz = t.size_vec2();
            let disp = fit_rect(video_rect, sz.x, sz.y);
            painter.image(t.id(), disp, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            let text = if self.failed || self.video.is_none() {
                lg.t("Video açılamadı", "Could not open video")
            } else {
                lg.t("Yükleniyor…", "Loading…")
            };
            painter.text(video_rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(20.0), Color32::GRAY);
        }

        // Denetim çubuğu
        painter.rect_filled(ctrl_rect, 0.0, Color32::from_rgb(28, 31, 33));
        painter.hline(ctrl_rect.x_range(), ctrl_rect.top(), Stroke::new(1.0, Color32::from_gray(52)));

        let (cur, dur, playing) = match &self.video {
            Some(v) => (v.current(), v.duration(), v.is_playing()),
            None => (0.0, 0.0, false),
        };
        let shown = self.scrub.unwrap_or(cur).min(dur.max(0.0));
        let ff = self.ff;
        let mut seek_to: Option<f64> = None;
        let mut drag_stopped = false;
        let mut btn_down = false;
        let mut btn_clicked = false;

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(ctrl_rect), |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(16.0);

                // Oynat / duraklat düğmesi (simgeler elle çizilir)
                let (brect, bresp) = ui.allocate_exact_size(vec2(44.0, 34.0), Sense::click());
                let bresp = bresp.on_hover_text(lg.t(
                    "Oynat / duraklat (Boşluk) — basılı tut: 2.5× ileri sar",
                    "Play / pause (Space) — hold: 2.5× fast-forward",
                ));
                let bg = if ff {
                    BLUE
                } else if bresp.hovered() {
                    Color32::from_gray(64)
                } else {
                    Color32::from_gray(48)
                };
                ui.painter().rect_filled(brect, 8.0, bg);
                let c = brect.center();
                if playing {
                    for dx in [-5.0f32, 5.0] {
                        ui.painter().rect_filled(Rect::from_center_size(pos2(c.x + dx, c.y), vec2(5.0, 14.0)), 1.0, Color32::WHITE);
                    }
                } else {
                    ui.painter().add(Shape::convex_polygon(
                        vec![pos2(c.x - 5.0, c.y - 8.0), pos2(c.x - 5.0, c.y + 8.0), pos2(c.x + 8.0, c.y)],
                        Color32::WHITE,
                        Stroke::NONE,
                    ));
                }
                btn_down = bresp.is_pointer_button_down_on();
                btn_clicked = bresp.clicked();

                ui.add_space(10.0);
                ui.label(RichText::new(format!("{} / {}", video::fmt_time(shown), video::fmt_time(dur))).monospace());
                if ff {
                    ui.label(RichText::new(format!("{FF_RATE}×")).strong().color(BLUE));
                }

                ui.add_space(10.0);
                ui.spacing_mut().slider_width = (ui.available_width() - 24.0).max(80.0);
                let mut pos = shown;
                let r = ui.add_enabled(
                    dur > 0.0,
                    egui::Slider::new(&mut pos, 0.0..=dur.max(0.01)).show_value(false),
                );
                if r.changed() {
                    seek_to = Some(pos);
                }
                if r.drag_stopped() {
                    drag_stopped = true;
                }
            });
        });

        // İlerleme çubuğu
        if let Some(s) = seek_to {
            self.scrub = Some(s);
            if let Some(v) = &mut self.video {
                v.seek(s);
            }
        }
        if drag_stopped {
            self.scrub = None;
        }

        // Basılı tutma: FF_RATE× ileri sar; kısa tıklama: oynat / duraklat
        if btn_down {
            let t0 = *self.ff_hold.get_or_insert_with(Instant::now);
            if !self.ff && t0.elapsed().as_millis() >= FF_HOLD_MS {
                self.ff = true;
                self.ff_resume = playing;
                if let Some(v) = &self.video {
                    v.set_rate(FF_RATE);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        } else {
            if self.ff {
                self.ff = false;
                self.ff_consumed = true;
                if let Some(v) = &self.video {
                    if self.ff_resume {
                        v.set_rate(1.0);
                    } else {
                        v.pause();
                    }
                }
            }
            self.ff_hold = None;
        }
        if btn_clicked {
            if self.ff_consumed {
                self.ff_consumed = false;
            } else {
                self.toggle_video();
            }
        }

        if self.video.as_ref().map_or(false, |v| v.wants_repaint()) {
            ctx.request_repaint();
        }

        // Bildirim
        let expired = self.toast.as_ref().map_or(false, |(_, at)| at.elapsed() >= Duration::from_millis(TOAST_MS));
        if expired {
            self.toast = None;
        }
        if let Some((text, _)) = &self.toast {
            pill(&painter, pos2(video_rect.center().x, video_rect.max.y - 40.0), text);
        }
    }

    fn canvas(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        if self.is_video_cur() {
            self.canvas_video(ctx, ui);
            return;
        }
        if self.tex_dirty {
            self.rebuild_tex(ctx);
        }
        let avail = ui.available_size();
        let (resp, painter) = ui.allocate_painter(avail, Sense::click_and_drag());
        let rect = resp.rect;

        let (Some(tex), Some(cur)) = (self.tex.clone(), self.cur.clone()) else {
            let text = if self.failed { self.lang.t("Resim açılamadı", "Could not open image") } else { self.lang.t("Yükleniyor…", "Loading…") };
            painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(20.0), Color32::GRAY);
            return;
        };

        let (iw, ih) = (cur.width() as f32, cur.height() as f32);
        let disp = fit_rect(rect, iw, ih);

        // Dokunmatik yüzey sıkıştırma / Ctrl+kaydırma ile de büyütme
        if resp.hovered() && !self.crop && !self.pen {
            let zd = ctx.input(|i| i.zoom_delta());
            if (zd - 1.0).abs() > 1e-4 {
                self.zoom_v = (self.zoom_v + zd.ln() / ZOOM_BASE.ln()).clamp(-1.0, 1.0);
            }
        }
        let zoom = ZOOM_BASE.powf(self.zoom_v);
        if zoom <= 1.001 {
            self.pan = Vec2::ZERO;
        }

        // Resmi çiz (büyütme + serbest açı)
        let center = disp.center() + self.pan;
        let img_rect = Rect::from_center_size(center, disp.size() * zoom);
        let mut mesh = egui::Mesh::with_texture(tex.id());
        mesh.add_rect_with_uv(img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        if self.rot_v != 0.0 {
            // Pozitif açı: saat yönü (ekranda y aşağı olduğu için)
            let (sn, cs) = self.rot_v.to_radians().sin_cos();
            for v in mesh.vertices.iter_mut() {
                let d = v.pos - center;
                v.pos = center + vec2(cs * d.x - sn * d.y, sn * d.x + cs * d.y);
            }
        }
        painter.add(Shape::mesh(mesh));

        // Etkileşim
        if self.crop {
            self.crop_interact(ctx, &resp, disp);
            self.draw_crop(&painter, rect, disp);
        } else if self.pen {
            self.pen_interact(&resp, disp);
            if !self.stroke.is_empty() {
                if self.stroke.len() == 1 {
                    painter.circle_filled(self.stroke[0], 2.0, Color32::RED);
                } else {
                    painter.add(Shape::line(self.stroke.clone(), Stroke::new(4.0, Color32::RED)));
                }
            }
        } else if zoom > 1.001 && resp.dragged() {
            self.pan += resp.drag_delta();
        }

        // Bildirim
        let expired = self.toast.as_ref().map_or(false, |(_, at)| at.elapsed() >= Duration::from_millis(TOAST_MS));
        if expired {
            self.toast = None;
        }
        if let Some((text, _)) = &self.toast {
            pill(&painter, pos2(rect.center().x, rect.max.y - 40.0), text);
        }
    }
}

impl eframe::App for DeckApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.workers.begin_frame();
        self.pump(ctx);
        // İlk açılışta (yalnızca bir kez) varsayılan resim açıcı olmayı sor
        static ASKED: std::sync::Once = std::sync::Once::new();
        ASKED.call_once(|| crate::macos_open::offer_default(self.lang == Lang::Tr));
        self.handle_drop(ctx);
        self.handle_keys(ctx);
        self.tick_slideshow(ctx);

        match self.screen {
            Screen::Browse => {
                egui::TopBottomPanel::top("gezgin")
                    .exact_height(TOP_BAR_H)
                    .frame(bar_frame())
                    .show(ctx, |ui| self.browser_bar(ui));
                if self.scanning {
                    ctx.request_repaint_after(Duration::from_millis(150));
                }
                let expired = self.toast.as_ref().map_or(false, |(_, at)| at.elapsed() >= Duration::from_millis(TOAST_MS));
                if expired {
                    self.toast = None;
                }
                if let Some((text, _)) = &self.toast {
                    let r = ctx.screen_rect();
                    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, egui::Id::new("bildirim_gezgin")));
                    pill(&painter, pos2(r.center().x, r.max.y - 40.0), text);
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(14.0, 10.0)))
                    .show(ctx, |ui| {
                        if self.scanning || !self.scan_list.is_empty() {
                            self.scan_header(ui);
                            ui.add_space(8.0);
                        }
                        if self.view_scan {
                            self.scan_view(ui);
                        } else {
                            self.browser_grid(ui);
                        }
                    });
            }
            Screen::Viewer => {
                egui::TopBottomPanel::top("araclar")
                    .exact_height(TOP_BAR_H)
                    .frame(bar_frame())
                    .show(ctx, |ui| self.toolbar(ctx, ui));
                egui::TopBottomPanel::bottom("serit")
                    .exact_height(110.0)
                    .frame(egui::Frame::none().fill(Color32::from_rgb(28, 31, 33)))
                    .show(ctx, |ui| self.bottom_ui(ui));
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(BG))
                    .show(ctx, |ui| self.canvas(ctx, ui));
            }
        }
    }
}

// ---------- Yardımcılar ----------

/// Türkçe harfleri (ğ ş ı İ ç ö ü) eksiksiz içeren bir sistem yazı tipini öne alır.
fn load_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("sistem-yazi".to_owned(), egui::FontData::from_owned(bytes));
            fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "sistem-yazi".to_owned());
            fonts.families.entry(egui::FontFamily::Monospace).or_default().push("sistem-yazi".to_owned());
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn fit_rect(r: Rect, w: f32, h: f32) -> Rect {
    let k = (r.width() / w).min(r.height() / h);
    Rect::from_center_size(r.center(), vec2(w * k, h * k))
}

/// Üst çubukların ortak çerçevesi
fn bar_frame() -> egui::Frame {
    egui::Frame::none().fill(Color32::from_rgb(28, 31, 33)).inner_margin(egui::Margin::symmetric(10.0, 6.0))
}

/// Kartın altına tek satır, ortalı, sığmazsa "…" ile kısaltılan ad yazar
fn label_under(painter: &egui::Painter, tile: Rect, text: String, width: f32) {
    let mut job = egui::text::LayoutJob::simple(text, FontId::proportional(12.5), Color32::from_gray(205), width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let g = painter.layout_job(job);
    painter.galley(pos2(tile.center().x - g.size().x / 2.0, tile.max.y + 4.0), g, Color32::WHITE);
}

/// macOS tarzı mavi klasör simgesi
fn draw_folder(painter: &egui::Painter, tile: Rect) {
    let w = tile.width() * 0.56;
    let h = w * 0.74;
    let body = Rect::from_center_size(tile.center() + vec2(0.0, h * 0.08), vec2(w, h));
    let tab = Rect::from_min_size(body.min - vec2(0.0, h * 0.15), vec2(w * 0.42, h * 0.3));
    painter.rect_filled(tab, 3.0, Color32::from_rgb(60, 130, 205));
    painter.rect_filled(body, 5.0, Color32::from_rgb(91, 172, 242));
}

fn tb(ui: &mut egui::Ui, label: &str, tip: &str, on: bool, color: Option<Color32>) -> bool {
    let mut t = RichText::new(label).size(15.0);
    if let Some(c) = color {
        t = t.color(c);
    }
    ui.add(egui::Button::new(t).selected(on).min_size(vec2(0.0, BTN_H))).on_hover_text(tip).clicked()
}

fn reset_label(ui: &mut egui::Ui, label: &str, tip: &str) -> bool {
    let t = RichText::new(label).small().monospace();
    ui.add_sized(vec2(48.0, 18.0), egui::Button::new(t).frame(false)).on_hover_text(tip).clicked()
}

/// Koyu yarı saydam zeminli kısa yazı
fn pill(painter: &egui::Painter, top_center: Pos2, text: &str) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(13.0), Color32::WHITE);
    let r = Align2::CENTER_TOP.anchor_size(top_center, galley.size());
    painter.rect_filled(r.expand(6.0), 6.0, Color32::from_black_alpha(170));
    painter.galley(r.min, galley, Color32::WHITE);
}

/// Video küçük resminin köşesine oynat işareti
fn play_badge(painter: &egui::Painter, tile: Rect) {
    let c = tile.left_bottom() + vec2(14.0, -14.0);
    painter.circle_filled(c, 11.0, Color32::from_black_alpha(170));
    painter.add(Shape::convex_polygon(
        vec![pos2(c.x - 3.0, c.y - 5.0), pos2(c.x - 3.0, c.y + 5.0), pos2(c.x + 5.0, c.y)],
        Color32::WHITE,
        Stroke::NONE,
    ));
}
