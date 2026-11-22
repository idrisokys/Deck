//! macOS: Finder'dan çift tık / "Birlikte aç" / Dock simgesine sürükleme.
//! Bu yollarla gelen dosyalar komut satırı argümanı olarak değil, "dosyayı aç"
//! Apple Event'i ('aevt'/'odoc') olarak gelir; burada yakalanıp kuyruğa konur.

use std::path::PathBuf;
use std::sync::Mutex;

static QUEUE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Gelen (henüz işlenmemiş) dosya yollarını alır.
pub fn take() -> Vec<PathBuf> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

#[cfg(target_os = "macos")]
mod imp {
    use super::QUEUE;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
    use objc2::{msg_send, sel};

    static CTX: OnceLock<eframe::egui::Context> = OnceLock::new();

    const AE_CLASS: u32 = 0x6165_7674; // 'aevt'
    const AE_OPEN_DOCS: u32 = 0x6F64_6F63; // 'odoc'
    const KEY_DIRECT_OBJECT: u32 = 0x2D2D_2D2D; // '----'

    unsafe fn path_of(desc: *mut AnyObject) -> Option<PathBuf> {
        if desc.is_null() {
            return None;
        }
        let url: *mut AnyObject = msg_send![desc, fileURLValue];
        if url.is_null() {
            return None;
        }
        let ns: *mut AnyObject = msg_send![url, path];
        if ns.is_null() {
            return None;
        }
        let c: *const c_char = msg_send![ns, UTF8String];
        if c.is_null() {
            return None;
        }
        Some(PathBuf::from(CStr::from_ptr(c).to_string_lossy().into_owned()))
    }

    extern "C" fn handle_open(_this: *mut AnyObject, _cmd: Sel, event: *mut AnyObject, _reply: *mut AnyObject) {
        if event.is_null() {
            return;
        }
        let mut found: Vec<PathBuf> = Vec::new();
        unsafe {
            let list: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT];
            if !list.is_null() {
                let n: isize = msg_send![list, numberOfItems];
                if n == 0 {
                    found.extend(path_of(list));
                }
                for i in 1..=n {
                    let item: *mut AnyObject = msg_send![list, descriptorAtIndex: i];
                    found.extend(path_of(item));
                }
            }
        }
        if found.is_empty() {
            return;
        }
        if let Ok(mut q) = QUEUE.lock() {
            q.extend(found);
        }
        if let Some(ctx) = CTX.get() {
            ctx.request_repaint();
        }
    }

    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn class_addMethod(cls: *const std::ffi::c_void, name: *const std::ffi::c_void, imp: *const std::ffi::c_void, types: *const c_char) -> i8;
        fn object_getClass(obj: *const std::ffi::c_void) -> *const std::ffi::c_void;
    }

    /// NSApplicationDelegate: `application:openFiles:` (macOS'un varsayılan "aç" işleyicisi bunu çağırır)
    extern "C" fn delegate_open_files(_this: *mut AnyObject, _cmd: Sel, app: *mut AnyObject, files: *mut AnyObject) {
        let mut found: Vec<PathBuf> = Vec::new();
        unsafe {
            if !files.is_null() {
                let n: usize = msg_send![files, count];
                for i in 0..n {
                    let ns: *mut AnyObject = msg_send![files, objectAtIndex: i];
                    if ns.is_null() {
                        continue;
                    }
                    let c: *const c_char = msg_send![ns, UTF8String];
                    if !c.is_null() {
                        found.push(PathBuf::from(CStr::from_ptr(c).to_string_lossy().into_owned()));
                    }
                }
            }
            if !app.is_null() {
                // NSApplicationDelegateReplySuccess = 0
                let _: () = msg_send![app, replyToOpenOrPrint: 0usize];
            }
        }
        if found.is_empty() {
            return;
        }
        if let Ok(mut q) = QUEUE.lock() {
            q.extend(found);
        }
        if let Some(ctx) = CTX.get() {
            ctx.request_repaint();
        }
    }

    /// winit'in uygulama delegesine `application:openFiles:` yöntemini çalışma anında ekler.
    unsafe fn add_delegate_method() {
        let Some(app_cls) = AnyClass::get("NSApplication") else { return };
        let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
        if app.is_null() {
            return;
        }
        let delegate: *mut AnyObject = msg_send![app, delegate];
        if delegate.is_null() {
            return;
        }
        let cls = object_getClass(delegate as *const std::ffi::c_void);
        if cls.is_null() {
            return;
        }
        let sel = sel!(application:openFiles:);
        let sel_ptr: *const std::ffi::c_void = std::mem::transmute::<Sel, *const std::ffi::c_void>(sel);
        let imp = delegate_open_files as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject);
        class_addMethod(cls, sel_ptr, imp as *const std::ffi::c_void, b"v@:@@\0".as_ptr() as *const c_char);
    }

    pub fn install(ctx: &eframe::egui::Context) {
        let _ = CTX.set(ctx.clone());
        unsafe {
            add_delegate_method();
            let Some(superclass) = AnyClass::get("NSObject") else { return };
            let Some(mut builder) = ClassBuilder::new("DeckOpenHandler", superclass) else { return };
            builder.add_method(
                sel!(handleOpenEvent:withReplyEvent:),
                handle_open as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );
            let cls = builder.register();
            let handler: *mut AnyObject = msg_send![cls, new]; // bilerek serbest bırakılmaz
            let Some(mgr_cls) = AnyClass::get("NSAppleEventManager") else { return };
            let mgr: *mut AnyObject = msg_send![mgr_cls, sharedAppleEventManager];
            if mgr.is_null() || handler.is_null() {
                return;
            }
            let _: () = msg_send![
                mgr,
                setEventHandler: handler,
                andSelector: sel!(handleOpenEvent:withReplyEvent:),
                forEventClass: AE_CLASS,
                andEventID: AE_OPEN_DOCS
            ];
        }
    }
}

