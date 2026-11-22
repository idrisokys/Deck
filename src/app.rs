//! Deck arayüzü (egui): araç çubuğu, resim alanı, kırpma, kırmızı kalem, alt şerit ve kaydırıcılar.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, Context, FontId, Key, Modifiers, Pos2, Rect, RichText, Sense,
    Shape, Stroke, TextureHandle, TextureOptions, Vec2,
};
use image::{imageops, RgbaImage};

use crate::gallery::{self, Cache, Msg, Workers};

const BG: Color32 = Color32::from_rgb(23, 26, 28);
const BLUE: Color32 = Color32::from_rgb(10, 132, 255);
const PEN_RED: Color32 = Color32::from_rgb(255, 80, 80);
const SLIDE_SECS: u64 = 3;
const TOAST_MS: u64 = 2500;
const MAX_UNDO: usize = 10;

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
    zoom_v: f32, // -1..1 → 0,25x .. 4x (ortası 1x)
    rot_v: f32,  // -180..180 derece
    pan: Vec2,

    play: bool,
    fullscreen: bool,
    last_slide: Instant,
    toast: Option<(String, Instant)>,
}

impl DeckApp {
    pub fn new(cc: &eframe::CreationContext<'_>, start: Option<PathBuf>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        ctx.set_visuals(v);

        let (tx, rx) = channel();
        let workers = Workers::start(&ctx, tx);

        let mut app = DeckApp {
            workers,
            rx,
            files: Vec::new(),
            idx: 0,
            cur: None,
            cur_path: None,
            tex: None,
            tex_dirty: false,
            failed: false,
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
        };
        if let Some(p) = start {
            app.open(p);
        }
        app
    }

    // ---------- Yükleme ----------

