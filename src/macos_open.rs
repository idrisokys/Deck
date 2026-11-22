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

    pub fn install(ctx: &eframe::egui::Context) {
        let _ = CTX.set(ctx.clone());
        unsafe {
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
