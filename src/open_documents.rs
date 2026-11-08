//! OS document-open requests, queued before or after the GUI starts.
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
static REQUESTS: Mutex<Vec<Vec<PathBuf>>> = Mutex::new(Vec::new());
static CONTEXT: OnceLock<eframe::egui::Context> = OnceLock::new();
#[cfg(any(target_os = "macos", test))]
fn enqueue(paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let mut queue = REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
    if queue.len() >= 32 {
        queue.remove(0);
    }
    queue.push(paths);
    drop(queue);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}
pub fn attach(ctx: &eframe::egui::Context) {
    let _ = CONTEXT.set(ctx.clone());
}
pub fn take() -> Vec<Vec<PathBuf>> {
    std::mem::take(&mut *REQUESTS.lock().unwrap_or_else(|e| e.into_inner()))
}
pub fn audio_files(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .take(1024)
        .filter_map(|p| {
            if !crate::library::audio_path(&p) || !p.is_file() {
                return None;
            }
            let p = p.canonicalize().ok()?;
            seen.insert(p.clone()).then_some(p)
        })
        .collect()
}
#[cfg(not(target_os = "macos"))]
pub fn install() {}
#[cfg(target_os = "macos")]
pub fn install() {
    macos::install();
}
#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::{
        MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::AnyObject,
        sel,
    };
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
    define_class!(
        // SAFETY: NSObject has no subclass invariants; callbacks run on AppKit's main thread.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct DocumentHandler;
        unsafe impl NSObjectProtocol for DocumentHandler {}
        impl DocumentHandler {
            #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
            fn handle(&self, event: &AnyObject, _reply: &AnyObject) {
                // SAFETY: NSAppleEventManager invokes this selector with two
                // NSAppleEventDescriptor objects. FourCCs and selectors are the
                // documented Open Documents/direct object/file URL protocol.
                unsafe {
                    let list: Option<Retained<AnyObject>> = msg_send![event, paramDescriptorForKeyword: u32::from_be_bytes(*b"----")];
                    let Some(list) = list else { return; };
                    let count: isize = msg_send![&*list, numberOfItems];
                    let mut paths = Vec::new();
                    for i in 1..=count.min(1024) {
                        let item: Option<Retained<AnyObject>> = msg_send![&*list, descriptorAtIndex: i];
                        let Some(item) = item else { continue; };
                        let file: Option<Retained<AnyObject>> = msg_send![&*item, coerceToDescriptorType: u32::from_be_bytes(*b"furl")];
                        let Some(file) = file else { continue; };
                        let native_url: Option<Retained<AnyObject>> = msg_send![&*file, fileURLValue];
                        let value: Option<Retained<NSString>> = native_url.and_then(|u| msg_send![&*u, absoluteString]);
                        if let Some(path) = value.and_then(|s| url::Url::parse(&s.to_string()).ok()).and_then(|u| u.to_file_path().ok()) { paths.push(path); }
                    }
                    enqueue(paths);
                }
            }
        }
    );
    thread_local! { static HANDLER: std::cell::RefCell<Option<Retained<DocumentHandler>>> = const { std::cell::RefCell::new(None) }; }
    pub fn install() {
        use objc2::runtime::ProtocolObject;
        use objc2_foundation::{NSNotificationCenter, NSObjectProtocol};
        thread_local! { static OBSERVER: std::cell::RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { std::cell::RefCell::new(None) }; }
        OBSERVER.with(|slot| {
            if slot.borrow().is_some() {
                return;
            }
            let callback =
                block2::RcBlock::new(|_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
                    register_handler()
                });
            // AppKit installs its standard Apple Event handlers during launch.
            // Register ours at will-finish, before queued Finder open events.
            let observer = unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(objc2_app_kit::NSApplicationWillFinishLaunchingNotification),
                    None,
                    None,
                    &callback,
                )
            };
            *slot.borrow_mut() = Some(observer);
        });
        register_handler();
    }
    fn register_handler() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // Initialize AppKit before overriding its default document handler.
        let _ = objc2_app_kit::NSApplication::sharedApplication(mtm);
        HANDLER.with(|slot| {

            // SAFETY: NSAppleEventManager is a Foundation singleton; we retain
            // the handler for the process lifetime, and its selector above has
            // the required two-descriptor signature. Winit's delegate is intact.
            unsafe {
                let handler: Retained<DocumentHandler> = msg_send![DocumentHandler::alloc(mtm), init];
                let class = objc2::runtime::AnyClass::get(c"NSAppleEventManager").expect("Foundation Apple Event manager");
                let manager: Retained<AnyObject> = msg_send![class, sharedAppleEventManager];
                let _: () = msg_send![&*manager, setEventHandler: &*handler, andSelector: sel!(handleOpenDocuments:withReplyEvent:), forEventClass: u32::from_be_bytes(*b"aevt"), andEventID: u32::from_be_bytes(*b"odoc")];
                *slot.borrow_mut() = Some(handler);
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_browser_paths_preserve_order_unicode_and_deduplicate() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("Caffè #1.MP3");
        let b = d.path().join("日本語.wav");
        let other = d.path().join("notes.txt");
        for p in [&a, &b, &other] {
            std::fs::write(p, b"fixture").unwrap();
        }
        assert_eq!(
            audio_files(vec![
                a.clone(),
                other,
                b.clone(),
                a.clone(),
                d.path().join("missing.flac")
            ]),
            vec![a.canonicalize().unwrap(), b.canonicalize().unwrap()]
        );
    }
    #[test]
    fn requests_arriving_before_or_after_startup_remain_batched() {
        let _ = take();
        enqueue(vec!["/first.wav".into(), "/second.mp3".into()]);
        enqueue(vec!["/third.flac".into()]);
        assert_eq!(take().iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 1]);
        assert!(take().is_empty());
    }
}
