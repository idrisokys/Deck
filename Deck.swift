// Deck — macOS 15 Sequoia+ (Apple Silicon) için koyu temalı, tamamen Türkçe resim galerisi.
// ImageIO'nun çözebildiği tüm resim biçimlerini (JPEG, PNG, HEIC, AVIF, WebP, TIFF, GIF, BMP,
// RAW, PSD, ICO, JPEG 2000, SVG ...) açar.

import SwiftUI
import AppKit
import UniformTypeIdentifiers
import Observation
import ImageIO
import CoreImage

// MARK: - Görüntüleyici modeli

/// Açık olan resmi tutar. Finder'dan ("Birlikte Aç" / varsayılan uygulama), ⌘O ile ya da
/// araç çubuğundaki klasör düğmesiyle gelen istekler buradan geçer.
@Observable
final class ViewerModel {
    static let shared = ViewerModel()

    var start: URL?
    /// Her yeni açılışta artar; görünümün baştan kurulmasını sağlar
    var token = 0

    func open(_ u: URL) {
        start = u
        token += 1
    }

    func pick() {
        let p = NSOpenPanel()
        p.allowedContentTypes = [.image]
        guard p.runModal() == .OK, let u = p.url else { return }
        open(u)
    }
}

// MARK: - Resim yükleme

enum Gallery {
    /// ImageIO'nun tanıdığı tüm türlerin dosya uzantıları (+ SVG ve yaygın RAW/yeni biçimler).
    /// Sistem yeni bir biçim eklerse liste kendiliğinden genişler.
    static let exts: Set<String> = {
        var types = (CGImageSourceCopyTypeIdentifiers() as? [String] ?? []).compactMap { UTType($0) }
        types.append(.svg)
        var s: Set<String> = ["dng", "cr2", "cr3", "nef", "arw", "orf", "raf", "rw2", "heif", "avif",
                              "jxl", "psd", "ico", "icns", "jp2", "svg", "tif"]
        for t in types { for e in t.tags[.filenameExtension] ?? [] { s.insert(e.lowercased()) } }
        return s
    }()

    static func isImage(_ u: URL) -> Bool { isImage(ext: u.pathExtension.lowercased()) }

    private static func isImage(ext: String) -> Bool {
        if ext.isEmpty { return false }
        if exts.contains(ext) { return true }
        // Listede olmayan ama sistemin resim olarak bildiği uzantılar
        return UTType(filenameExtension: ext)?.conforms(to: .image) ?? false
    }

    /// Çözülmüş resimler: gezinirken komşu resimler hazır beklesin, bellek bayt sınırıyla korunur.
    static let cache: NSCache<NSURL, NSImage> = {
        let c = NSCache<NSURL, NSImage>()
        c.totalCostLimit = 768 << 20
        return c
    }()

    static func cached(_ u: URL) -> NSImage? { cache.object(forKey: u as NSURL) }

    /// Herhangi bir iş parçacığından çağrılabilir.
    static func load(_ u: URL) -> NSImage? {
        if let c = cached(u) { return c }
        guard let m = decode(u) else { return nil }
        cache.setObject(m, forKey: u as NSURL, cost: Int(m.size.width * m.size.height) * 4)
        return m
    }

    static func loadAsync(_ u: URL) async -> NSImage? {
        await Task.detached(priority: .userInitiated) { load(u) }.value
    }

