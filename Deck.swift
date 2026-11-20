// Deck — macOS 15 Sequoia+ (Apple Silicon) için SwiftUI + Observation
// Çift panelli, koyu temalı, tamamen Türkçe dosya yöneticisi.

import SwiftUI
import AppKit
import UniformTypeIdentifiers
import Observation

// MARK: - Tema

enum Theme {
    static let background = Color(red: 0.055, green: 0.063, blue: 0.082)
    static let pane = Color(red: 0.075, green: 0.085, blue: 0.108)
    static let bar = Color(red: 0.11, green: 0.125, blue: 0.155)
    static let accent = Color(red: 0.95, green: 0.22, blue: 0.22)
}

// MARK: - Yardımcı tipler

enum Side { case left, right }

enum PromptKind {
    case newFolder
    case rename(URL)
}

struct AppError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

extension URL {
    /// Sondaki "/" farklarından etkilenmeyen karşılaştırma yolu
    var normPath: String { standardizedFileURL.path }
}

enum SortKey: String, CaseIterable, Identifiable {
    case name, created, modified, size, kind
    var id: String { rawValue }
    var title: String {
        switch self {
        case .name: "Ad"
        case .created: "Oluşturma Tarihi"
        case .modified: "Değiştirilme Tarihi"
        case .size: "Boyut"
        case .kind: "Tür"
        }
    }
}

enum ActionButton: String, CaseIterable, Identifiable {
    case newFolder, customize, sort, hidden, copy, move, duplicate, rename, unzip, zip, trash, share, info, bookmark
    var id: String { rawValue }

    var title: String {
        switch self {
        case .newFolder: "Yeni klasör"
        case .customize: "Düğmeleri ayarla"
        case .sort: "Sıralama"
        case .hidden: "Gizliyi göster"
        case .copy: "Kopyala"
        case .move: "Taşı"
        case .duplicate: "Kopyasını oluştur"
        case .rename: "Yeniden adlandır"
        case .unzip: "ZIP çıkar"
        case .zip: "ZIP oluştur"
        case .trash: "Sil"
        case .share: "Paylaş"
        case .info: "Özellikler"
        case .bookmark: "Yer imleri"
        }
    }

    var symbol: String {
        switch self {
        case .newFolder: "folder.badge.plus"
        case .customize: "square.grid.2x2"
        case .sort: "arrow.up.arrow.down"
        case .hidden: "eye.slash"
        case .copy: "doc.on.doc"
        case .move: "arrowshape.turn.up.right"
        case .duplicate: "plus.square.on.square"
        case .rename: "pencil.line"
        case .unzip: "tray.and.arrow.up"
        case .zip: "doc.zipper"
        case .trash: "trash"
        case .share: "square.and.arrow.up"
        case .info: "info.circle"
        case .bookmark: "bookmark.circle.fill"
        }
    }

    var color: Color {
        switch self {
        case .newFolder: .yellow
        case .customize: .white
        case .sort: .red
        case .hidden: .blue
        case .copy: .teal
        case .move: .cyan
        case .duplicate: .teal
        case .rename: .blue
        case .unzip: .yellow
        case .zip: .orange
        case .trash: .red
        case .share: .orange
        case .info: .blue
        case .bookmark: .yellow
        }
    }
}

/// Orta sütundaki Resim / Belge / Arşiv süzgeçleri
enum FileFilter: String, CaseIterable, Identifiable {
    case image, document, archive
    var id: String { rawValue }

    var title: String {
        switch self {
        case .image: "Resim"
        case .document: "Belge"
        case .archive: "Arşiv"
        }
    }

    var extensions: Set<String> {
        switch self {
        case .image:
            ["jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "tif", "tiff", "bmp", "svg",
             "ico", "avif", "psd", "raw", "dng", "cr2", "cr3", "nef", "arw"]
        case .document:
            ["pdf", "doc", "docx", "txt", "rtf", "odt", "pages", "md", "epub",
             "xls", "xlsx", "csv", "numbers", "ods",
             "ppt", "pptx", "key", "odp"]
        case .archive:
            ["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "cab", "dmg", "iso", "pkg"]
        }
    }
}

// MARK: - Dosya modeli

struct FileItem: Identifiable, Hashable {
    let url: URL
    let isDirectory: Bool
    let isPackage: Bool
    let created: Date
    let modified: Date
    let size: Int64
    let kind: String

    var id: URL { url }
    var name: String { url.lastPathComponent }
    /// Tek tıkla içine girilebilen klasör (.app gibi paketler hariç)
    var isEnterable: Bool { isDirectory && !isPackage }
    var isZip: Bool { !isDirectory && url.pathExtension.lowercased() == "zip" }
}

enum DateText {
    static let formatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "tr_TR")
        f.dateFormat = "d HH:mm"          // örn. "19 12:14" → ayın günü + saat
        return f
    }()
    static func string(_ date: Date) -> String {
        date == .distantPast ? "–" : formatter.string(from: date)
    }
}

enum Places {
    static var home: String { FileManager.default.homeDirectoryForCurrentUser.path }

    static func symbol(for url: URL) -> String {
        let p = url.normPath
        let h = home
        if p == h { return "house" }
        switch p {
        case h + "/Desktop": return "laptopcomputer"
        case h + "/Documents": return "doc.text"
        case h + "/Downloads": return "arrow.down.circle"
        case "/Applications": return "square.grid.2x2"
        case h + "/Pictures": return "photo"
        case h + "/Movies": return "film"
        case h + "/Music": return "music.note"
        default: return "folder"
        }
    }

    static func title(for url: URL) -> String {
        let p = url.normPath
        let h = home
        if p == h { return "Ana Sayfa" }
        switch p {
        case h + "/Desktop": return "Masaüstü"
        case h + "/Documents": return "Belgeler"
        case h + "/Downloads": return "İndirilenler"
        case "/Applications": return "Uygulamalar"
        case h + "/Pictures": return "Resimler"
        case h + "/Movies": return "Filmler"
        case h + "/Music": return "Müzik"
        default: return FileManager.default.displayName(atPath: p)
        }
    }
}

/// Finder'ın kendi (sistem) simgelerini kullanır.
enum IconCache {
    static let cache = NSCache<NSString, NSImage>()

    static func icon(for item: FileItem) -> NSImage {
        let ext = item.url.pathExtension.lowercased()
        let perPath = item.isDirectory || item.isPackage || ext.isEmpty
        let key = perPath ? "p:" + item.url.path : "e:" + ext
        if let cached = cache.object(forKey: key as NSString) { return cached }
        let image: NSImage
        if !perPath, let type = UTType(filenameExtension: ext) {
            image = NSWorkspace.shared.icon(for: type)
        } else {
            image = NSWorkspace.shared.icon(forFile: item.url.path)
        }
        cache.setObject(image, forKey: key as NSString)
        return image
    }

    static func icon(forPath path: String) -> NSImage {
        let key = "p:" + path
        if let cached = cache.object(forKey: key as NSString) { return cached }
        let image = NSWorkspace.shared.icon(forFile: path)
        cache.setObject(image, forKey: key as NSString)
        return image
    }
}

struct ProcResult {
    let status: Int32
    let output: String
}

