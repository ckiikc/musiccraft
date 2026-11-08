//! Keep AppKit's standard About panel above MusicCraft's pinned windows.
#[cfg(any(target_os = "macos", test))]
fn panel_level(owner_level: isize) -> isize {
    owner_level.max(0).saturating_add(1)
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}
#[cfg(target_os = "macos")]
pub use macos::install;

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{
        ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
        runtime::AnyObject, sel,
    };
    #[cfg(debug_assertions)]
    use objc2_app_kit::NSMenuItem;
    use objc2_app_kit::{NSApplication, NSPanel, NSWindow};
    use objc2_foundation::{NSObject, NSObjectProtocol};
    use std::cell::RefCell;

    thread_local! {
        // NSMenuItem.target is weak. Retain our target for the app's lifetime.
        static HANDLER: RefCell<Option<Retained<AboutHandler>>> = const { RefCell::new(None) };
        static PANEL: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
        #[cfg(debug_assertions)]
        static ITEM: RefCell<Option<Retained<NSMenuItem>>> = const { RefCell::new(None) };
    }
    define_class!(
        // SAFETY: NSObject has no subclass invariants; AppKit invokes menu actions on main.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct AboutHandler;
        unsafe impl NSObjectProtocol for AboutHandler {}
        impl AboutHandler {
            #[unsafe(method(showMusicCraftAbout:))]
            fn show(&self, sender: Option<&AnyObject>) {
                let app = NSApplication::sharedApplication(MainThreadMarker::from(self));
                let known = PANEL.with(|p| p.borrow().clone());
                let before = app.windows();
                let owner_level = before.iter()
                    .filter(|w| w.isVisible() && known.as_ref().is_none_or(|p| p.windowNumber() != w.windowNumber()))
                    // Ignore transient menus/tooltips; ordinary and pinned app windows are 0/3.
                    .map(|w| w.level()).filter(|level| (0..=3).contains(level)).max().unwrap_or(0);
                app.orderFrontStandardAboutPanel(sender);
                let panel = app.windows().iter().find(|w| {
                    w.isVisible() && w.isKindOfClass(NSPanel::class())
                        && !before.iter().any(|old| old.windowNumber() == w.windowNumber())
                }).or_else(|| known.filter(|w| w.isVisible())).or_else(|| app.keyWindow().filter(|w| w.isKindOfClass(NSPanel::class())));
                if let Some(panel) = panel {
                    panel.setLevel(super::panel_level(owner_level));
                    panel.makeKeyAndOrderFront(sender);
                    PANEL.with(|p| *p.borrow_mut() = Some(panel));
                }
            }
        }
    );
    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        HANDLER.with(|slot| {
            if slot.borrow().is_some() {
                return;
            }
            let app = NSApplication::sharedApplication(mtm);
            let Some(menu) = app.mainMenu() else {
                return;
            };
            let item = menu
                .itemArray()
                .iter()
                .filter_map(|i| i.submenu())
                .flat_map(|m| m.itemArray().to_vec())
                .find(|i| i.action() == Some(sel!(orderFrontStandardAboutPanel:)));
            let Some(item) = item else {
                return;
            };
            // SAFETY: our target implements exactly the one-object action below,
            // and HANDLER retains it while NSMenuItem keeps only a weak reference.
            unsafe {
                let handler: Retained<AboutHandler> = msg_send![AboutHandler::alloc(mtm), init];
                item.setTarget(Some(&handler));
                item.setAction(Some(sel!(showMusicCraftAbout:)));
                *slot.borrow_mut() = Some(handler);
            }
            #[cfg(debug_assertions)]
            ITEM.with(|i| *i.borrow_mut() = Some(item));
        });
    }
    #[cfg(debug_assertions)]
    pub(super) fn close_preview() {
        PANEL.with(|p| {
            if let Some(p) = p.borrow().as_ref() {
                p.close();
            }
        });
    }
    #[cfg(debug_assertions)]
    pub(super) fn preview() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        ITEM.with(|item| {
            if let Some(item) = item.borrow().as_ref() {
                // Invoke the installed menu item, not a separate About implementation.
                unsafe {
                    NSApplication::sharedApplication(mtm).sendAction_to_from(
                        item.action().unwrap_or(sel!(showMusicCraftAbout:)),
                        item.target().as_deref(),
                        Some(item),
                    );
                }
            }
        });
    }
}

#[cfg(debug_assertions)]
pub fn preview(ctx: &eframe::egui::Context) {
    let id = eframe::egui::Id::new("native-about-preview");
    if ctx.cumulative_frame_nr() < 4 {
        ctx.request_repaint();
        return;
    }
    let reopen = id.with("reopen");
    if ctx.data_mut(|d| d.remove_temp::<bool>(id)).unwrap_or(false) {
        #[cfg(target_os = "macos")]
        {
            macos::preview();
            macos::close_preview();
        }
        ctx.data_mut(|d| {
            d.insert_temp(
                reopen,
                std::time::Instant::now() + std::time::Duration::from_secs(1),
            )
        });
    }
    if let Some(deadline) = ctx.data(|d| d.get_temp::<std::time::Instant>(reopen)) {
        if std::time::Instant::now() >= deadline {
            ctx.data_mut(|d| d.remove::<std::time::Instant>(reopen));
            #[cfg(target_os = "macos")]
            macos::preview();
        } else {
            ctx.request_repaint_after(
                deadline.saturating_duration_since(std::time::Instant::now()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn about_is_above_normal_and_pinned_windows_without_changing_the_owner() {
        assert!(super::panel_level(3) > 3); // Regression: the AppKit default was level zero.
        assert_eq!(super::panel_level(0), 1);
    }
}