    /// ImageIO ile piksel boyutunda çözer: EXIF yönü uygulanır, çözme hemen (çağıran iş
    /// parçacığında) yapılır; böylece ekrana çizerken ana iş parçacığında takılma olmaz.
    private static func decode(_ u: URL) -> NSImage? {
        if let src = CGImageSourceCreateWithURL(u as CFURL, nil),
           let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any],
           let w = props[kCGImagePropertyPixelWidth] as? Int,
           let h = props[kCGImagePropertyPixelHeight] as? Int, max(w, h) > 0 {
            let opts: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceShouldCacheImmediately: true,
                kCGImageSourceThumbnailMaxPixelSize: max(w, h)
            ]
            if let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) {
                return NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
            }
        }
        return NSImage(contentsOf: u)     // SVG gibi ImageIO'nun açmadıkları
    }

    /// Şerit için küçük önizleme. Dosyada gömülü küçük resim varsa onu kullanır (RAW/HEIC'te çok hızlı).
    static func thumbnail(_ u: URL, maxPixel: Int = 200) -> NSImage? {
        if let src = CGImageSourceCreateWithURL(u as CFURL, nil) {
            let opts: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageIfAbsent: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceShouldCacheImmediately: true,
                kCGImageSourceThumbnailMaxPixelSize: maxPixel
            ]
            if let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) {
                return NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
            }
        }
        return NSImage(contentsOf: u)
    }

    /// Aynı klasördeki resimler (ada göre, sayıya duyarlı sıralı). `u` her zaman listededir.
    static func siblings(of u: URL) -> [URL] {
        let dir = u.deletingLastPathComponent()
        let all = (try? FileManager.default.contentsOfDirectory(
            at: dir, includingPropertiesForKeys: nil, options: [.skipsHiddenFiles])) ?? []
        var memo: [String: Bool] = [:]
        let me = u.standardizedFileURL.path
        var named: [(name: String, url: URL)] = []
        for f in all {
            let e = f.pathExtension.lowercased()
            let ok: Bool
            if let v = memo[e] { ok = v } else { ok = isImage(ext: e); memo[e] = ok }
            if ok || f.standardizedFileURL.path == me { named.append((f.lastPathComponent, f)) }
        }
        named.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        let list = named.map { $0.url }
        return list.isEmpty ? [u] : list
    }

    static func siblingsAsync(of u: URL) async -> [URL] {
        await Task.detached(priority: .userInitiated) { siblings(of: u) }.value
    }

    /// Dosyayı onay sormadan Çöp Kutusu'na atar.
    static func trash(_ u: URL) -> Bool {
        (try? FileManager.default.trashItem(at: u, resultingItemURL: nil)) != nil
    }

    /// Döndür/çevir sonucunu hemen bir bitmap'e çizer. (Çizim bloklu NSImage her yeniden
    /// çizimde — örn. kırpma sürüklenirken — tam çözünürlüklü resmi baştan çizerdi.)
    static func xform(_ m: NSImage, _ size: NSSize, _ t: @escaping (NSAffineTransform) -> Void) -> NSImage {
        if let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size.width), pixelsHigh: Int(size.height),
                                      bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                      colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
           let ctx = NSGraphicsContext(bitmapImageRep: rep) {
            rep.size = size
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = ctx
            let a = NSAffineTransform(); t(a); a.concat()
            m.draw(at: .zero, from: .zero, operation: .copy, fraction: 1)
            NSGraphicsContext.restoreGraphicsState()
            let out = NSImage(size: size)
            out.addRepresentation(rep)
            return out
        }
        return NSImage(size: size, flipped: false) { _ in
            let a = NSAffineTransform(); t(a); a.concat()
            m.draw(at: .zero, from: .zero, operation: .copy, fraction: 1); return true
        }
    }

    /// Çizgiyi resmin üstüne piksel olarak işler. `pts`: resme göre 0...1 (sol üst başlangıçlı), `width`: piksel.
    static func paint(_ m: NSImage, _ pts: [CGPoint], _ color: NSColor, _ width: CGFloat) -> NSImage {
        let size = m.size
        guard !pts.isEmpty,
              let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size.width), pixelsHigh: Int(size.height),
                                         bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                         colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
              let ctx = NSGraphicsContext(bitmapImageRep: rep) else { return m }
        rep.size = size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = ctx
        m.draw(at: .zero, from: .zero, operation: .copy, fraction: 1)
        let pp = pts.map { CGPoint(x: $0.x * size.width, y: (1 - $0.y) * size.height) }
        color.setStroke(); color.setFill()
        if pp.count == 1 {
            NSBezierPath(ovalIn: CGRect(x: pp[0].x - width / 2, y: pp[0].y - width / 2, width: width, height: width)).fill()
        } else {
            let path = NSBezierPath()
            path.lineWidth = width; path.lineCapStyle = .round; path.lineJoinStyle = .round
            path.move(to: pp[0])
            for q in pp.dropFirst() { path.line(to: q) }
            path.stroke()
        }
        NSGraphicsContext.restoreGraphicsState()
        let out = NSImage(size: size)
        out.addRepresentation(rep)
        return out
    }
}

// MARK: - Filtreler

/// Temel düzeltme ayarları. Değerler varsayılandaysa resme hiçbir şey uygulanmaz.
struct Filters: Equatable {
    var exposure = 0.0       // pozlama (EV)         -2...2
    var brightness = 0.0     // parlaklık            -0.5...0.5
    var contrast = 1.0       // karşıtlık            0.5...1.5
    var saturation = 1.0     // doygunluk            0...2
    var warmth = 0.0         // sıcaklık (soğuk/sıcak) -1...1
    var sharpness = 0.0      // keskinlik            0...2
    var tone = 0             // 0 renkli, 1 siyah-beyaz, 2 sepya