func runProcess(_ exe: String, _ args: [String], cwd: URL? = nil) throws -> ProcResult {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: exe)
    p.arguments = args
    if let cwd { p.currentDirectoryURL = cwd }
    let pipe = Pipe()
    p.standardError = pipe
    p.standardOutput = FileHandle.nullDevice
    try p.run()
    let data = pipe.fileHandleForReading.readDataToEndOfFile()
    p.waitUntilExit()
    return ProcResult(status: p.terminationStatus, output: String(decoding: data, as: UTF8.self))
}

// MARK: - Panel modeli

@Observable
final class PaneModel {
    var currentURL: URL
    var items: [FileItem] = []
    var selection: Set<URL> = []
    var backStack: [URL] = []

    /// Resim / Belge / Arşiv süzgeci (klasörler her zaman görünür, böylece gezinilebilir)
    var typeFilter: FileFilter? { didSet { pruneSelection() } }
    /// Arama metni: bulunduğu klasörde ada göre anında süzer
    var query = "" { didSet { pruneSelection() } }

    @ObservationIgnored weak var app: AppModel?
    @ObservationIgnored private var anchor: URL?
    @ObservationIgnored private var lastNavigation = Date.distantPast
    @ObservationIgnored private var watcher: DispatchSourceFileSystemObject?

    init(url: URL) {
        currentURL = url.standardizedFileURL
    }

    var selectedItems: [FileItem] { items.filter { selection.contains($0.url) } }

    var isFiltering: Bool {
        typeFilter != nil || !query.trimmingCharacters(in: .whitespaces).isEmpty
    }

    /// Süzgeç ve aramaya göre görünen öğeler
    var visibleItems: [FileItem] {
        let q = query.trimmingCharacters(in: .whitespaces)
        if typeFilter == nil && q.isEmpty { return items }
        return items.filter { item in
            if let f = typeFilter, !item.isDirectory || item.isPackage {
                if !f.extensions.contains(item.url.pathExtension.lowercased()) { return false }
            }
            if !q.isEmpty {
                return item.name.range(of: q, options: [.caseInsensitive, .diacriticInsensitive], locale: .current) != nil
            }
            return true
        }
    }

    private func pruneSelection() {
        let present = Set(visibleItems.map(\.url))
        let kept = selection.intersection(present)
        if kept != selection { selection = kept }
    }

    /// Klasöre girdikten hemen sonra gelen ikinci tıkı (çift tık) yok sayar.
    var recentlyNavigated: Bool { Date().timeIntervalSince(lastNavigation) < 0.35 }

    func start() {
        navigate(to: currentURL, record: false)
    }

    // Gezinme

    func navigate(to url: URL, record: Bool = true) {
        let target = url.standardizedFileURL
        let list: [FileItem]
        do {
            list = try fetch(target)
        } catch {
            app?.showToast("“\(Places.title(for: target))” açılamadı. Erişim izni gerekebilir (Sistem Ayarları › Gizlilik ve Güvenlik).")
            return
        }
        if record, target.normPath != currentURL.normPath {
            backStack.append(currentURL)
        }
        currentURL = target
        items = sort(list)
        selection = []
        anchor = nil
        lastNavigation = Date()
        startWatching()
    }

    func goBack() {
        guard let last = backStack.popLast() else { return }
        navigate(to: last, record: false)
    }

    func goUp() {
        if currentURL.normPath == "/" { return }
        navigate(to: currentURL.deletingLastPathComponent())
    }

    func reload() {
        do {
            items = sort(try fetch(currentURL))
            let present = Set(items.map(\.url))
            selection = selection.intersection(present)
        } catch {
            // Klasör silindiyse bir üst klasöre çık
            if currentURL.normPath != "/" {
                navigate(to: currentURL.deletingLastPathComponent(), record: false)
            } else {
                items = []
            }
        }
    }

    // Seçim

    func click(_ item: FileItem, flags: NSEvent.ModifierFlags) {
        if recentlyNavigated { return }
        if flags.contains(.command) {
            toggleSelect(item)
        } else if flags.contains(.shift),
                  let a = anchor,
                  case let list = visibleItems,
                  let i = list.firstIndex(where: { $0.url == a }),
                  let j = list.firstIndex(where: { $0.url == item.url }) {
            selection = Set(list[min(i, j)...max(i, j)].map(\.url))
        } else if item.isEnterable {
            navigate(to: item.url)               // tek tıkla klasöre gir
        } else {
            selection = [item.url]
            anchor = item.url
        }
    }

    func toggleSelect(_ item: FileItem) {
        if selection.contains(item.url) { selection.remove(item.url) } else { selection.insert(item.url) }
        anchor = item.url
    }

    func ensureSelected(_ item: FileItem) {
        if !selection.contains(item.url) {
            selection = [item.url]
            anchor = item.url
        }
    }

    // İç işler

    private func fetch(_ url: URL) throws -> [FileItem] {
        let keys: [URLResourceKey] = [
            .isDirectoryKey, .isPackageKey, .isSymbolicLinkKey,
            .creationDateKey, .contentModificationDateKey,
            .fileSizeKey, .localizedTypeDescriptionKey
        ]
        var options: FileManager.DirectoryEnumerationOptions = []
        if app?.showHidden != true { options.insert(.skipsHiddenFiles) }
        let urls = try FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: keys, options: options)
        let keySet = Set(keys)
        return urls.map { u in
            let v = try? u.resourceValues(forKeys: keySet)
            var isDir = v?.isDirectory ?? false
            if v?.isSymbolicLink == true {
                isDir = (try? u.resolvingSymlinksInPath().resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
            }
            return FileItem(
                url: u,
                isDirectory: isDir,
                isPackage: v?.isPackage ?? false,
                created: v?.creationDate ?? .distantPast,
                modified: v?.contentModificationDate ?? .distantPast,
                size: Int64(v?.fileSize ?? 0),
                kind: v?.localizedTypeDescription ?? ""
            )
        }
    }

    private func sort(_ list: [FileItem]) -> [FileItem] {
        let key = app?.sortKey ?? .created
        let asc = app?.ascending ?? false
        return list.sorted { a, b in
            let r: ComparisonResult
            switch key {
            case .name: r = a.name.localizedStandardCompare(b.name)
            case .created: r = a.created.compare(b.created)
            case .modified: r = a.modified.compare(b.modified)
            case .size: r = a.size < b.size ? .orderedAscending : (a.size > b.size ? .orderedDescending : .orderedSame)
            case .kind: r = a.kind.localizedStandardCompare(b.kind)
            }
            if r == .orderedSame {
                return a.name.localizedStandardCompare(b.name) == .orderedAscending
            }
            return asc ? r == .orderedAscending : r == .orderedDescending
        }
    }

    /// Klasörde değişiklik olunca listeyi otomatik yeniler.
    private func startWatching() {
        watcher?.cancel()
        watcher = nil
        let fd = open(currentURL.path, O_EVTONLY)
        guard fd >= 0 else { return }
        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fd,
            eventMask: [.write, .delete, .rename, .extend],
            queue: .main
        )
        source.setEventHandler { [weak self] in
            MainActor.assumeIsolated { self?.reload() }
        }
        source.setCancelHandler { close(fd) }
        source.resume()
        watcher = source
    }
}