    /// Resim hemen açılır; klasördeki diğer resimler arka planda listelenir.
    fn open(&mut self, path: PathBuf) {
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
        if let Some(p) = rfd::FileDialog::new().add_filter("Resimler", gallery::EXTS).pick_file() {
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
        if self.cache.get(&p).is_some() || self.pending.contains(&p) {
            return;
        }
        self.pending.insert(p.clone());
        let _ = self.workers.full_tx.send(p);
    }

    /// Geçerli resmi yükler; yüklenirken eski resim görünür kalır.
    fn load_current(&mut self) {
        let Some(p) = self.files.get(self.idx).cloned() else {
            self.cur = None;
            self.cur_path = None;
            self.tex = None;
            self.failed = false;
            return;
        };
        if let Some(img) = self.cache.get(&p) {
            self.set_cur(p, img);
            self.prefetch();
        } else {
            self.failed = false;
            self.request_full(p);
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
                }
                Msg::Siblings(list) => {
                    let keep = self.files.get(self.idx).cloned();
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
            self.toast("Çöp Kutusu'na atılamadı");
            return;
        }
        self.cache.remove(&p);
        self.thumbs.remove(&p);
        self.thumb_req.remove(&p);
        self.files.remove(self.idx);
        self.reset_edit();
        if self.files.is_empty() {
            self.idx = 0;
            self.cur = None;
            self.cur_path = None;
            self.tex = None;
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
        match gallery::save_copy(&path, part) {
            Ok(dest) => {
                self.crop = false;
                self.cn = def_crop();
                let name = dest.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                self.toast(format!("Kaydedildi: {name}"));
                self.workers.list(path);
            }
            Err(e) => self.toast(format!("Kaydedilemedi: {e}")),
        }
    }

    fn toggle_fullscreen(&mut self, ctx: &Context) {
        self.fullscreen = !self.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
    }

    // ---------- Girdiler ----------

    fn handle_drop(&mut self, ctx: &Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(p) = dropped.into_iter().filter_map(|f| f.path).find(|p| p.is_file()) {
            self.open(p);
        }
    }

    fn handle_keys(&mut self, ctx: &Context) {
        let (left, right, up, down, esc, z, o, s) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::COMMAND, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::S),
            )
        });
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
            }
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
        if !self.play || self.files.len() < 2 {
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

    fn toolbar(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if tb(ui, "Önceki", "Önceki resim (←)", false, None) {
                self.go(-1);
            }
            if tb(ui, "Sonraki", "Sonraki resim (→)", false, None) {
                self.go(1);
            }
            ui.separator();
            if tb(ui, "Sola", "Sola döndür (↑)", false, None) {
                self.rot90(false);
            }
            if tb(ui, "Sağa", "Sağa döndür (↓)", false, None) {
                self.rot90(true);
            }
            if tb(ui, "Yatay", "Yatay çevir", false, None) {
                self.flip(true);
            }
            if tb(ui, "Dikey", "Dikey çevir", false, None) {
                self.flip(false);
            }
            if tb(ui, "Kırp", "Kırp", self.crop, None) {
                self.crop = !self.crop;
                self.cn = def_crop();
                self.dm = None;
                if self.crop {
                    self.pen = false;
                    self.stroke.clear();
                    self.reset_view();
                }
            }
            if tb(ui, "Kalem", "Kırmızı kalem", self.pen, Some(PEN_RED)) {
                self.pen = !self.pen;
                self.stroke.clear();
                if self.pen {
                    self.crop = false;
                    self.reset_view();
                }
            }
            ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
                if tb(ui, "Geri al", "Geri al (⌘Z)", false, None) {
                    self.undo_last();
                }
            });
            if tb(ui, "Tam ekran", "Tam ekran (Esc ile çık)", self.fullscreen, None) {
                self.toggle_fullscreen(ctx);
            }
            if tb(ui, "Slayt", "Slayt gösterisi (3 sn)", self.play, None) {
                self.play = !self.play;
                self.last_slide = Instant::now();
                if self.play != self.fullscreen {
                    self.toggle_fullscreen(ctx);
                }
            }
            ui.separator();
            if tb(ui, "Kaydet", "Kopya olarak kaydet (⌘S)", false, None) {
                self.save();
            }
            if tb(ui, "Çöp", "Çöp Kutusu'na at", false, None) {
                self.del();
            }
            if tb(ui, "Finder", "Finder'da göster", false, None) {
                if let Some(p) = self.files.get(self.idx) {
                    let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
                }
            }
            if tb(ui, "Aç…", "Resim aç (⌘O)", false, None) {
                self.open_dialog();
            }
        });
        ui.add_space(4.0);
    }

    // ---------- Alt şerit ----------

    fn bottom_ui(&mut self, ui: &mut egui::Ui) {
        let mut click: Option<usize> = None;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 330.0).max(100.0);
            ui.allocate_ui(vec2(w, 96.0), |ui| {
                egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 14.0;
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
                                    if self.thumb_req.insert(p.clone()) {
                                        let _ = self.workers.thumb_tx.send(p);
                                    }
                                }
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
            });
            ui.separator();
            self.slider_ui(ui);
        });
        if let Some(n) = click {
            self.show_idx(n);
        }
    }

    /// Çizgi üzerinde küçük yuvarlak: sağa çek → büyüt / sağa döndür, sola çek → küçült / sola döndür.
    fn slider_ui(&mut self, ui: &mut egui::Ui) {
        let enabled = !(self.crop || self.pen);
        ui.vertical(|ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.spacing_mut().slider_width = 150.0;

                ui.horizontal(|ui| {
                    let label = format!("%{}", (4f32.powf(self.zoom_v) * 100.0).round() as i32);
                    if reset_label(ui, &label, "Sıfırla (%100)") {
                        self.zoom_v = 0.0;
                    }
                    ui.label(RichText::new("küçült").small().weak());
                    let r = ui.add(egui::Slider::new(&mut self.zoom_v, -1.0..=1.0).show_value(false));
                    if r.changed() && self.zoom_v.abs() < 0.04 {
                        self.zoom_v = 0.0;
                    }
                    ui.label(RichText::new("büyüt").small().weak());
                });

                ui.horizontal(|ui| {
                    let label = format!("{}°", self.rot_v.round() as i32);
                    if reset_label(ui, &label, "Sıfırla (0°)") {
                        self.rot_v = 0.0;
                    }
                    ui.label(RichText::new("sola").small().weak());
                    let r = ui.add(egui::Slider::new(&mut self.rot_v, -180.0..=180.0).show_value(false));
                    if r.changed() && self.rot_v.abs() < 3.0 {
                        self.rot_v = 0.0;
                    }
                    ui.label(RichText::new("sağa").small().weak());
                });
            });
        });
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
        pill(painter, pos2(rect.center().x, rect.min.y + 12.0), "Kaydet düğmesi: kopya olarak kaydet  •  Esc: iptal");
    }

    fn canvas(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        if self.tex_dirty {
            self.rebuild_tex(ctx);
        }
        let avail = ui.available_size();
        let (resp, painter) = ui.allocate_painter(avail, Sense::click_and_drag());
        let rect = resp.rect;

        let (Some(tex), Some(cur)) = (self.tex.clone(), self.cur.clone()) else {
            let text = if self.failed {
                "Resim açılamadı"
            } else if self.files.is_empty() {
                "Bir resim açın"
            } else {
                "Yükleniyor…"
            };
            painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(20.0), Color32::GRAY);
            if self.files.is_empty() {
                painter.text(
                    rect.center() + vec2(0.0, 28.0),
                    Align2::CENTER_CENTER,
                    "ya da resmi pencereye sürükleyin",
                    FontId::proportional(14.0),
                    Color32::DARK_GRAY,
                );
                let br = Rect::from_center_size(rect.center() + vec2(0.0, 72.0), vec2(120.0, 32.0));
                if ui.put(br, egui::Button::new("Resim Aç…")).clicked() {
                    self.open_dialog();
                }
            }
            return;
        };

        let (iw, ih) = (cur.width() as f32, cur.height() as f32);
        let disp = fit_rect(rect, iw, ih);

        // Dokunmatik yüzey sıkıştırma / Ctrl+kaydırma ile de büyütme
        if resp.hovered() && !self.crop && !self.pen {
            let zd = ctx.input(|i| i.zoom_delta());
            if (zd - 1.0).abs() > 1e-4 {
                self.zoom_v = (self.zoom_v + zd.ln() / 4f32.ln()).clamp(-1.0, 1.0);
            }
        }
        let zoom = 4f32.powf(self.zoom_v);
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
        self.pump(ctx);
        self.handle_drop(ctx);
        self.handle_keys(ctx);
        self.tick_slideshow(ctx);

        egui::TopBottomPanel::top("araclar").show(ctx, |ui| self.toolbar(ctx, ui));
        egui::TopBottomPanel::bottom("serit").exact_height(114.0).show(ctx, |ui| self.bottom_ui(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| self.canvas(ctx, ui));
    }
}

// ---------- Yardımcılar ----------

fn fit_rect(r: Rect, w: f32, h: f32) -> Rect {
    let k = (r.width() / w).min(r.height() / h);
    Rect::from_center_size(r.center(), vec2(w * k, h * k))
}

fn tb(ui: &mut egui::Ui, label: &str, tip: &str, on: bool, color: Option<Color32>) -> bool {
    let mut t = RichText::new(label).size(15.0);
    if let Some(c) = color {
        t = t.color(c);
    }
    ui.add(egui::Button::new(t).selected(on).min_size(vec2(0.0, 30.0))).on_hover_text(tip).clicked()
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