    var isIdentity: Bool { self == Filters() }
}

enum Filtering {
    static let ctx = CIContext()

    /// `maxSide` verilirse önizleme için resim küçültülerek işlenir (kaydederken tam boyutta işlenir).
    static func apply(_ f: Filters, to cg: CGImage, maxSide: CGFloat? = nil) -> CGImage? {
        var ci = CIImage(cgImage: cg)
        if let ms = maxSide {
            let big = max(ci.extent.width, ci.extent.height)
            if big > ms { let k = ms / big; ci = ci.transformed(by: CGAffineTransform(scaleX: k, y: k)) }
        }
        let rect = ci.extent
        func run(_ name: String, _ p: [String: Any]) {
            var q = p; q[kCIInputImageKey] = ci
            if let o = CIFilter(name: name, parameters: q)?.outputImage { ci = o }
        }
        if f.exposure != 0 { run("CIExposureAdjust", ["inputEV": f.exposure]) }
        if f.warmth != 0 {
            run("CITemperatureAndTint", ["inputNeutral": CIVector(x: 6500 + f.warmth * 2500, y: 0),
                                         "inputTargetNeutral": CIVector(x: 6500, y: 0)])
        }
        run("CIColorControls", ["inputBrightness": f.brightness, "inputContrast": f.contrast,
                                "inputSaturation": f.tone == 1 ? 0 : f.saturation])
        if f.sharpness > 0 { run("CISharpenLuminance", ["inputSharpness": f.sharpness]) }
        if f.tone == 2 { run("CISepiaTone", ["inputIntensity": 1.0]) }
        return ctx.createCGImage(ci, from: rect)
    }
}

// MARK: - Galeri görünümü

/// Geri alma için bir önceki durum
struct Snap { var img: NSImage?; var fl: Filters; var dirty: Bool }

struct CropMode { var l = false, r = false, t = false, b = false, mv = false }

struct GalleryView: View {
    static let defCn = CGRect(x: 0.1, y: 0.1, width: 0.8, height: 0.8)

    let start: URL
    @State private var files: [URL]
    @State private var i = 0
    @State private var img: NSImage?
    @State private var failed = false
    @State private var crop = false
    @State private var play = false
    @State private var fl = Filters()
    @State private var showFilters = false
    @State private var shown: NSImage?      // filtre uygulanmış önizleme (filtre yoksa nil)
    @State private var draw = false
    @State private var penColor = Color.red
    @State private var penWidth = 6.0
    @State private var live: [CGPoint] = []     // çizilmekte olan çizgi (ekran koordinatı)
    @State private var undo: [Snap] = []
    @State private var dirty = false            // döndürme/çevirme/çizim yapıldı, henüz kaydedilmedi
    @State private var zoomPct = 0              // -200...+200, 50'şer
    @State private var pan = CGSize.zero
    @State private var panBase = CGSize.zero
    static let zoomSteps = Array(stride(from: -200, through: 200, by: 50))
    static let penColors: [Color] = [.red, .yellow, .green, .blue, .white, .black]
    @State private var cn = GalleryView.defCn      // kırpma karesi (resme göre 0...1, sol üst başlangıçlı)
    @State private var d0 = GalleryView.defCn
    @State private var dm: CropMode?
    @State private var loadTask: Task<Void, Never>?
    @FocusState private var foc: Bool

    /// Resim hemen açılır; klasördeki diğer resimler arka planda listelenir.
    init(start: URL) {
        self.start = start
        _files = State(initialValue: [start])
    }

    // MARK: Yükleme

    func startUp() async {
        loadCurrent()
        await refreshFiles(keeping: start)
    }

    /// Klasör listesini arka planda yeniler; açık resim yerinde kalır.
    func refreshFiles(keeping u: URL) async {
        let all = await Gallery.siblingsAsync(of: u)
        files = all
        i = all.firstIndex { $0.standardizedFileURL.path == u.standardizedFileURL.path } ?? 0
        prefetch()
    }

