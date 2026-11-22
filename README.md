# Deck

macOS (Apple Silicon) için koyu temalı, **Türkçe ve İngilizce** resim görüntüleyici. **Rust** ile yazıldı
([egui/eframe](https://github.com/emilk/egui) arayüzü + [image](https://github.com/image-rs/image) kütüphanesi).

<p align="center"><img src="Deck-icon-1024.png" width="160" alt="Deck simgesi"></p>

## Depo yapısı

```
.
├── .github/workflows/Deck-build.yml   # GitHub Actions derleme iş akışı
├── Cargo.toml                         # Rust proje tanımı ve bağımlılıklar
├── src/
│   ├── main.rs                        # Giriş noktası, pencere ayarları
│   ├── app.rs                         # Arayüz: araç çubuğu, resim alanı, kırpma, kalem, şerit, kaydırıcılar
│   ├── macos_open.rs                  # Finder'dan çift tık / Birlikte aç (Apple Event) desteği
│   ├── thumbs.rs                      # Küçük resimler: hızlı JPEG çözme, disk önbelleği, temizlik
│   └── gallery.rs                     # Klasör tarama, resim çözme, önbellek, arka plan işçileri, kaydetme
├── Deck.icns                          # Uygulama simgesi
├── Deck-icon-1024.png                 # macOS ızgarasına oturtulmuş 1024 px simge (derlemeye gömülür)
├── Deck-Info.plist                    # Uygulama bilgileri, izin metinleri
├── Deck-lproj/                        # tr / en izin metinleri (InfoPlist.strings)
├── src/lang.rs                        # Arayüz dili (Türkçe / English): seçim, kayıt, sistem dili
├── Deck-build.sh                      # Yerelde ve CI'da kullanılan derleme betiği (.app üretir)
├── .gitignore
└── README.md
```

## GitHub'da derleme

1. GitHub'da yeni bir depo açın.
2. Bu klasördeki **tüm dosyaları** (gizli `.github` klasörü dahil) depoya yükleyin.
   Web arayüzü dot-klasörlerini atlarsa *Add file → Create new file* deyip dosya adına
   `.github/workflows/Deck-build.yml` yazın ve içeriği yapıştırın.
3. **Actions** sekmesinde derleme kendiliğinden başlar (ilk seferde Rust bağımlılıkları indirileceği için 5–10 dk sürebilir).
4. Bitince çalıştırmanın sayfasında **Artifacts** bölümünden `Deck-macOS-arm64` dosyasını indirin (içinde `.zip` ve `.dmg` var).

Elle başlatmak için: **Actions → Derle (macOS Apple Silicon) → Run workflow**.
Kalıcı indirme bağlantısı için `git tag v2.0 && git push origin v2.0` (zip ve dmg **Releases** sayfasına eklenir).

> İlk derlemeden sonra oluşan `Cargo.lock` dosyasını depoya eklemeniz önerilir; bağımlılık sürümleri sabitlenir.

## Yerelde derleme

```bash
curl https://sh.rustup.rs -sSf | sh     # Rust yoksa
chmod +x Deck-build.sh
./Deck-build.sh
```

Yalnızca çalıştırmak için: `cargo run --release -- /yol/resim.jpg`

## İndirilen uygulamayı açma

Uygulama Apple geliştirici hesabıyla imzalı değil (ad-hoc imza). macOS "hasarlı" ya da "doğrulanamadı" derse:

```bash
xattr -cr /Applications/Deck.app
```

ya da uygulamaya **sağ tıklayıp → Aç** deyin.

## Kullanım

- **Dil / Language:** ilk açılışta sistem dili Türkçe ise arayüz Türkçe, değilse English açılır. Üst çubuktaki **EN / TR** düğmesi dili anında değiştirir; seçim bir sonraki açılışta hatırlanır.
- **Gezinme sınırı:** klasör gezgini yalnızca **ana dizin (ev klasörü)** ve altında çalışır; yol çubuğu ana dizinden başlar, ana dizinin üstüne çıkılamaz.
- **Klasör gezgini (açılış ekranı):** uygulama **Pictures** klasöründe açılır. Üstteki çubukta **Geri / İleri / Yukarı / Pictures**, tıklanabilir yol ve **Klasör seç…** vardır; içerik her satırda **altı ön izleme** olacak şekilde dizilir. Klasöre tıklayınca içine girilir, **bir resme tıklayınca** görüntüleyici açılır. Görüntüleyiciden gezgine dönmek için araç çubuğundaki **Klasörler**, **Esc** ya da **⌘B**; gezginde **Backspace** üst klasöre çıkar. Bir klasörü pencereye sürükleyip bırakmak da gezgini oraya götürür.
- **Tara:** gezgin çubuğundaki **Tara**, bulunduğunuz klasörden bağımsız olarak **ana dizini (ev klasörü)** ve tüm alt dizinlerini tarar; Sistem klasörleri (`Applications`, `Library`, `/System`, `/Library`, `/usr`, `/private`, `/Volumes` vb.) ve `.app`, `.framework`, `.bundle` gibi paketler taranmaz. Sonuçlar ana ekranda gösterilir: resim içeren klasörler (ön izleme + resim sayısı, her satırda altı kart). Karta tıklayınca klasör açılır ve tarama sırasında yalnızca resimler listelenir (alt klasörler gösterilmez); üstteki satırdaki **Liste**, **Önceki klasör / Sonraki klasör** düğmeleriyle sıralı klasörler arasında gezilir. Tarama sürerken düğme **Durdur** olur.
- **Alt şerit (görüntüleyici):** küçük resim şeridi pencerenin tam genişliğini kaplar. Büyütme ve serbest döndürme kaydırıcıları üst çubukta, **Aç…** düğmesinin yanında sağa sabittir (üst çubuk 58 px).
- **Resim açma:** **⌘O** ya da araç çubuğundaki **Aç…**, resmi pencereye **sürükleyip bırakma**, ya da komut satırından `Deck resim.jpg`.
- Açılan resimle aynı klasördeki tüm resimler alttaki şeritte (ada göre, sayıya duyarlı sıralı) listelenir; küçük resme tıklayarak geçiş yapılır.
- **Klavye:** **← →** önceki/sonraki, **↑** sola döndür, **↓** sağa döndür, **⌘Z** geri al, **⌘S** kopya olarak kaydet, **⌘O** aç, **Esc** kırpmayı/kalemi kapatır (tam ekrandan çıkar).
- **Araç çubuğu:** gezinme, döndürme (90°), çevirme, kırpma, kırmızı kalem, geri al, tam ekran, slayt gösterisi (3 sn), kaydet, Çöp Kutusu'na at, Finder'da göster, aç.
- **Kaydırıcılar (alt sağ):** üstteki **büyütme/küçültme** (sağa çek → büyür, sola çek → küçülür; %1–%10000), alttaki **serbest döndürme** (sola çek → sola, sağa çek → sağa; −180°…+180°). Sayıya tıklamak sıfırlar. Büyütülmüş resim sürükleyerek kaydırılır; dokunmatik yüzeyde sıkıştırma da çalışır. Bu ikisi yalnızca görünümü değiştirir, kaydedilen kopyaya yansımaz.
- **Kırmızı kalem:** resmin üzerine sürükleyerek çizilir; çizgiler doğrudan resme işlenir. **Geri al** son 10 işlemi (çizgi, döndürme, çevirme) geri alır.
- **Kaydet:** sormadan, kırpma alanı, çizgiler ve döndürme/çevirmeyle birlikte resmi **“ad kopya.uzantı”** adıyla aynı klasöre kaydeder; özgün dosyaya dokunmaz. Kaydedilen biçim JPEG/PNG/TIFF/BMP/GIF'tir, diğer biçimler PNG olarak yazılır.
- **Hız:** resim klasör taranmadan hemen açılır; komşu resimler arka planda önceden çözülür; küçük resimler yalnızca görünür olunca üretilir; slayt gösterisi kapalıyken zamanlayıcı çalışmaz.

## Swift sürümüne göre farklar

| | Swift sürümü | Rust sürümü |
|---|---|---|
| Finder'da **Birlikte Aç / varsayılan uygulama** | Var | **Yok** (egui bu Apple olayını almıyor); bunun yerine ⌘O, sürükle-bırak, komut satırı |
| Desteklenen biçimler | macOS'un çözdüğü her şey (HEIC, AVIF, RAW, PSD, SVG…) | JPEG, PNG, GIF, WebP, TIFF, BMP, ICO, TGA, QOI, PNM, HDR, EXR, DDS |
| Paylaş düğmesi | Var | Yok (yerine **Finder'da göster**) |
| Geri al | Yok | Var |
| Asgari macOS | 15 | 11 |

## Küçük resim önbelleği

Küçük resimler `~/Library/Caches/Deck/thumbs` klasöründe saklanır (dosya yolu, boyutu ve değişiklik zamanına göre; dosya değişirse yeniden üretilir).
30 günden uzun süredir kullanılmayanlar ve 400 MB'ı aşan kısım (en eskiden başlayarak) uygulama açıldıktan kısa süre sonra otomatik silinir.
Elle temizlemek için klasörü silmeniz yeterli: `rm -rf ~/Library/Caches/Deck/thumbs`