// MARK: - Uygulama modeli

@Observable
final class AppModel {
    var left: PaneModel
    var right: PaneModel
    var activeSide: Side = .right

    // Varsayılan sıralama: oluşturma tarihi, en yeni en üstte
    var sortKey: SortKey = .created
    var ascending = false
    var showHidden = false

    var bookmarks: [URL]
    var hiddenButtons: Set<String>

    var toast: String?
    var showPrompt = false
    var prompt: PromptKind = .newFolder
    var promptText = ""
    var showCustomize = false

    @ObservationIgnored private var toastTask: Task<Void, Never>?
    @ObservationIgnored private var sharePicker: NSSharingServicePicker?

    init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        left = PaneModel(url: home)
        right = PaneModel(url: home.appendingPathComponent("Downloads"))
        // Üst bar artık boş başlar; eski varsayılan yer imleri bir kez temizlenir.
        if !UserDefaults.standard.bool(forKey: "bookmarksClearedV2") {
            UserDefaults.standard.removeObject(forKey: "bookmarks")
            UserDefaults.standard.set(true, forKey: "bookmarksClearedV2")
        }
        let saved = UserDefaults.standard.stringArray(forKey: "bookmarks")
        bookmarks = saved?.map { URL(fileURLWithPath: $0) } ?? []
        hiddenButtons = Set(UserDefaults.standard.stringArray(forKey: "hiddenButtons") ?? [])
        left.app = self
        right.app = self
        left.start()
        right.start()
    }

    var active: PaneModel { activeSide == .left ? left : right }
    var other: PaneModel { activeSide == .left ? right : left }

    func reloadAll() {
        left.reload()
        right.reload()
    }

    // MARK: Bildirim

    func showToast(_ text: String) {
        toast = text
        toastTask?.cancel()
        toastTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(4))
            if !Task.isCancelled { self?.toast = nil }
        }
    }

    // MARK: Sıralama / gizli dosyalar

    func setSort(_ key: SortKey) {
        guard key != sortKey else { return }
        sortKey = key
        ascending = (key == .name || key == .kind)
        reloadAll()
    }

    func toggleSort(_ key: SortKey) {
        if sortKey == key {
            ascending.toggle()
        } else {
            sortKey = key
            ascending = (key == .name || key == .kind)
        }
        reloadAll()
    }

    func toggleSortDate() {
        if sortKey == .created || sortKey == .modified {
            ascending.toggle()
        } else {
            sortKey = .created
            ascending = false
        }
        reloadAll()
    }

    func setAscending(_ value: Bool) {
        ascending = value
        reloadAll()
    }

    func toggleHidden() {
        showHidden.toggle()
        reloadAll()
        showToast(showHidden ? "Gizli dosyalar gösteriliyor." : "Gizli dosyalar gizlendi.")
    }

    // MARK: Yer imleri (üst bar)

    var bookmarkTarget: URL {
        let s = active.selectedItems
        if s.count == 1, s[0].isEnterable { return s[0].url }
        return active.currentURL
    }

    func isBookmarked(_ url: URL) -> Bool {
        bookmarks.contains { $0.normPath == url.normPath }
    }

    /// Klasör barda varsa kaldırır, yoksa ekler.
    func toggleBookmark() {
        let target = bookmarkTarget.standardizedFileURL
        let name = Places.title(for: target)
        if let i = bookmarks.firstIndex(where: { $0.normPath == target.normPath }) {
            bookmarks.remove(at: i)
            showToast("“\(name)” yer imlerinden kaldırıldı.")
        } else {
            bookmarks.append(target)
            showToast("“\(name)” yer imlerine eklendi.")
        }
        saveBookmarks()
    }

    func removeBookmark(_ url: URL) {
        bookmarks.removeAll { $0.normPath == url.normPath }
        saveBookmarks()
    }

    func resetBookmarks() {
        bookmarks = []
        saveBookmarks()
    }

    private func saveBookmarks() {
        UserDefaults.standard.set(bookmarks.map(\.path), forKey: "bookmarks")
    }

    func setButton(_ b: ActionButton, visible: Bool) {
        if visible { hiddenButtons.remove(b.rawValue) } else { hiddenButtons.insert(b.rawValue) }
        UserDefaults.standard.set(Array(hiddenButtons), forKey: "hiddenButtons")
    }

    // MARK: İşlemler

    private func requireSelection() -> [FileItem]? {
        let s = active.selectedItems
        if s.isEmpty {
            showToast("Önce öğe seçin: dosyaya tek tıklayın, klasör seçmek için ⌘ tuşuyla tıklayın.")
            return nil
        }
        return s
    }

    var promptTitle: String {
        switch prompt {
        case .newFolder: "Yeni klasör"
        case .rename: "Yeniden adlandır"
        }
    }

    func beginNewFolder() {
        let url = AppModel.uniqueURL(base: "Yeni Klasör", ext: "", in: active.currentURL)
        prompt = .newFolder
        promptText = url.lastPathComponent
        showPrompt = true
    }

    func beginRename() {
        guard let s = requireSelection() else { return }
        guard s.count == 1 else {
            showToast("Yeniden adlandırmak için tek bir öğe seçin.")
            return
        }
        prompt = .rename(s[0].url)
        promptText = s[0].name
        showPrompt = true
    }

    func confirmPrompt() {
        let name = promptText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, !name.contains("/"), !name.contains(":") else {
            showToast("Geçersiz ad.")
            return
        }
        let fm = FileManager.default
        switch prompt {
        case .newFolder:
            let url = active.currentURL.appendingPathComponent(name)
            if fm.fileExists(atPath: url.path) { showToast("Bu adda bir öğe zaten var."); return }
            do {
                try fm.createDirectory(at: url, withIntermediateDirectories: false)
                active.reload()
                active.selection = [url]
            } catch {
                showToast("Klasör oluşturulamadı: \(error.localizedDescription)")
            }
        case .rename(let old):
            let new = old.deletingLastPathComponent().appendingPathComponent(name)
            let onlyCaseChange = new.path.lowercased() == old.path.lowercased()
            if !onlyCaseChange, fm.fileExists(atPath: new.path) { showToast("Bu adda bir öğe zaten var."); return }
            do {
                try fm.moveItem(at: old, to: new)
                active.reload()
                active.selection = [new]
            } catch {
                showToast("Yeniden adlandırılamadı: \(error.localizedDescription)")
            }
        }
    }

    /// Etkin paneldeki seçimi diğer panele kopyalar / taşır.
    func transfer(move: Bool) {
        guard let items = requireSelection() else { return }
        let dest = other.currentURL
        if move, active.currentURL.normPath == dest.normPath {
            showToast("Kaynak ve hedef klasör aynı.")
            return
        }
        let list = items.map { ($0.url, $0.isEnterable) }
        showToast(move ? "Taşınıyor…" : "Kopyalanıyor…")
        active.selection = []
        background {
            let fm = FileManager.default
            var count = 0
            var lastError: Error?
            for (src, isDir) in list {
                let target = AppModel.uniqueURL(for: src, isDir: isDir, in: dest)
                do {
                    if move { try fm.moveItem(at: src, to: target) } else { try fm.copyItem(at: src, to: target) }
                    count += 1
                } catch {
                    lastError = error
                }
            }
            if count == 0, let e = lastError { throw e }
            return "\(count) öğe \(move ? "taşındı" : "kopyalandı")."
        }
    }

    func duplicateSelected() {
        guard let items = requireSelection() else { return }
        let list = items.map { ($0.url, $0.isEnterable) }
        let dir = active.currentURL
        background {
            for (u, isDir) in list {
                let dest = AppModel.uniqueURL(for: u, isDir: isDir, in: dir, suffix: " kopya")
                try FileManager.default.copyItem(at: u, to: dest)
            }
            return "\(list.count) öğenin kopyası oluşturuldu."
        }
    }

    func trashSelected() {
        guard let items = requireSelection() else { return }
        var done = 0
        var lastError: Error?
        for item in items {
            do {
                try FileManager.default.trashItem(at: item.url, resultingItemURL: nil)
                done += 1
            } catch {
                lastError = error
            }
        }
        active.selection = []
        reloadAll()
        if let e = lastError, done == 0 {
            showToast("Silinemedi: \(e.localizedDescription)")
        } else {
            showToast("\(done) öğe Çöp Kutusu'na taşındı.")
        }
    }

    func extractSelectedZips() {
        guard let items = requireSelection() else { return }
        let zips = items.filter(\.isZip).map(\.url)
        guard !zips.isEmpty else {
            showToast("Seçili öğeler arasında ZIP dosyası yok.")
            return
        }
        extractZips(zips)
    }

    /// ZIP'i, adını taşıyan bir klasöre çıkarır (çift tıkta da bu çalışır).
    func extractZips(_ urls: [URL]) {
        showToast("ZIP çıkarılıyor…")
        background {
            var done = 0
            for zip in urls {
                try AppModel.extract(zip)
                done += 1
            }
            return "\(done) arşiv klasöre çıkarıldı."
        }
    }

    func zipSelected() {
        guard let items = requireSelection() else { return }
        let dir = active.currentURL
        let urls = items.map(\.url)
        showToast("ZIP oluşturuluyor…")
        background {
            let dest: URL
            if urls.count == 1 {
                dest = AppModel.uniqueURL(base: urls[0].lastPathComponent, ext: "zip", in: dir)
                let r = try runProcess("/usr/bin/ditto", ["-c", "-k", "--sequesterRsrc", "--keepParent", urls[0].path, dest.path])
                guard r.status == 0 else { throw AppError(message: "ZIP oluşturulamadı. \(r.output)") }
            } else {
                dest = AppModel.uniqueURL(base: "Arşiv", ext: "zip", in: dir)
                let r = try runProcess("/usr/bin/zip", ["-r", "-y", "-q", dest.path] + urls.map(\.lastPathComponent), cwd: dir)
                guard r.status == 0 else { throw AppError(message: "ZIP oluşturulamadı. \(r.output)") }
            }
            return "“\(dest.lastPathComponent)” oluşturuldu."
        }
    }

    func shareSelected() {
        guard let items = requireSelection() else { return }
        let picker = NSSharingServicePicker(items: items.map { $0.url as Any })
        sharePicker = picker
        if let window = NSApp.keyWindow, let view = window.contentView {
            let p = view.convert(window.mouseLocationOutsideOfEventStream, from: nil)
            picker.show(relativeTo: NSRect(x: p.x, y: p.y, width: 1, height: 1), of: view, preferredEdge: .minY)
        }
    }

    /// Finder'ın kendi "Bilgi Al" penceresini açar (sistem özellikleri).
    func showInfo() {
        let s = active.selectedItems
        let targets = s.isEmpty ? [active.currentURL] : s.map(\.url)
        for url in targets.prefix(5) {
            let path = url.path
                .replacingOccurrences(of: "\\", with: "\\\\")
                .replacingOccurrences(of: "\"", with: "\\\"")
            let source = """
            tell application "Finder"
                activate
                open information window of (POSIX file "\(path)" as alias)
            end tell
            """
            var error: NSDictionary?
            NSAppleScript(source: source)?.executeAndReturnError(&error)
            if let error {
                let msg = error[NSAppleScript.errorMessage] as? String ?? "Bilinmeyen hata"
                showToast("Özellikler açılamadı: \(msg)")
            }
        }
    }

    // MARK: Arka plan işleri

    private func background(_ work: @escaping @Sendable () throws -> String) {
        Task.detached {
            do {
                let message = try work()
                await MainActor.run {
                    self.reloadAll()
                    self.showToast(message)
                }
            } catch {
                await MainActor.run {
                    self.reloadAll()
                    self.showToast("Hata: \(error.localizedDescription)")
                }
            }
        }
    }

    static func extract(_ zip: URL) throws {
        let fm = FileManager.default
        let dir = zip.deletingLastPathComponent()
        let name = zip.deletingPathExtension().lastPathComponent
        let temp = dir.appendingPathComponent(".cikar-\(UUID().uuidString)")
        try fm.createDirectory(at: temp, withIntermediateDirectories: false)
        defer { try? fm.removeItem(at: temp) }

        let r = try runProcess("/usr/bin/ditto", ["-x", "-k", zip.path, temp.path])
        guard r.status == 0 else { throw AppError(message: "ZIP çıkarılamadı. \(r.output)") }
        try? fm.removeItem(at: temp.appendingPathComponent("__MACOSX"))

        let contents = try fm.contentsOfDirectory(at: temp, includingPropertiesForKeys: [.isDirectoryKey], options: [])
        let meaningful = contents.filter { $0.lastPathComponent != ".DS_Store" }
        let result = uniqueURL(base: name, ext: "", in: dir)

        // Arşivde tek bir kök klasör varsa "klasör içinde klasör" olmasın diye bir seviye açılır.
        if meaningful.count == 1,
           (try? meaningful[0].resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true {
            try fm.moveItem(at: meaningful[0], to: result)
        } else {
            try fm.moveItem(at: temp, to: result)
        }
    }

    static func uniqueURL(for src: URL, isDir: Bool, in dir: URL, suffix: String = "") -> URL {
        let ext = isDir ? "" : src.pathExtension
        let base = isDir ? src.lastPathComponent : src.deletingPathExtension().lastPathComponent
        return uniqueURL(base: base + suffix, ext: ext, in: dir)
    }

    static func uniqueURL(base: String, ext: String, in dir: URL) -> URL {
        func make(_ b: String) -> URL {
            dir.appendingPathComponent(ext.isEmpty ? b : b + "." + ext)
        }
        var candidate = make(base)
        var i = 2
        while FileManager.default.fileExists(atPath: candidate.path) {
            candidate = make("\(base) \(i)")
            i += 1
        }
        return candidate
    }
}

