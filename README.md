# Deck

macOS 15 Sequoia ve üzeri (Apple Silicon) için koyu temalı, tamamen Türkçe resim görüntüleyici. SwiftUI ile yazıldı.

<p align="center"><img src="Deck-icon-1024.png" width="160" alt="Deck simgesi"></p>

## Depo yapısı

```
.
├── .github/workflows/Deck-build.yml   # GitHub Actions derleme iş akışı
├── Deck.swift                         # Uygulamanın tüm kodu (resim görüntüleyici)
├── Deck.icns                          # Uygulama simgesi
├── Deck-icon-source.png               # Simgenin kaynak tasarımı (balıkçı teknesi)
├── Deck-icon-1024.png                 # macOS ızgarasına oturtulmuş 1024 px simge
├── Deck-Info.plist                    # Uygulama bilgileri, Türkçe yerelleştirme, izin metinleri
├── Deck-build.sh                      # Yerelde ve CI'da kullanılan derleme betiği
├── Deck-tools/Deck-make-icon.py       # Kaynak görselden .icns üreten betik (isteğe bağlı)
├── .gitignore                         # (GitHub bu adı şart koşar)
└── README.md                          # (GitHub bu adı şart koşar)
```

## Simgeyi yeniden üretme (isteğe bağlı)

Simgeyi değiştirmek isterseniz `Deck-icon-source.png` dosyasını yenisiyle değiştirip şunu çalıştırın
(Python 3 ve Pillow gerekir: `pip3 install pillow`):

```bash
cd Deck-tools
python3 Deck-make-icon.py ../Deck-icon-source.png ../Deck-icon-1024.png ../Deck.icns
```

## GitHub'da derleme

1. GitHub'da yeni bir depo açın.
2. Bu klasördeki **tüm dosyaları** (gizli `.github` klasörü dahil) depoya yükleyin.
   Web arayüzü dot-klasörlerini atlarsa *Add file → Create new file* deyip dosya adına
   `.github/workflows/Deck-build.yml` yazın ve içeriği yapıştırın. Git ile:
   ```bash
   git init
   git add .
   git commit -m "İlk sürüm"
   git branch -M main
   git remote add origin https://github.com/KULLANICI/DEPO.git
   git push -u origin main
   ```
3. **Actions** sekmesinde derleme kendiliğinden başlar (yaklaşık 2–4 dk).
4. Bitince çalıştırmanın sayfasında **Artifacts** bölümünden `Deck-macOS-arm64` dosyasını indirin (içinde `.zip` ve `.dmg` var).

Elle başlatmak için: **Actions → Derle (macOS Apple Silicon) → Run workflow**.

## Release (kalıcı indirme bağlantısı)

```bash
git tag v1.0
git push origin v1.0
```

Zip ve dmg **Releases** sayfasına otomatik eklenir.

## İndirilen uygulamayı açma

Uygulama Apple geliştirici hesabıyla imzalı değil (ad-hoc imza). macOS "hasarlı" ya da "doğrulanamadı" derse:

```bash
xattr -cr /Applications/Deck.app
```

ya da uygulamaya **sağ tıklayıp → Aç** deyin.

## Yerelde derleme

```bash
chmod +x Deck-build.sh
./Deck-build.sh
```

## Kullanım notları

- Resim açmanın yolları: Finder'da resme sağ tıklayıp **Birlikte Aç → Deck**, uygulamayı varsayılan resim görüntüleyici yapmak, **⌘O** ya da araç çubuğundaki klasör düğmesi.
- Açılan resimle aynı klasördeki tüm resimler alttaki şeritte listelenir (ada göre sıralı); şeritten bir küçük resme tıklayarak geçiş yapılır.
- Klavye: **← →** önceki/sonraki resim, **↑** sola döndür, **↓** sağa döndür, **Enter** kırpmayı kopya olarak kaydet, **Esc** kırpmayı iptal.
- Araç çubuğu: gezinme, döndürme, çevirme, kırpma, tam ekran, slayt gösterisi (3 sn), Çöp Kutusu'na at, paylaş, başka resim aç.
- Kırpma, resmi **“ad kopya.uzantı”** adıyla aynı klasöre kaydeder; özgün dosyaya dokunmaz.
- **Desteklenen biçimler:** macOS'un (ImageIO) çözebildiği tüm resim biçimleri: JPEG, PNG, HEIC/HEIF, AVIF, WebP, TIFF, GIF, BMP, JPEG 2000, PSD, ICO/ICNS, RAW (DNG, CR2/CR3, NEF, ARW, ORF, RAF, RW2 …) ve SVG. İşletim sistemi yeni bir biçim eklerse Deck onu da açar. Animasyonlu GIF'te ilk kare gösterilir.
- **Hız:** İlk resim klasör taranmadan hemen açılır; komşu resimler arka planda önceden çözülür (oklarla gezinme anında olur); küçük resimler dosyadaki gömülü önizlemeden üretilir; slayt gösterisi kapalıyken zamanlayıcı çalışmaz.