    /// Geçerli resmi yükler; yenisi istenirse eskisi iptal olur, yüklenirken eski resim görünür kalır.
    func loadCurrent() {
        loadTask?.cancel()
        guard files.indices.contains(i) else { img = nil; failed = false; return }
        let u = files[i]
        if let c = Gallery.cached(u) { img = c; failed = false; prefetch(); return }
        loadTask = Task { @MainActor in
            let m = await Gallery.loadAsync(u)
            if Task.isCancelled { return }
            img = m
            failed = (m == nil)
            prefetch()
        }
    }

    /// Önceki ve sonraki resmi arka planda çözer; oklarla gezinmek anında olur.
    func prefetch() {
        guard files.count > 1 else { return }
        for d in [1, -1] {
            let u = files[(i + d + files.count) % files.count]
            if Gallery.cached(u) == nil {
                Task.detached(priority: .utility) { _ = Gallery.load(u) }
            }
        }
    }

    // MARK: Eylemler

    func show(_ n: Int) {
        guard files.indices.contains(n) else { return }
        crop = false; cn = Self.defCn; resetEdits()
        i = n; loadCurrent()
    }

    func go(_ d: Int) {
        guard !files.isEmpty else { return }
        show((i + d + files.count) % files.count)
    }

    func resetEdits() {
        fl = Filters(); undo = []; dirty = false; live = []
        zoomPct = 0; pan = .zero; panBase = .zero
    }

    func pushUndo() {
        undo.append(Snap(img: img, fl: fl, dirty: dirty))
        if undo.count > 15 { undo.removeFirst() }
    }

    func undoStep() {
        guard let s = undo.popLast() else { return }
        img = s.img; fl = s.fl; dirty = s.dirty
    }

    var canSave: Bool { dirty || crop || !fl.isIdentity }
    var zs: CGFloat { zoomPct >= 0 ? 1 + CGFloat(zoomPct) / 100 : 1 / (1 - CGFloat(zoomPct) / 100) }

    func zoomLabel(_ p: Int) -> String { p == 0 ? "0%" : String(format: "%+d%%", p) }

    func stepZoom(_ d: Int) {
        let n = min(max(zoomPct + d * 50, -200), 200)
        zoomPct = n
    }

    func addPoint(_ p: CGPoint, _ d: CGRect) {
        guard draw, d.width > 1 else { return }
        let q = CGPoint(x: min(max(p.x, d.minX), d.maxX), y: min(max(p.y, d.minY), d.maxY))
        if let l = live.last, abs(l.x - q.x) + abs(l.y - q.y) < 0.5 { return }
        live.append(q)
    }

    /// Çizgi bitince resme işlenir (geri alınabilir)
    func commitStroke(_ d: CGRect) {
        let pts = live
        live = []
        guard draw, let m = img, !pts.isEmpty, d.width > 1, d.height > 1 else { return }
        let n = pts.map { CGPoint(x: ($0.x - d.minX) / d.width, y: ($0.y - d.minY) / d.height) }
        let w = max(1, penWidth * m.size.width / d.width)
        pushUndo()
        img = Gallery.paint(m, n, NSColor(penColor), w)
        dirty = true
    }

    func rot(_ d: CGFloat) {
        guard let m = img else { return }
        pushUndo(); dirty = true
        let s = m.size, n = NSSize(width: s.height, height: s.width)
        img = Gallery.xform(m, n) {
            $0.translateX(by: n.width / 2, yBy: n.height / 2); $0.rotate(byDegrees: d)
            $0.translateX(by: -s.width / 2, yBy: -s.height / 2)
        }
    }

    func flip(_ h: Bool) {
        guard let m = img else { return }
        pushUndo(); dirty = true
        let s = m.size
        img = Gallery.xform(m, s) {
            $0.translateX(by: h ? s.width : 0, yBy: h ? 0 : s.height)
            $0.scaleX(by: h ? -1 : 1, yBy: h ? 1 : -1)
        }
    }

    func del() {
        guard files.indices.contains(i) else { return }
        crop = false; resetEdits()
        let u = files[i]
        guard Gallery.trash(u) else { return }          // onay sormadan doğrudan Çöp Kutusu'na
        Gallery.cache.removeObject(forKey: u as NSURL)
        files.remove(at: i)
        if files.isEmpty { loadTask?.cancel(); img = nil; i = 0 }
        else { i = min(i, files.count - 1); loadCurrent() }
    }