// MARK: - Görünümler

struct ContentView: View {
    @Environment(AppModel.self) var app

    var body: some View {
        @Bindable var model = app
        VStack(spacing: 0) {
            TopBar()
            HStack(spacing: 0) {
                PaneView(pane: app.left, side: .left)
                CenterColumn().frame(width: 196)
                PaneView(pane: app.right, side: .right)
            }
        }
        .background(Theme.background)
        .preferredColorScheme(.dark)
        .alert(model.promptTitle, isPresented: $model.showPrompt) {
            TextField("Ad", text: $model.promptText)
            Button("Tamam") { app.confirmPrompt() }
            Button("İptal", role: .cancel) {}
        }
        .sheet(isPresented: $model.showCustomize) { CustomizeSheet() }
        .overlay(alignment: .bottom) {
            if let t = app.toast {
                Text(t)
                    .font(.system(size: 14, weight: .medium))
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .background(.ultraThinMaterial, in: Capsule())
                    .padding(.bottom, 18)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .animation(.easeInOut(duration: 0.2), value: app.toast)
    }
}

// Üst bar: yer imleri

struct TopBar: View {
    @Environment(AppModel.self) var app
    static let height: CGFloat = 62

    var body: some View {
        GeometryReader { geo in
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 0) {
                    ForEach(app.bookmarks, id: \.path) { url in
                        BookmarkChip(url: url)
                    }
                    if app.bookmarks.isEmpty {
                        Text("Bir klasör seçip ortadaki “Yer imleri” düğmesine basarak buraya ekleyin.")
                            .font(.system(size: 14))
                            .foregroundStyle(.white.opacity(0.35))
                            .frame(maxHeight: .infinity)
                    }
                }
                .padding(.horizontal, 5)
                // İçerik çubuğun tüm yüksekliğini kaplar; simgeler başlık çubuğu ile
                // alttaki siyah alan arasında dikey olarak ortalanır.
                .frame(minWidth: geo.size.width, minHeight: TopBar.height, maxHeight: TopBar.height)
            }
            .frame(height: TopBar.height)
        }
        .frame(height: TopBar.height)
    }
}

