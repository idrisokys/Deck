//! Video oynatma (macOS: AVFoundation). Ses dahil; kareler `AVPlayerItemVideoOutput` ile BGRA olarak alınıp
//! egui dokusuna çevrilir. Hız `AVPlayer.rate` ile değişir (basılı tutunca 2.5×).
//! macOS dışında derlenebilsin diye boş bir gerçekleme vardır (video açılmaz).

#[cfg(target_os = "macos")]
pub use imp::{thumbnail, VideoPlayer};
#[cfg(not(target_os = "macos"))]
pub use stub::{thumbnail, VideoPlayer};

/// "m:ss" ya da "h:mm:ss"
pub fn fmt_time(secs: f64) -> String {
    let s = if secs.is_finite() && secs > 0.0 { secs as u64 } else { 0 };
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{c_void, CString};
    use std::path::Path;
    use std::time::Instant;

    use eframe::egui;
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{msg_send, msg_send_id, Encode, Encoding, RefEncode};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CMTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    unsafe impl Encode for CMTime {
        const ENCODING: Encoding = Encoding::Struct("?", &[Encoding::LongLong, Encoding::Int, Encoding::UInt, Encoding::LongLong]);
    }
    unsafe impl RefEncode for CMTime {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {
        fn CMTimeMakeWithSeconds(seconds: f64, preferred_timescale: i32) -> CMTime;
        fn CMTimeGetSeconds(t: CMTime) -> f64;
    }

    #[link(name = "CoreVideo", kind = "framework")]
    extern "C" {
        static kCVPixelBufferPixelFormatTypeKey: *const AnyObject;
        fn CVPixelBufferLockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferUnlockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferGetBaseAddress(pb: *mut c_void) -> *const u8;
        fn CVPixelBufferGetBytesPerRow(pb: *mut c_void) -> usize;
        fn CVPixelBufferGetWidth(pb: *mut c_void) -> usize;
        fn CVPixelBufferGetHeight(pb: *mut c_void) -> usize;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGImageGetWidth(img: *mut c_void) -> usize;
        fn CGImageGetHeight(img: *mut c_void) -> usize;
        fn CGImageRelease(img: *mut c_void);
        fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
        fn CGColorSpaceRelease(cs: *mut c_void);
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: *mut c_void,
            bitmap_info: u32,
        ) -> *mut c_void;
        fn CGContextDrawImage(ctx: *mut c_void, rect: CGRect, img: *mut c_void);
        fn CGContextRelease(ctx: *mut c_void);
    }

    /// kCVPixelFormatType_32BGRA ('BGRA')
    const PIXEL_FORMAT_BGRA: u32 = 0x4247_5241;
    /// Çok büyük videolarda doku boyutunu sınırlamak için en geniş kenar (piksel)
    const MAX_SIDE: usize = 1920;

    pub struct VideoPlayer {
        player: Retained<AnyObject>,
        item: Retained<AnyObject>,
        output: Retained<AnyObject>,
        /// Arayüz yeni kare bekliyor (arama / ilk açılış sonrası duraklatılmışken de kare alınsın)
        pending: bool,
        pending_since: Instant,
    }

    fn class(name: &str) -> Option<&'static AnyClass> {
        AnyClass::get(name)
    }

    fn ns_string(s: &str) -> Option<Retained<AnyObject>> {
        let c = CString::new(s).ok()?;
        let cls = class("NSString")?;
        unsafe { msg_send_id![cls, stringWithUTF8String: c.as_ptr()] }
    }

    /// Videonun küçük resmi: ~1. saniyedeki kare (döndürme bilgisi uygulanır), en uzun kenarı `max_side` piksel.
    /// Arka plan iş parçacığından çağrılır.
    pub fn thumbnail(path: &Path, max_side: u32) -> Option<image::RgbaImage> {
        objc2::rc::autoreleasepool(|_| unsafe {
            let ns_path = ns_string(&path.to_string_lossy())?;
            let url: Retained<AnyObject> = msg_send_id![class("NSURL")?, fileURLWithPath: &*ns_path];
            let asset: Retained<AnyObject> = msg_send_id![class("AVAsset")?, assetWithURL: &*url];
            let gen: Retained<AnyObject> = msg_send_id![class("AVAssetImageGenerator")?, assetImageGeneratorWithAsset: &*asset];
            let _: () = msg_send![&*gen, setAppliesPreferredTrackTransform: Bool::YES];

            let dur: CMTime = msg_send![&*asset, duration];
            let d = CMTimeGetSeconds(dur);
            let at = if d.is_finite() && d > 0.0 { (d / 2.0).min(1.0) } else { 0.0 };
            let t = CMTimeMakeWithSeconds(at, 600);

            let mut err: *mut AnyObject = std::ptr::null_mut();
            let img: *mut c_void = msg_send![
                &*gen,
                copyCGImageAtTime: t,
                actualTime: std::ptr::null_mut::<CMTime>(),
                error: &mut err as *mut *mut AnyObject
            ];
            if img.is_null() {
                return None;
            }

            let (w, h) = (CGImageGetWidth(img), CGImageGetHeight(img));
            let mut result: Option<image::RgbaImage> = None;
            if w > 0 && h > 0 {
                let k = (max_side as f64 / w.max(h) as f64).min(1.0);
                let tw = ((w as f64 * k) as usize).max(1);
                let th = ((h as f64 * k) as usize).max(1);
                let mut buf = vec![0u8; tw * th * 4];
                let cs = CGColorSpaceCreateDeviceRGB();
                // kCGImageAlphaPremultipliedLast (1): bellekte R G B A
                let cx = CGBitmapContextCreate(buf.as_mut_ptr() as *mut c_void, tw, th, 8, tw * 4, cs, 1);
                if !cx.is_null() {
                    CGContextDrawImage(cx, CGRect { x: 0.0, y: 0.0, w: tw as f64, h: th as f64 }, img);
                    CGContextRelease(cx);
                    result = image::RgbaImage::from_raw(tw as u32, th as u32, buf);
                }
                CGColorSpaceRelease(cs);
            }
            CGImageRelease(img);
            result
        })
    }

    impl VideoPlayer {
        pub fn open(path: &Path) -> Option<VideoPlayer> {
            unsafe {
                let ns_path = ns_string(&path.to_string_lossy())?;
                let url: Retained<AnyObject> = msg_send_id![class("NSURL")?, fileURLWithPath: &*ns_path];
                let item: Retained<AnyObject> = msg_send_id![class("AVPlayerItem")?, playerItemWithURL: &*url];

                // Kare çıktısı: BGRA
                let fmt: Retained<AnyObject> = msg_send_id![class("NSNumber")?, numberWithUnsignedInt: PIXEL_FORMAT_BGRA];
                let attrs: Retained<AnyObject> = msg_send_id![
                    class("NSDictionary")?,
                    dictionaryWithObject: &*fmt,
                    forKey: kCVPixelBufferPixelFormatTypeKey
                ];
                let alloc: Allocated<AnyObject> = msg_send_id![class("AVPlayerItemVideoOutput")?, alloc];
                let output: Retained<AnyObject> = msg_send_id![alloc, initWithPixelBufferAttributes: &*attrs];
                let _: () = msg_send![&*item, addOutput: &*output];

                let player: Retained<AnyObject> = msg_send_id![class("AVPlayer")?, playerWithPlayerItem: &*item];
                Some(VideoPlayer { player, item, output, pending: true, pending_since: Instant::now() })
            }
        }

        /// Verilen hızla oynatır (1.0 normal, 2.5 hızlı ileri sarma).
        pub fn set_rate(&self, rate: f32) {
            unsafe {
                let _: () = msg_send![&*self.player, setRate: rate];
            }
        }

        pub fn pause(&self) {
            unsafe {
                let _: () = msg_send![&*self.player, pause];
            }
        }

        pub fn rate(&self) -> f32 {
            unsafe { msg_send![&*self.player, rate] }
        }

        pub fn is_playing(&self) -> bool {
            self.rate().abs() > 0.01
        }

        pub fn current(&self) -> f64 {
            let t: CMTime = unsafe { msg_send![&*self.player, currentTime] };
            let s = unsafe { CMTimeGetSeconds(t) };
            if s.is_finite() {
                s.max(0.0)
            } else {
                0.0
            }
        }

        /// Süre (saniye); yüklenene kadar 0.
        pub fn duration(&self) -> f64 {
            let t: CMTime = unsafe { msg_send![&*self.item, duration] };
            let s = unsafe { CMTimeGetSeconds(t) };
            if s.is_finite() {
                s.max(0.0)
            } else {
                0.0
            }
        }

        pub fn seek(&mut self, secs: f64) {
            let secs = secs.max(0.0);
            unsafe {
                let t = CMTimeMakeWithSeconds(secs, 600);
                let _: () = msg_send![&*self.player, seekToTime: t];
            }
            self.pending = true;
            self.pending_since = Instant::now();
        }

        /// Video bitti mi (durmuş ve sona gelmiş)?
        pub fn ended(&self) -> bool {
            let d = self.duration();
            d > 0.0 && !self.is_playing() && self.current() >= d - 0.15
        }

        /// AVPlayerItemStatusFailed
        pub fn failed(&self) -> bool {
            let st: isize = unsafe { msg_send![&*self.item, status] };
            st == 2
        }

        /// Arayüzün bir sonraki kareyi hemen çizmesi gerekiyor mu?
        pub fn wants_repaint(&self) -> bool {
            self.is_playing() || (self.pending && self.pending_since.elapsed().as_millis() < 1500)
        }

        /// Yeni bir kare varsa döndürür (BGRA → RGBA; çok büyükse seyrekleştirilir).
        pub fn poll_frame(&mut self) -> Option<egui::ColorImage> {
            unsafe {
                let t: CMTime = msg_send![&*self.player, currentTime];
                let has: Bool = msg_send![&*self.output, hasNewPixelBufferForItemTime: t];
                if !has.as_bool() {
                    return None;
                }
                let pb: *mut c_void = msg_send![
                    &*self.output,
                    copyPixelBufferForItemTime: t,
                    itemTimeForDisplay: std::ptr::null_mut::<CMTime>()
                ];
                if pb.is_null() {
                    return None;
                }
                let mut out: Option<egui::ColorImage> = None;
                if CVPixelBufferLockBaseAddress(pb, 1) == 0 {
                    let base = CVPixelBufferGetBaseAddress(pb);
                    let stride = CVPixelBufferGetBytesPerRow(pb);
                    let (w, h) = (CVPixelBufferGetWidth(pb), CVPixelBufferGetHeight(pb));
                    if !base.is_null() && w > 0 && h > 0 {
                        // Çok büyükse her `step` pikselden birini al
                        let step = (w.max(h) + MAX_SIDE - 1) / MAX_SIDE;
                        let step = step.max(1);
                        let (ow, oh) = (w / step, h / step);
                        let mut rgba: Vec<u8> = Vec::with_capacity(ow * oh * 4);
                        for y in 0..oh {
                            let row = base.add(y * step * stride);
                            for x in 0..ow {
                                let p = row.add(x * step * 4);
                                rgba.extend_from_slice(&[*p.add(2), *p.add(1), *p, 255]);
                            }
                        }
                        out = Some(egui::ColorImage::from_rgba_unmultiplied([ow, oh], &rgba));
                    }
                    CVPixelBufferUnlockBaseAddress(pb, 1);
                }
                CFRelease(pb as *const c_void);
                self.pending = false;
                out
            }
        }
    }

    impl Drop for VideoPlayer {
        fn drop(&mut self) {
            // Başka videoya geçince ses kesilsin
            self.pause();
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
mod stub {
    use std::path::Path;

    use eframe::egui;

    pub fn thumbnail(_path: &Path, _max_side: u32) -> Option<image::RgbaImage> {
        None
    }

    pub struct VideoPlayer;

    impl VideoPlayer {
        pub fn open(_path: &Path) -> Option<VideoPlayer> {
            None
        }
        pub fn set_rate(&self, _rate: f32) {}
        pub fn pause(&self) {}
        pub fn rate(&self) -> f32 {
            0.0
        }
        pub fn is_playing(&self) -> bool {
            false
        }
        pub fn current(&self) -> f64 {
            0.0
        }
        pub fn duration(&self) -> f64 {
            0.0
        }
        pub fn seek(&mut self, _secs: f64) {}
        pub fn ended(&self) -> bool {
            false
        }
        pub fn failed(&self) -> bool {
            true
        }
        pub fn wants_repaint(&self) -> bool {
            false
        }
        pub fn poll_frame(&mut self) -> Option<egui::ColorImage> {
            None
        }
    }
}