    /// Filtre ve/veya kırpma uygulanmış hâli "ad kopya.uzantı" olarak aynı klasöre kaydeder
    func saveCopy() {
        guard files.indices.contains(i), let m = img, canSave,
              var cg = m.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return }
        if !fl.isIdentity, let f = Filtering.apply(fl, to: cg) { cg = f }      // tam boyutta
        if crop {
            let W = CGFloat(cg.width), H = CGFloat(cg.height)
            guard let part = cg.cropping(to: CGRect(x: (cn.minX * W).rounded(), y: (cn.minY * H).rounded(),
                                                    width: (cn.width * W).rounded(), height: (cn.height * H).rounded())) else { return }
            cg = part
        }
        let src = files[i]
        let types: [String: NSBitmapImageRep.FileType] = ["png": .png, "jpg": .jpeg, "jpeg": .jpeg, "tiff": .tiff, "tif": .tiff, "bmp": .bmp, "gif": .gif]
        var ext = src.pathExtension.lowercased()
        if types[ext] == nil { ext = "png" }          // heic/webp/raw... → png
        guard let data = NSBitmapImageRep(cgImage: cg).representation(using: types[ext]!, properties: [.compressionFactor: 0.95]) else { return }
        let base = src.deletingPathExtension().lastPathComponent, dir = src.deletingLastPathComponent()
        var dest = dir.appendingPathComponent("\(base) kopya.\(ext)"); var n = 2
        while FileManager.default.fileExists(atPath: dest.path) { dest = dir.appendingPathComponent("\(base) kopya \(n).\(ext)"); n += 1 }
        guard (try? data.write(to: dest)) != nil else { return }
        crop = false; cn = Self.defCn; resetEdits()
        // Kaydedilen kopya açılır; böylece düzenlenmiş hâli görürsün
        let saved = dest
        Task { await refreshFiles(keeping: saved); loadCurrent() }
    }

    /// Önizlemeyi yeniden üretir (döndürme, çevirme ya da ayar değişince)
    func refilter() {
        guard !fl.isIdentity, let m = img,
              let cg = m.cgImage(forProposedRect: nil, context: nil, hints: nil),
              let out = Filtering.apply(fl, to: cg, maxSide: 2000) else { shown = nil; return }
        shown = NSImage(cgImage: out, size: m.size)
    }

    // MARK: Kırpma

    /// Resmin ekranda kapladığı alan
    func disp(_ sz: CGSize) -> CGRect {
        guard let m = img, m.size.width > 0, m.size.height > 0 else { return .zero }
        let k = min(sz.width / m.size.width, sz.height / m.size.height)
        let w = m.size.width * k, h = m.size.height * k
        return CGRect(x: (sz.width - w) / 2, y: (sz.height - h) / 2, width: w, height: h)
    }

    func cropRect(_ d: CGRect) -> CGRect {
        CGRect(x: d.minX + cn.minX * d.width, y: d.minY + cn.minY * d.height, width: cn.width * d.width, height: cn.height * d.height)
    }

    // Köşe/kenardan tutup boyutlandır, içinden tutup taşı
    func dragCrop(_ v: DragGesture.Value, _ d: CGRect) {
        guard crop, d.width > 1 else { return }
        let c = cropRect(d)
        if dm == nil {
            let t: CGFloat = 16, p = v.startLocation
            guard c.insetBy(dx: -t, dy: -t).contains(p) else { return }
            var m = CropMode()
            m.l = abs(p.x - c.minX) < t; m.r = abs(p.x - c.maxX) < t
            m.t = abs(p.y - c.minY) < t; m.b = abs(p.y - c.maxY) < t
            m.mv = !(m.l || m.r || m.t || m.b)
            dm = m; d0 = cn
        }
        guard let m = dm else { return }
        let dx = v.translation.width / d.width, dy = v.translation.height / d.height
        var x0 = d0.minX, x1 = d0.maxX, y0 = d0.minY, y1 = d0.maxY
        if m.mv {
            x0 = min(max(d0.minX + dx, 0), 1 - d0.width); y0 = min(max(d0.minY + dy, 0), 1 - d0.height)
            x1 = x0 + d0.width; y1 = y0 + d0.height
        } else {
            if m.l { x0 += dx }; if m.r { x1 += dx }; if m.t { y0 += dy }; if m.b { y1 += dy }
            x0 = min(max(x0, 0), x1 - 0.03); x1 = max(min(x1, 1), x0 + 0.03)
            y0 = min(max(y0, 0), y1 - 0.03); y1 = max(min(y1, 1), y0 + 0.03)
        }
        cn = CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
    }

    func cropOverlay(_ sz: CGSize, _ c: CGRect) -> some View {
        let pts = [CGPoint(x: c.minX, y: c.minY), CGPoint(x: c.midX, y: c.minY), CGPoint(x: c.maxX, y: c.minY),
                   CGPoint(x: c.minX, y: c.midY), CGPoint(x: c.maxX, y: c.midY),
                   CGPoint(x: c.minX, y: c.maxY), CGPoint(x: c.midX, y: c.maxY), CGPoint(x: c.maxX, y: c.maxY)]
        return ZStack(alignment: .topLeading) {
            Path { $0.addRect(CGRect(origin: .zero, size: sz)); $0.addRect(c) }
                .fill(Color.black.opacity(0.55), style: FillStyle(eoFill: true))
            Rectangle().stroke(Color.white, lineWidth: 1.5).frame(width: c.width, height: c.height).position(x: c.midX, y: c.midY)
            ForEach(pts.indices, id: \.self) { Rectangle().fill(Color.white).frame(width: 10, height: 10).position(pts[$0]) }
            Text("Üstteki Kaydet düğmesi: kopya olarak kaydet  •  Esc: iptal")
                .font(.caption).foregroundColor(.white).padding(6)
                .background(Color.black.opacity(0.6)).cornerRadius(6)
                .frame(maxWidth: .infinity, alignment: .top).padding(.top, 6)
        }.allowsHitTesting(false)
    }

    // MARK: Görünüm

    func btn(_ s: String, _ tip: String, _ on: Bool = false, _ f: @escaping () -> Void) -> some View {
        Button(action: f) {
            Image(systemName: s).font(.title2).frame(width: 40, height: 40)
                .background(on ? Color.blue : .clear).cornerRadius(8)
        }.buttonStyle(.plain).foregroundColor(.white).help(tip)
    }

    func row(_ title: String, _ v: Binding<Double>, _ r: ClosedRange<Double>, _ neutral: Double) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(title).font(.callout)
                Spacer()
                Text(String(format: "%+.2f", v.wrappedValue - neutral)).font(.caption).monospacedDigit().foregroundColor(.secondary)
            }
            .contentShape(Rectangle())
            .onTapGesture(count: 2) { pushUndo(); v.wrappedValue = neutral }
            Slider(value: v, in: r, onEditingChanged: { e in if e { pushUndo() } })
        }
    }

    var filterPanel: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Filtreler").font(.headline)
            Picker("", selection: Binding(get: { fl.tone }, set: { pushUndo(); fl.tone = $0 })) {
                Text("Renkli").tag(0); Text("Siyah-beyaz").tag(1); Text("Sepya").tag(2)
            }.pickerStyle(.segmented).labelsHidden()
            row("Pozlama", $fl.exposure, -2...2, 0)
            row("Parlaklık", $fl.brightness, -0.5...0.5, 0)
            row("Karşıtlık", $fl.contrast, 0.5...1.5, 1)
            row("Doygunluk", $fl.saturation, 0...2, 1)
            row("Sıcaklık", $fl.warmth, -1...1, 0)
            row("Keskinlik", $fl.sharpness, 0...2, 0)
            Text("Bir ayarın adına çift tıklayınca sıfırlanır").font(.caption).foregroundColor(.secondary)
            Button("Sıfırla") { pushUndo(); fl = Filters() }.disabled(fl.isIdentity)
            Spacer()
        }
        .padding(12).frame(width: 250)
        .background(Color.white.opacity(0.06))
    }

    var drawBar: some View {
        HStack(spacing: 10) {
            ForEach(Self.penColors.indices, id: \.self) { n in
                Button { penColor = Self.penColors[n] } label: {
                    Circle().fill(Self.penColors[n]).frame(width: 22, height: 22)
                        .overlay(Circle().stroke(Color.white, lineWidth: penColor == Self.penColors[n] ? 3 : 1))
                }.buttonStyle(.plain)
            }
            ColorPicker("", selection: $penColor).labelsHidden()
            Divider().frame(height: 24)
            Circle().fill(penColor).frame(width: min(penWidth, 30), height: min(penWidth, 30)).frame(width: 30, height: 30)
            Slider(value: $penWidth, in: 1...40).frame(width: 150)
            Text("\(Int(penWidth)) px").font(.caption).monospacedDigit().foregroundColor(.secondary).frame(width: 44, alignment: .leading)
        }
        .padding(.horizontal, 12).padding(.vertical, 6)
        .background(Color.white.opacity(0.06)).cornerRadius(8)
    }

    var zoomBar: some View {
        HStack(spacing: 6) {
            Spacer()
            Text("Yakınlaştırma").font(.caption).foregroundColor(.secondary)
            Button { stepZoom(-1) } label: { Image(systemName: "minus") }.buttonStyle(.plain).foregroundColor(.white)
            Picker("", selection: $zoomPct) {
                ForEach(Self.zoomSteps, id: \.self) { Text(zoomLabel($0)).tag($0) }
            }.pickerStyle(.menu).labelsHidden().frame(width: 84)
            Button { stepZoom(1) } label: { Image(systemName: "plus") }.buttonStyle(.plain).foregroundColor(.white)
        }
        .padding(.horizontal, 12)
        .disabled(crop || draw)
        .opacity(crop || draw ? 0.4 : 1)
    }

    var body: some View {
        VStack(spacing: 8) {
            HStack(spacing: 4) {
                btn("arrow.left", "Önceki") { go(-1) }
                btn("arrow.right", "Sonraki") { go(1) }
                Divider().frame(height: 30)
                btn("arrow.uturn.backward", "Geri al (⌘Z)") { undoStep() }
                    .disabled(undo.isEmpty).opacity(undo.isEmpty ? 0.35 : 1)
                    .keyboardShortcut("z", modifiers: .command)
                btn("square.and.arrow.down", "Kaydet — kopya olarak (⌘S)") { saveCopy() }
                    .disabled(!canSave).opacity(canSave ? 1 : 0.35)
                    .keyboardShortcut("s", modifiers: .command)
                Divider().frame(height: 30)
                btn("rotate.left", "Sola döndür") { rot(90) }
                btn("rotate.right", "Sağa döndür") { rot(-90) }
                btn("flip.horizontal", "Yatay çevir") { flip(true) }
                btn("arrow.up.and.down.righttriangle.up.righttriangle.down", "Dikey çevir") { flip(false) }
                btn("crop", "Kırp", crop) { crop.toggle(); cn = Self.defCn; if crop { draw = false; zoomPct = 0 } }
                btn("slider.horizontal.3", "Filtreler (F)", showFilters) { showFilters.toggle() }
                btn("pencil.tip", "Çizim", draw) { draw.toggle(); if draw { crop = false; zoomPct = 0 } }
                btn("arrow.up.left.and.arrow.down.right", "Tam ekran") { NSApp.keyWindow?.toggleFullScreen(nil) }
                btn("play.rectangle.on.rectangle", "Slayt gösterisi", play) { play.toggle(); NSApp.keyWindow?.toggleFullScreen(nil) }
                Divider().frame(height: 30)
                btn("trash", "Çöp Kutusu'na at") { del() }
                if files.indices.contains(i) {
                    ShareLink(item: files[i]) {
                        Image(systemName: "square.and.arrow.up").font(.title2).frame(width: 40, height: 40)
                    }.buttonStyle(.plain).foregroundColor(.white).help("Paylaş")
                }
                btn("folder", "Resim aç (⌘O)") { ViewerModel.shared.pick() }
            }.padding(.top, 8)

            if draw { drawBar }

            HStack(spacing: 0) {
            GeometryReader { g in
                let d = disp(g.size)
                ZStack(alignment: .topLeading) {
                    if let m = img {
                        Image(nsImage: shown ?? m).resizable().interpolation(.high).scaledToFit()
                            .frame(width: g.size.width, height: g.size.height)
                            .scaleEffect(zs).offset(pan)
                    } else if files.isEmpty {
                        Text("Gösterilecek resim yok").foregroundColor(.secondary)
                            .frame(width: g.size.width, height: g.size.height)
                    } else if failed {
                        Text("Resim açılamadı").foregroundColor(.secondary)
                            .frame(width: g.size.width, height: g.size.height)
                    }
                    if crop { cropOverlay(g.size, cropRect(d)) }
                    if draw && !live.isEmpty {
                        Path { q in q.move(to: live[0]); q.addLines(live) }
                            .stroke(penColor, style: StrokeStyle(lineWidth: penWidth, lineCap: .round, lineJoin: .round))
                            .allowsHitTesting(false)
                    }
                }
                .clipped()
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0)
                    .onChanged { v in
                        if draw { addPoint(v.location, d) }
                        else if crop { dragCrop(v, d) }
                        else if zs > 1 {
                            pan = CGSize(width: panBase.width + v.translation.width, height: panBase.height + v.translation.height)
                        }
                    }
                    .onEnded { _ in
                        if draw { commitStroke(d) }
                        dm = nil; panBase = pan
                    })
            }
            if showFilters { filterPanel }
            }

            zoomBar

            ScrollViewReader { proxy in
                ScrollView(.horizontal) {
                    LazyHStack(spacing: 14) {
                        ForEach(files.indices, id: \.self) { n in
                            Button { show(n) } label: {
                                GalleryThumb(url: files[n]).frame(width: 80, height: 80)
                                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(n == i ? Color.blue : .clear, lineWidth: 3))
                            }.buttonStyle(.plain)
                        }
                    }.padding(.horizontal, 12)
                }
                .onChange(of: i) { _, n in proxy.scrollTo(n, anchor: .center) }
            }.frame(height: 96)
        }
        .background(Color(red: 0.09, green: 0.1, blue: 0.11))
        .focusable().focused($foc).focusEffectDisabled()
        .onAppear { foc = true }
        .onChange(of: img) { _, _ in refilter() }
        .onChange(of: fl) { _, _ in refilter() }
        .onChange(of: showFilters) { _, _ in foc = true }
        .onChange(of: zoomPct) { _, _ in pan = .zero; panBase = .zero }
        .task { await startUp() }
        // Slayt gösterisi: zamanlayıcı yalnızca oynatılırken çalışır (boştayken CPU'yu uyandırmaz)
        .task(id: play) {
            guard play else { return }
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(3))
                if Task.isCancelled { break }
                go(1)
            }
        }
        .onKeyPress(.leftArrow) { go(-1); return .handled }
        .onKeyPress(.rightArrow) { go(1); return .handled }
        .onKeyPress(.upArrow) { rot(90); return .handled }      // yukarı: sola döndür
        .onKeyPress(.downArrow) { rot(-90); return .handled }   // aşağı: sağa döndür
        .onKeyPress("f") { showFilters.toggle(); return .handled }
        .onKeyPress(.escape) {
            if crop { crop = false; return .handled }
            if draw { draw = false; return .handled }
            return .ignored
        }
    }
}