/// Uygulama başlarken bir kez çağrılır (macOS dışında hiçbir şey yapmaz).
pub fn install(ctx: &eframe::egui::Context) {
    #[cfg(target_os = "macos")]
    imp::install(ctx);
    #[cfg(not(target_os = "macos"))]
    let _ = ctx;
}

// ---------- Varsayılan resim açıcı olarak ayarlama ----------

#[cfg(target_os = "macos")]
mod default_handler {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_void};

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(alloc: *const c_void, c: *const c_char, enc: u32) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }
    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn LSSetDefaultRoleHandlerForContentType(content_type: *const c_void, role: u32, handler: *const c_void) -> i32;
    }

    const UTF8: u32 = 0x0800_0100;
    const ROLES_ALL: u32 = 0xFFFF_FFFF;
    const BUNDLE_ID: &str = "com.idris.deck";
    /// Deck-Info.plist içindeki belge türleriyle aynı
    const UTIS: &[&str] = &[
        "public.jpeg",
        "public.png",
        "com.compuserve.gif",
        "com.microsoft.bmp",
        "public.tiff",
        "org.webmproject.webp",
        "com.microsoft.ico",
        "com.truevision.tga-image",
    ];

    fn cfstr(s: &str) -> *const c_void {
        let c = CString::new(s).unwrap_or_default();
        unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
    }

    pub fn set_all() {
        let handler = cfstr(BUNDLE_ID);
        for uti in UTIS {
            let ct = cfstr(uti);
            unsafe {
                LSSetDefaultRoleHandlerForContentType(ct, ROLES_ALL, handler);
                CFRelease(ct);
            }
        }
        unsafe { CFRelease(handler) };
    }
}

/// İlk açılışta bir kez sorar: Deck varsayılan resim açıcı olsun mu? (macOS dışında hiçbir şey yapmaz)
pub fn offer_default(tr: bool) {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let dir = home.join("Library").join("Application Support").join("Deck");
        let marker = dir.join("default_asked");
        if marker.exists() {
            return;
        }
        let (title, text) = if tr {
            ("Deck varsayılan resim açıcı olsun mu?", "Tüm resim biçimleri (JPEG, PNG, GIF, WebP, TIFF, BMP…) çift tıklayınca Deck ile açılsın mı? Bunu daha sonra Finder'da Bilgi Al ▸ Birlikte aç bölümünden değiştirebilirsiniz.")
        } else {
            ("Make Deck your default image viewer?", "Open all image formats (JPEG, PNG, GIF, WebP, TIFF, BMP…) with Deck when double-clicked? You can change this later in Finder via Get Info ▸ Open with.")
        };
        let yes = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title(title)
            .set_description(text)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes;
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&marker, if yes { "yes" } else { "no" });
        if yes {
            default_handler::set_all();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = tr;
}