struct BookmarkChip: View {
    let url: URL
    @Environment(AppModel.self) var app

    var body: some View {
        let isCurrent = app.active.currentURL.normPath == url.normPath
        Button {
            // Hangi panel etkinse (en son tıklanan), klasör o panelde açılır.
            app.active.navigate(to: url)
            app.reloadAll()
        } label: {
            HStack(spacing: 10) {
                Image(systemName: Places.symbol(for: url))
                    .font(.system(size: 22))
                    .foregroundStyle(Theme.accent)
                Text(Places.title(for: url))
                    .font(.system(size: 16))
                    .foregroundStyle(.white.opacity(0.9))
                    .lineLimit(1)
            }
            .padding(.horizontal, 14)
            // Görünen vurgu: çubuğun içinde ortalı, yuvarlak köşeli kutu
            .frame(maxHeight: .infinity)
            .background(
                RoundedRectangle(cornerRadius: 12)
                    .fill(isCurrent ? Color.white.opacity(0.12) : Color.clear)
                    .padding(.vertical, 8)
            )
            // Tıklanabilir alan: görünen kutudan taşar, çubuğun tüm yüksekliğini ve
            // iki yandaki boşluğun yarısını kapsar (fare ucu nereye gelirse gelsin çalışır).
            .padding(.horizontal, 7)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .frame(height: TopBar.height)
        .contextMenu {
            Button("Yer imlerinden kaldır") { app.removeBookmark(url) }
        }
    }
}

// Panel

struct PaneView: View {
    let pane: PaneModel
    let side: Side
    @Environment(AppModel.self) var app

    var body: some View {
        VStack(spacing: 0) {
            BreadcrumbBar(pane: pane, side: side)
            ColumnHeader()
            FileList(pane: pane, side: side)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.pane)
        .overlay(
            Rectangle()
                .stroke(app.activeSide == side ? Theme.accent.opacity(0.55) : Color.white.opacity(0.06), lineWidth: 1)
                .allowsHitTesting(false)
        )
        // Panelin herhangi bir yerine (liste, başlık, boş alan) tıklanınca bu panel etkin olur.
        .background(ClickTracker { app.activeSide = side })
    }
}

/// Fare tıklamasını (sol/sağ) yakalar; tıklama bu görünümün alanı içindeyse `onClick` çağrılır.
/// Olay, düğme eylemlerinden önce işlendiği için üstteki yer imi düğmeleri doğru paneli kullanır.
struct ClickTracker: NSViewRepresentable {
    let onClick: () -> Void

    func makeNSView(context: Context) -> TrackerView {
        let v = TrackerView()
        v.onClick = onClick
        return v
    }

    func updateNSView(_ nsView: TrackerView, context: Context) {
        nsView.onClick = onClick
    }

    final class TrackerView: NSView {
        var onClick: (() -> Void)?
        private var monitor: Any?

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] event in
                MainActor.assumeIsolated { self?.handle(event) }
                return event
            }
        }

        private func handle(_ event: NSEvent) {
            guard event.window === window else { return }
            let p = convert(event.locationInWindow, from: nil)
            if bounds.contains(p) { onClick?() }
        }

        deinit {
            if let monitor { NSEvent.removeMonitor(monitor) }
        }
    }
}

struct BreadcrumbBar: View {
    let pane: PaneModel
    let side: Side
    @Environment(AppModel.self) var app

    var crumbs: [(name: String, url: URL)] {
        var result: [(name: String, url: URL)] = []
        var url = pane.currentURL.standardizedFileURL
        while true {
            let isRoot = url.path == "/"
            let name = isRoot ? FileManager.default.displayName(atPath: "/") : url.lastPathComponent
            result.insert((name: name, url: url), at: 0)
            if isRoot { break }
            url = url.deletingLastPathComponent()
        }
        return result
    }

    var body: some View {
        let list = crumbs
        HStack(spacing: 8) {
            Button { app.activeSide = side; pane.goBack() } label: {
                Image(systemName: "chevron.backward")
            }
            .disabled(pane.backStack.isEmpty)
            .help("Geri")

            Button { app.activeSide = side; pane.goUp() } label: {
                Image(systemName: "arrow.up")
            }
            .disabled(pane.currentURL.normPath == "/")
            .help("Üst klasör")

            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(Array(list.enumerated()), id: \.offset) { i, c in
                            if i > 0 {
                                Image(systemName: "chevron.right")
                                    .font(.system(size: 11, weight: .semibold))
                                    .foregroundStyle(.secondary)
                            }
                            Button {
                                app.activeSide = side
                                pane.navigate(to: c.url)
                            } label: {
                                HStack(spacing: 6) {
                                    if i == 0 {
                                        Image(systemName: "internaldrive")
                                            .font(.system(size: 17))
                                            .foregroundStyle(.secondary)
                                    } else {
                                        Image(nsImage: IconCache.icon(forPath: c.url.path))
                                            .resizable()
                                            .frame(width: 22, height: 22)
                                    }
                                    Text(c.name)
                                        .font(.system(size: 16))
                                        .foregroundStyle(.white.opacity(0.92))
                                        .lineLimit(1)
                                }
                            }
                            .buttonStyle(.plain)
                            .id(i)
                        }
                    }
                }
                .onChange(of: pane.currentURL) {
                    proxy.scrollTo(list.count - 1, anchor: .trailing)
                }
                .onAppear {
                    proxy.scrollTo(list.count - 1, anchor: .trailing)
                }
            }
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, 12)
        .frame(height: 46)
        .background(RoundedRectangle(cornerRadius: 12).fill(Theme.bar))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(app.activeSide == side ? Theme.accent.opacity(0.85) : Color.clear, lineWidth: 1.5)
                .allowsHitTesting(false)
        )
        .padding(8)
    }
}