enum ThumbCache {
    static let cache: NSCache<NSURL, NSImage> = {
        let c = NSCache<NSURL, NSImage>()
        c.countLimit = 600
        return c
    }()
}

struct GalleryThumb: View {
    let url: URL
    @State private var t: NSImage?

    var body: some View {
        Group { if let t { Image(nsImage: t).resizable().scaledToFit() } else { Color.clear } }
            .task(id: url) {
                if let c = ThumbCache.cache.object(forKey: url as NSURL) { t = c; return }
                let m = await Task.detached(priority: .utility) { Gallery.thumbnail(url) }.value
                if let m { ThumbCache.cache.setObject(m, forKey: url as NSURL) }
                t = m
            }
    }
}

// MARK: - Ana görünüm

struct ContentView: View {
    let model = ViewerModel.shared

    var body: some View {
        Group {
            if let u = model.start {
                GalleryView(start: u).id(model.token)
            } else {
                VStack(spacing: 14) {
                    Image(systemName: "photo.on.rectangle.angled")
                        .font(.system(size: 56))
                        .foregroundColor(.secondary)
                    Text("Bir resim açın")
                        .font(.title3)
                        .foregroundColor(.secondary)
                    Button("Resim Aç…") { model.pick() }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Color(red: 0.09, green: 0.1, blue: 0.11))
        .preferredColorScheme(.dark)
    }
}

// MARK: - Menü, uygulama

struct AppCommands: Commands {
    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("Aç…") { ViewerModel.shared.pick() }
                .keyboardShortcut("o", modifiers: .command)
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    /// Finder'da "Birlikte Aç" ile ya da varsayılan uygulama olarak gelen resimleri açar.
    func application(_ application: NSApplication, open urls: [URL]) {
        guard let u = urls.first(where: { Gallery.isImage($0) }) else { return }
        ViewerModel.shared.open(u)
        NSApp.activate(ignoringOtherApps: true)
    }
}

@main
struct DeckApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate

    init() {
        // Sistem menüleri ve diyaloglar da Türkçe olsun (bir sonraki açılıştan itibaren geçerli)
        UserDefaults.standard.set(["tr"], forKey: "AppleLanguages")
    }

    var body: some Scene {
        Window("Deck", id: "main") {
            ContentView()
                .frame(minWidth: 820, minHeight: 460)
        }
        .defaultSize(width: 1000, height: 700)
        .commands { AppCommands() }
    }
}
