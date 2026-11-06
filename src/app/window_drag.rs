//! Preserve the original AppKit mouse-down across egui's drag threshold.
//!
//! winit 0.30 uses NSApplication.currentEvent() for StartDrag. By the time an
//! egui drag is recognized, that can be mouseDragged or an unrelated event.
//! AppKit requires the original mouse-down, in the correct window coordinates.

use eframe::egui;

pub(super) fn reset(ctx: &egui::Context) {
    let id = egui::Id::new("mini-drag");
    ctx.data_mut(|data| {
        data.remove::<bool>(id.with("native-drag-started"));
        data.remove::<bool>(id.with("titlebar-pressed"));
    });
    cancel();
}

#[cfg(debug_assertions)]
thread_local! {
    static TRACE: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}
#[cfg(debug_assertions)]
pub(crate) fn enable_trace(path: std::path::PathBuf) {
    TRACE.with(|slot| *slot.borrow_mut() = Some(path));
}
#[cfg(debug_assertions)]
pub(super) fn trace(value: serde_json::Value) {
    use std::io::Write;
    TRACE.with(|slot| {
        if let Some(path) = slot.borrow().as_ref()
            && let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
        {
            let _ = writeln!(file, "{value}");
        }
    });
}

#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn install() {}
#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn shutdown() {}
#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn cancel() {}
#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn arm(_: &egui::Context) {}
#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn start(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
}

#[cfg(all(target_os = "macos", not(test)))]
pub(super) use macos::{arm, cancel, install, shutdown, start};

#[cfg(all(target_os = "macos", not(test)))]
mod macos {
    use super::egui;
    use block2::RcBlock;
    use objc2::{MainThreadMarker, Message, rc::Retained, runtime::AnyObject};
    #[cfg(debug_assertions)]
    use objc2_app_kit::NSApplication;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventType, NSWindow};
    use std::{cell::RefCell, ptr::NonNull, rc::Rc};

    struct Press {
        event: Retained<NSEvent>,
        window: Retained<NSWindow>,
        frame: [f64; 4],
    }
    #[derive(Default)]
    struct Events {
        down: Option<Retained<NSEvent>>,
        armed: Option<Press>,
    }
    struct Monitor {
        token: Retained<AnyObject>,
        events: Rc<RefCell<Events>>,
    }
    impl Drop for Monitor {
        fn drop(&mut self) {
            // SAFETY: this is the token returned by AppKit's local monitor API,
            // installed and removed only on the UI thread.
            unsafe { NSEvent::removeMonitor(&self.token) };
        }
    }
    thread_local! {
        static MONITOR: RefCell<Option<Monitor>> = const { RefCell::new(None) };
    }
    fn frame(window: &NSWindow) -> [f64; 4] {
        let rect = window.frame();
        [
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        ]
    }
    pub(in super::super) fn install() {
        if MainThreadMarker::new().is_none() {
            return;
        }
        MONITOR.with(|slot| {
            if slot.borrow().is_some() {
                return;
            }
            let events = Rc::new(RefCell::new(Events::default()));
            let state = events.clone();
            let handler = RcBlock::new(move |event: NonNull<NSEvent>| {
                // SAFETY: AppKit supplies a live event for this callback. Retain
                // only the original left-down; return the same event unmodified.
                let event = unsafe { event.as_ref() };
                #[cfg(debug_assertions)]
                super::trace(serde_json::json!({"stage":"event", "type":event.r#type().0, "window":event.windowNumber(), "number":event.eventNumber()}));
                let mut state = state.borrow_mut();
                state.armed = None;
                state.down = (event.r#type() == NSEventType::LeftMouseDown).then(|| event.retain());
                event as *const NSEvent as *mut NSEvent
            });
            // App-local observation, not a global event tap; no input is consumed.
            // SAFETY: callback returns exactly its valid AppKit input pointer.
            let token = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                    NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp,
                    &handler,
                )
            };
            if let Some(token) = token {
                *slot.borrow_mut() = Some(Monitor { token, events });
            }
        });
    }
    pub(in super::super) fn shutdown() {
        MONITOR.with(|slot| {
            slot.borrow_mut().take();
        });
    }
    pub(in super::super) fn cancel() {
        MONITOR.with(|slot| {
            if let Some(monitor) = slot.borrow().as_ref() {
                monitor.events.borrow_mut().armed = None;
            }
        });
    }
    pub(in super::super) fn arm(ctx: &egui::Context) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(pointer) = ctx.input(|i| i.pointer.press_origin()) else {
            return;
        };
        let Some(size) = ctx.input(|i| i.viewport().inner_rect.map(|rect| rect.size())) else {
            return;
        };
        MONITOR.with(|slot| {
            let slot = slot.borrow();
            let Some(monitor) = slot.as_ref() else {
                return;
            };
            let mut state = monitor.events.borrow_mut();
            let Some(event) = state.down.clone() else {
                return;
            };
            let Some(window) = event.window(mtm) else {
                return;
            };
            let content = window.contentRectForFrameRect(window.frame());
            let point = event.locationInWindow();
            let zoom = f64::from(ctx.zoom_factor());
            let native_pointer = egui::pos2(
                (point.x / zoom) as f32,
                ((content.size.height - point.y) / zoom) as f32,
            );
            let native_size = egui::vec2(
                (content.size.width / zoom) as f32,
                (content.size.height / zoom) as f32,
            );
            #[cfg(debug_assertions)]
            super::trace(serde_json::json!({"stage":"arm", "pointer":[pointer.x,pointer.y], "native_pointer":[native_pointer.x,native_pointer.y],"size":[size.x,size.y],"native_size":[native_size.x,native_size.y],"frame":frame(&window)}));
            // Bind this UI press to its native window, never an auxiliary window
            // or a stale down from the full-player geometry.
            if native_pointer.distance(pointer) <= 2.0
                && (native_size - size).abs().max_elem() <= 2.0
            {
                state.armed = Some(Press {
                    event,
                    frame: frame(&window),
                    window,
                });
            }
        });
    }
    pub(in super::super) fn start(_: &egui::Context) {
        // Take the press before handing control to Window Server; AppKit can
        // reenter the event loop and remove/replace the current left-down.
        let press = MONITOR.with(|slot| {
            let slot = slot.borrow();
            let monitor = slot.as_ref()?;
            let mut state = monitor.events.borrow_mut();
            let press = state.armed.take()?;
            let down = state.down.as_ref()?;
            (down.eventNumber() == press.event.eventNumber()
                && down.timestamp() == press.event.timestamp()
                && down.windowNumber() == press.event.windowNumber()
                && frame(&press.window) == press.frame)
                .then_some(press)
        });
        if let Some(press) = press {
            #[cfg(debug_assertions)]
            if let Some(mtm) = MainThreadMarker::new() {
                let current = NSApplication::sharedApplication(mtm).currentEvent();
                super::trace(
                    serde_json::json!({"stage":"start", "original_type":press.event.r#type().0, "original_window":press.event.windowNumber(), "current_type":current.as_ref().map(|e|e.r#type().0),"current_window":current.as_ref().map(|e|e.windowNumber()), "frame":press.frame}),
                );
            }
            press.window.performWindowDragWithEvent(&press.event);
        }
    }
}