struct ColumnHeader: View {
    @Environment(AppModel.self) var app

    var body: some View {
        HStack(spacing: 8) {
            Button { app.toggleSort(.name) } label: {
                HStack(spacing: 6) {
                    Text("Ad")
                    if app.sortKey == .name { arrow }
                }
            }
            Spacer()
            Button { app.toggleSortDate() } label: {
                HStack(spacing: 6) {
                    Text("Tarih")
                    if app.sortKey == .created || app.sortKey == .modified { arrow }
                }
            }
            .frame(width: 86, alignment: .leading)
        }
        .buttonStyle(.plain)
        .font(.system(size: 15, weight: .medium))
        .foregroundStyle(.white.opacity(0.75))
        .padding(.horizontal, 14)
        .frame(height: 34)
        .overlay(alignment: .bottom) { Divider().opacity(0.4) }
    }

    private var arrow: some View {
        Image(systemName: app.ascending ? "chevron.up" : "chevron.down")
            .font(.system(size: 11, weight: .bold))
    }
}

struct FileList: View {
    let pane: PaneModel
    let side: Side

    var body: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                ForEach(Array(pane.visibleItems.enumerated()), id: \.element.id) { i, item in
                    FileRow(pane: pane, side: side, item: item, striped: i % 2 == 1)
                }
            }
        }
        .id(pane.currentURL)
        .overlay {
            if pane.visibleItems.isEmpty {
                Text(pane.isFiltering && !pane.items.isEmpty ? "Sonuç bulunamadı" : "Bu klasör boş")
                    .font(.system(size: 15))
                    .foregroundStyle(.secondary)
            }
        }
    }
}

struct FileRow: View {
    let pane: PaneModel
    let side: Side
    let item: FileItem
    let striped: Bool
    @Environment(AppModel.self) var app
    @State private var hover = false

    var selected: Bool { pane.selection.contains(item.url) }

    var body: some View {
        let date = app.sortKey == .modified ? item.modified : item.created
        HStack(spacing: 12) {
            Image(nsImage: IconCache.icon(for: item))
                .resizable()
                .interpolation(.high)
                .frame(width: 28, height: 28)
            Text(item.name)
                .font(.system(size: 16))
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 8)
            Text(DateText.string(date))
                .font(.system(size: 16).monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 86, alignment: .leading)
        }
        .padding(.horizontal, 14)
        .frame(height: 40)
        .background(rowBackground)
        .contentShape(Rectangle())
        .onHover { hover = $0 }
        .simultaneousGesture(TapGesture(count: 2).onEnded { doubleClick() })
        .simultaneousGesture(TapGesture().onEnded { singleClick() })
        .contextMenu {
            Button("Aç") { open() }
            Button(selected ? "Seçimi kaldır" : "Seç") {
                app.activeSide = side
                pane.toggleSelect(item)
            }
            Divider()
            if item.isZip {
                Button("ZIP'i klasöre çıkar") { prepare(); app.extractSelectedZips() }
            }
            Button("Yeniden adlandır") { prepare(); app.beginRename() }
            Button("Kopyasını oluştur") { prepare(); app.duplicateSelected() }
            Button("ZIP oluştur") { prepare(); app.zipSelected() }
            Button("Finder'da göster") { NSWorkspace.shared.activateFileViewerSelecting([item.url]) }
            Divider()
            Button("Sil", role: .destructive) { prepare(); app.trashSelected() }
        }
    }

    private var rowBackground: Color {
        if selected { return Theme.accent.opacity(0.30) }
        if hover { return Color.white.opacity(0.07) }
        return striped ? Color.white.opacity(0.035) : .clear
    }

    private func prepare() {
        app.activeSide = side
        pane.ensureSelected(item)
    }

    private func open() {
        app.activeSide = side
        if item.isEnterable { pane.navigate(to: item.url) } else { NSWorkspace.shared.open(item.url) }
    }

    private func singleClick() {
        app.activeSide = side
        pane.click(item, flags: NSEvent.modifierFlags)
    }

    private func doubleClick() {
        guard !item.isEnterable else { return }
        if item.isZip {
            app.extractZips([item.url])          // çift tık: ZIP'i klasöre çıkar
        } else {
            NSWorkspace.shared.open(item.url)
        }
    }
}

// Orta sütun: eylem düğmeleri

struct CellButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(configuration.isPressed ? Color.white.opacity(0.12) : Color.clear)
    }
}

// MARK: - Özel simgeler

private let glyphBlue = LinearGradient(
    colors: [Color(red: 0.45, green: 0.88, blue: 1.0), Color(red: 0.12, green: 0.45, blue: 1.0)],
    startPoint: .topLeading, endPoint: .bottomTrailing)

/// Sıralama: üç yuvarlak mavi çubuk + sağda mavi aşağı ok
struct SortGlyph: View {
    private let widths: [CGFloat] = [24, 22, 26]

    var body: some View {
        HStack(alignment: .center, spacing: 6) {
            VStack(alignment: .leading, spacing: 5) {
                ForEach(0..<widths.count, id: \.self) { i in
                    Capsule()
                        .fill(glyphBlue)
                        .frame(width: widths[i], height: 5.5)
                }
            }
            SortArrow()
                .stroke(glyphBlue, style: StrokeStyle(lineWidth: 4.5, lineCap: .round, lineJoin: .round))
                .frame(width: 16, height: 30)
        }
        .shadow(color: Color(red: 0.2, green: 0.6, blue: 1.0).opacity(0.45), radius: 3)
    }
}

struct SortArrow: Shape {
    func path(in rect: CGRect) -> Path {
        var p = Path()
        p.move(to: CGPoint(x: rect.midX, y: rect.minY + 2))
        p.addLine(to: CGPoint(x: rect.midX, y: rect.maxY - 3))
        p.move(to: CGPoint(x: rect.minX + 1.5, y: rect.maxY - 10.5))
        p.addLine(to: CGPoint(x: rect.midX, y: rect.maxY - 3))
        p.addLine(to: CGPoint(x: rect.maxX - 1.5, y: rect.maxY - 10.5))
        return p
    }
}

/// Yeniden adlandır: iki metin çizgisi, alt çizgi ve büyük mavi kalem
struct RenameGlyph: View {
    var body: some View {
        ZStack {
            Capsule().fill(glyphBlue).frame(width: 21, height: 5)
                .offset(x: -11, y: -9)
            Capsule().fill(glyphBlue).frame(width: 15, height: 5)
                .offset(x: -14, y: -1)
            Capsule()
                .fill(LinearGradient(colors: [Color(red: 0.45, green: 0.88, blue: 1.0), Color(red: 0.15, green: 0.5, blue: 1.0)],
                                     startPoint: .leading, endPoint: .trailing))
                .frame(width: 17, height: 3)
                .offset(x: -12, y: 15)
            PencilGlyph()
                .rotationEffect(.degrees(45))
                .offset(x: 6, y: -1)
        }
        .frame(width: 46, height: 38)
        .shadow(color: Color(red: 0.2, green: 0.6, blue: 1.0).opacity(0.45), radius: 3)
    }
}

struct PencilGlyph: View {
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 3, style: .continuous)      // silgi başlığı
                .fill(LinearGradient(colors: [Color(red: 0.6, green: 0.92, blue: 1.0), Color(red: 0.25, green: 0.6, blue: 1.0)],
                                     startPoint: .top, endPoint: .bottom))
                .frame(width: 11, height: 7)
                .offset(y: -14)
            Rectangle()                                                // ayırıcı bant
                .fill(Color(red: 0.03, green: 0.07, blue: 0.16))
                .frame(width: 11, height: 1.6)
                .offset(y: -10.2)
            HStack(spacing: 0) {                                       // gövde: açık / koyu yarı
                Rectangle().fill(Color(red: 0.55, green: 0.9, blue: 1.0))
                Rectangle().fill(Color(red: 0.12, green: 0.42, blue: 0.95))
            }
            .frame(width: 11, height: 16)
            .offset(y: -1.5)
            DownTriangle()                                             // uç
                .fill(Color(red: 0.8, green: 0.95, blue: 1.0))
                .frame(width: 11, height: 9)
                .offset(y: 11)
            DownTriangle()                                             // kurşun
                .fill(Color(red: 0.03, green: 0.07, blue: 0.16))
                .frame(width: 4.5, height: 4)
                .offset(y: 13.5)
        }
    }
}

struct DownTriangle: Shape {
    func path(in rect: CGRect) -> Path {
        var p = Path()
        p.move(to: CGPoint(x: rect.minX, y: rect.minY))
        p.addLine(to: CGPoint(x: rect.maxX, y: rect.minY))
        p.addLine(to: CGPoint(x: rect.midX, y: rect.maxY))
        p.closeSubpath()
        return p
    }
}

struct ActionCell: View {
    let kind: ActionButton
    let title: String
    let symbol: String
    let color: Color

    var body: some View {
        VStack(spacing: 6) {
            Group {
                switch kind {
                case .sort:
                    SortGlyph()
                case .rename:
                    RenameGlyph()
                default:
                    Image(systemName: symbol)
                        .font(.system(size: 30))
                        .symbolRenderingMode(.hierarchical)
                        .foregroundStyle(color)
                }
            }
            .frame(height: 38)
            Text(title)
                .font(.system(size: 13))
                .multilineTextAlignment(.center)
                .lineLimit(2)
                .foregroundStyle(.white.opacity(0.92))
        }
        .frame(maxWidth: .infinity, minHeight: 88)
        .contentShape(Rectangle())
        .overlay(Rectangle().stroke(Color.white.opacity(0.05), lineWidth: 0.5))
    }
}

// MARK: - Süzgeç çubuğu (Resim / Belge / Arşiv + arama)

/// Resim: çerçeve, güneş ve dağ
struct ImageGlyph: View {
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(LinearGradient(colors: [Color(red: 0.55, green: 0.85, blue: 0.5), Color(red: 0.1, green: 0.6, blue: 0.4)],
                                     startPoint: .topLeading, endPoint: .bottomTrailing))
            Circle().fill(Color.yellow).frame(width: 7, height: 7).offset(x: 8, y: -7)
            MountainShape()
                .fill(Color.white.opacity(0.92))
                .frame(width: 34, height: 15)
                .offset(y: 6)
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
        .frame(width: 34, height: 28)
        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
    }
}

struct MountainShape: Shape {
    func path(in r: CGRect) -> Path {
        var p = Path()
        p.move(to: CGPoint(x: r.minX, y: r.maxY))
        p.addLine(to: CGPoint(x: r.width * 0.30, y: r.minY + 2))
        p.addLine(to: CGPoint(x: r.width * 0.55, y: r.height * 0.6))
        p.addLine(to: CGPoint(x: r.width * 0.72, y: r.height * 0.28))
        p.addLine(to: CGPoint(x: r.maxX, y: r.maxY))
        p.closeSubpath()
        return p
    }
}

/// Belge: kıvrık köşeli sayfa ve metin çizgileri
struct DocumentGlyph: View {
    var body: some View {
        ZStack {
            DocShape()
                .fill(LinearGradient(colors: [Color(red: 0.45, green: 0.88, blue: 1.0), Color(red: 0.15, green: 0.5, blue: 1.0)],
                                     startPoint: .topLeading, endPoint: .bottomTrailing))
            VStack(alignment: .leading, spacing: 3.5) {
                Capsule().fill(.white.opacity(0.9)).frame(width: 16, height: 2.5)
                Capsule().fill(.white.opacity(0.9)).frame(width: 16, height: 2.5)
                Capsule().fill(.white.opacity(0.9)).frame(width: 10, height: 2.5)
            }
            .offset(y: 5)
        }
        .frame(width: 26, height: 32)
    }
}

struct DocShape: Shape {
    func path(in r: CGRect) -> Path {
        let c: CGFloat = 9
        var p = Path()
        p.move(to: CGPoint(x: r.minX + 4, y: r.minY))
        p.addLine(to: CGPoint(x: r.maxX - c, y: r.minY))
        p.addLine(to: CGPoint(x: r.maxX, y: r.minY + c))
        p.addLine(to: CGPoint(x: r.maxX, y: r.maxY - 4))
        p.addQuadCurve(to: CGPoint(x: r.maxX - 4, y: r.maxY), control: CGPoint(x: r.maxX, y: r.maxY))
        p.addLine(to: CGPoint(x: r.minX + 4, y: r.maxY))
        p.addQuadCurve(to: CGPoint(x: r.minX, y: r.maxY - 4), control: CGPoint(x: r.minX, y: r.maxY))
        p.addLine(to: CGPoint(x: r.minX, y: r.minY + 4))
        p.addQuadCurve(to: CGPoint(x: r.minX + 4, y: r.minY), control: CGPoint(x: r.minX, y: r.minY))
        p.closeSubpath()
        return p
    }
}

/// Arşiv: kapaklı kutu ve tutamak
struct ArchiveGlyph: View {
    var body: some View {
        let g = LinearGradient(colors: [Color(red: 1.0, green: 0.78, blue: 0.3), Color(red: 1.0, green: 0.5, blue: 0.1)],
                               startPoint: .top, endPoint: .bottom)
        ZStack {
            RoundedRectangle(cornerRadius: 3, style: .continuous)        // kapak
                .fill(g).frame(width: 34, height: 9).offset(y: -9.5)
            RoundedRectangle(cornerRadius: 3, style: .continuous)        // gövde
                .fill(g).frame(width: 30, height: 20).offset(y: 5)
            Capsule().fill(Color(red: 0.35, green: 0.18, blue: 0.02))    // tutamak
                .frame(width: 11, height: 4).offset(y: 1)
        }
        .frame(width: 34, height: 32)
    }
}

struct FilterBar: View {
    @Environment(AppModel.self) var app
    @FocusState private var searchFocused: Bool

    var body: some View {
        let pane = app.active
        VStack(spacing: 0) {
            Rectangle().fill(Color.white.opacity(0.08)).frame(height: 1)
            HStack(spacing: 0) {
                ForEach(FileFilter.allCases) { f in
                    let on = pane.typeFilter == f
                    Button {
                        pane.typeFilter = on ? nil : f
                    } label: {
                        VStack(spacing: 6) {
                            Group {
                                switch f {
                                case .image: ImageGlyph()
                                case .document: DocumentGlyph()
                                case .archive: ArchiveGlyph()
                                }
                            }
                            .frame(height: 34)
                            Text(f.title)
                                .font(.system(size: 13, weight: on ? .semibold : .regular))
                                .foregroundStyle(.white.opacity(on ? 1 : 0.92))
                        }
                        .frame(maxWidth: .infinity, minHeight: 78)
                        .background(on ? Theme.accent.opacity(0.28) : Color.clear)
                        .overlay(alignment: .bottom) {
                            Rectangle().fill(Theme.accent).frame(height: on ? 2 : 0)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(CellButtonStyle())
                    .overlay(Rectangle().stroke(Color.white.opacity(0.05), lineWidth: 0.5))
                }
            }
            // Arama kutusu: sütun genişliğine tam oturur
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 13))
                    .foregroundStyle(.white.opacity(0.5))
                TextField("Ara", text: Binding(
                    get: { app.active.query },
                    set: { app.active.query = $0 }
                ))
                .textFieldStyle(.plain)
                .font(.system(size: 14))
                .focused($searchFocused)
                .onExitCommand { app.active.query = ""; searchFocused = false }
                if !pane.query.isEmpty {
                    Button { app.active.query = "" } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 13))
                            .foregroundStyle(.white.opacity(0.5))
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 10)
            .frame(height: 32)
            .frame(maxWidth: .infinity)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.black.opacity(0.35)))
            .overlay(RoundedRectangle(cornerRadius: 8)
                .stroke(searchFocused ? Theme.accent.opacity(0.8) : Color.white.opacity(0.1), lineWidth: 1))
            .padding(.horizontal, 6)
            .padding(.vertical, 8)
        }
        .background(Theme.bar.opacity(0.8))
    }
}

struct CenterColumn: View {
    @Environment(AppModel.self) var app
    @State private var showSort = false

    private var visible: [ActionButton] {
        ActionButton.allCases.filter { $0 == .customize || !app.hiddenButtons.contains($0.rawValue) }
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVGrid(columns: [GridItem(.flexible(), spacing: 0), GridItem(.flexible(), spacing: 0)], spacing: 0) {
                    ForEach(visible) { b in cell(b) }
                }
            }
            FilterBar()
        }
        .background(Theme.bar.opacity(0.55))
    }

    private func display(_ b: ActionButton) -> (title: String, symbol: String, color: Color) {
        switch b {
        case .hidden:
            return (app.showHidden ? "Gizliyi gizle" : "Gizliyi göster", app.showHidden ? "eye" : "eye.slash", b.color)
        default:
            return (b.title, b.symbol, b.color)
        }
    }

    @ViewBuilder
    private func cell(_ b: ActionButton) -> some View {
        let d = display(b)
        if b == .sort {
            Button { showSort.toggle() } label: {
                ActionCell(kind: b, title: d.title, symbol: d.symbol, color: d.color)
            }
            .buttonStyle(CellButtonStyle())
            .popover(isPresented: $showSort, arrowEdge: .trailing) {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Sıralama ölçütü").font(.headline)
                    ForEach(SortKey.allCases) { k in
                        Button {
                            app.setSort(k)
                        } label: {
                            HStack {
                                Image(systemName: app.sortKey == k ? "checkmark" : "").frame(width: 16)
                                Text(k.title)
                                Spacer()
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                    Divider()
                    Button {
                        app.setAscending(true)
                    } label: {
                        HStack {
                            Image(systemName: app.ascending ? "checkmark" : "").frame(width: 16)
                            Text("Artan"); Spacer()
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    Button {
                        app.setAscending(false)
                    } label: {
                        HStack {
                            Image(systemName: app.ascending ? "" : "checkmark").frame(width: 16)
                            Text("Azalan (en yeni üstte)"); Spacer()
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                .padding(16)
                .frame(width: 230)
            }
        } else {
            Button { perform(b) } label: {
                ActionCell(kind: b, title: d.title, symbol: d.symbol, color: d.color)
            }
            .buttonStyle(CellButtonStyle())
        }
    }

    private func perform(_ b: ActionButton) {
        switch b {
        case .newFolder: app.beginNewFolder()
        case .customize: app.showCustomize = true
        case .sort: break
        case .hidden: app.toggleHidden()
        case .copy: app.transfer(move: false)
        case .move: app.transfer(move: true)
        case .duplicate: app.duplicateSelected()
        case .rename: app.beginRename()
        case .unzip: app.extractSelectedZips()
        case .zip: app.zipSelected()
        case .trash: app.trashSelected()
        case .share: app.shareSelected()
        case .info: app.showInfo()
        case .bookmark: app.toggleBookmark()
        }
    }
}

struct CustomizeSheet: View {
    @Environment(AppModel.self) var app
    @Environment(\.dismiss) var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Düğmeleri ayarla").font(.title2.bold())
            Text("Ortadaki sütunda görünmesini istediğiniz düğmeleri seçin.")
                .foregroundStyle(.secondary)
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(ActionButton.allCases.filter { $0 != .customize }) { b in
                        Toggle(isOn: Binding(
                            get: { !app.hiddenButtons.contains(b.rawValue) },
                            set: { app.setButton(b, visible: $0) }
                        )) {
                            Label(b.title, systemImage: b.symbol)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack {
                Button("Yer imlerini temizle") { app.resetBookmarks() }
                Spacer()
                Button("Bitti") { dismiss() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(22)
        .frame(width: 400, height: 540)
    }
}

// MARK: - Menü komutları

struct AppCommands: Commands {
    let model: AppModel

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("Yeni Klasör") { model.beginNewFolder() }
                .keyboardShortcut("n", modifiers: [.command, .shift])
        }
        CommandMenu("Git") {
            Button("Geri") { model.active.goBack() }
                .keyboardShortcut("[", modifiers: .command)
            Button("Üst Klasör") { model.active.goUp() }
                .keyboardShortcut(.upArrow, modifiers: .command)
            Button("Yenile") { model.reloadAll() }
                .keyboardShortcut("r", modifiers: .command)
        }
        CommandMenu("Dosya İşlemleri") {
            Button("Diğer Panele Kopyala") { model.transfer(move: false) }
                .keyboardShortcut("c", modifiers: [.command, .option])
            Button("Diğer Panele Taşı") { model.transfer(move: true) }
                .keyboardShortcut("m", modifiers: [.command, .option])
            Button("Çöp Kutusuna Taşı") { model.trashSelected() }
                .keyboardShortcut(.delete, modifiers: .command)
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

// MARK: - Uygulama

@main
struct DeckApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    @State private var model = AppModel()

    init() {
        // Sistem menüleri ve diyaloglar da Türkçe olsun (bir sonraki açılıştan itibaren geçerli)
        UserDefaults.standard.set(["tr"], forKey: "AppleLanguages")
    }

    var body: some Scene {
        Window("Deck", id: "main") {
            ContentView()
                .environment(model)
                .frame(minWidth: 960, minHeight: 560)
        }
        .defaultSize(width: 1180, height: 740)
        .commands { AppCommands(model: model) }
    }
}
