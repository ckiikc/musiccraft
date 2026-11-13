//! AppKit menus enqueue commands; the existing app/player applies them on its next frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Files,
    Playlists,
    Queue,
    Folder,
    Network,
    PairPhone,
    AutoSync,
    Sources,
    CancelSync,
    Facets,
    ClearFilters,
    PlayPause,
    Previous,
    Next,
    Stop,
    Sequence,
    ShuffleTracks,
    ShuffleAlbums,
    Repeat,
    Details,
    Artist,
    Settings,
    Multiroom,
    OnlineCovers,
    SaveCovers,
    Audio,
    Pinned,
    Mini,
    Guide,
}
use Action::*;

#[cfg(target_os = "macos")]
const GROUPS: &[(&str, &[Option<Action>])] = &[
    (
        "File",
        &[
            Some(Files),
            Some(Folder),
            None,
            Some(PairPhone),
            Some(Network),
        ],
    ),
    (
        "Library",
        &[
            Some(Playlists),
            Some(Queue),
            None,
            Some(AutoSync),
            None,
            Some(Sources),
            Some(CancelSync),
            Some(Facets),
            Some(ClearFilters),
        ],
    ),
    (
        "Playback",
        &[
            Some(PlayPause),
            Some(Previous),
            Some(Next),
            Some(Stop),
            None,
            Some(Sequence),
            Some(ShuffleTracks),
            Some(ShuffleAlbums),
            Some(Repeat),
            None,
            Some(Details),
            Some(Artist),
        ],
    ),
    (
        "Settings",
        &[
            Some(Settings),
            Some(Multiroom),
            None,
            Some(OnlineCovers),
            Some(SaveCovers),
            Some(Audio),
            Some(Pinned),
            Some(Mini),
        ],
    ),
    ("Help", &[Some(Guide)]),
];

#[derive(Clone, PartialEq, Eq)]
pub struct State {
    pub language: &'static str,
    pub can_toggle: bool,
    pub navigation: bool,
    pub paused: bool,
    pub scanning: bool,
    pub auto_sync: bool,
    pub facets: bool,
    pub order: usize,
    pub repeat: bool,
    pub online_covers: bool,
    pub save_covers: bool,
    pub pinned: bool,
    pub mini: bool,
}
impl Action {
    #[cfg(target_os = "macos")]
    fn label(self, state: &State) -> &'static str {
        crate::i18n::logical_text(match self {
            Files => "Add files or playlist…",
            Playlists => "Playlists",
            Queue => "Playback queue",
            Folder => "Add local / NAS folder…",
            Network => "Connect smartphone / FTP…",
            PairPhone => "Pair with MusicCraft Android",
            AutoSync => "Synchronize automatically",
            Sources => "Sources and synchronization…",
            CancelSync => "Cancel synchronization",
            Facets => "Faceted browser",
            ClearFilters => "Clear search and filters",
            PlayPause => {
                if state.paused {
                    "Play"
                } else {
                    "Pause"
                }
            }
            Previous => "Previous track",
            Next => "Next track",
            Stop => "Stop",
            Sequence => "Sequence",
            ShuffleTracks => "Shuffle tracks",
            ShuffleAlbums => "Shuffle albums",
            Repeat => "Repeat track",
            Details => "Track details…",
            Artist => "Artist and lyrics…",
            Settings => "All settings…",
            Multiroom => "Server and multiroom…",
            OnlineCovers => "Search online for missing covers",
            SaveCovers => "Save found covers in the album folder",
            Audio => "Audio output…",
            Pinned => "Always on top",
            Mini => "Mini player",
            Guide => "User guide",
        })
    }
    pub fn enabled(self, state: &State) -> bool {
        match self {
            PlayPause => state.can_toggle,
            Previous | Next => state.navigation,
            CancelSync => state.scanning,
            _ => true,
        }
    }
    #[cfg(target_os = "macos")]
    fn checked(self, state: &State) -> bool {
        match self {
            AutoSync => state.auto_sync,
            Facets => state.facets,
            Sequence => state.order == 0,
            ShuffleTracks => state.order == 1,
            ShuffleAlbums => state.order == 2,
            Repeat => state.repeat,
            OnlineCovers => state.online_covers,
            SaveCovers => state.save_covers,
            Pinned => state.pinned,
            Mini => state.mini,
            _ => false,
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn sync(_: &eframe::egui::Context, _: State) {}
#[cfg(not(target_os = "macos"))]
pub fn take() -> Vec<Action> {
    Vec::new()
}
#[cfg(target_os = "macos")]
pub use macos::{sync, take};

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
    use std::cell::RefCell;

    struct Bridge {
        // NSMenuItem retains its action target weakly.
        _target: Retained<MenuHandler>,
        roots: Vec<(&'static str, Retained<NSMenuItem>)>,
        items: Vec<(Action, Retained<NSMenuItem>)>,
        context: eframe::egui::Context,
        pending: Vec<Action>,
        state: Option<State>,
    }
    thread_local! { static BRIDGE: RefCell<Option<Bridge>> = const {RefCell::new(None)}; }
    define_class!(
        // SAFETY: NSObject has no subclass invariants; AppKit sends menu actions on main.
        #[unsafe(super=NSObject)]
        #[thread_kind=MainThreadOnly]
        struct MenuHandler;
        unsafe impl NSObjectProtocol for MenuHandler {}
        impl MenuHandler {
            #[unsafe(method(musicCraftMenuAction:))]
            fn perform(&self, sender: &NSMenuItem) {
                BRIDGE.with(|slot| {
                    let mut slot=slot.borrow_mut();
                    let Some(bridge)=slot.as_mut() else {return};
                    let action=bridge.items.iter().find(|(_,item)| item.tag()==sender.tag()).map(|(action,_)| *action);
                    if let Some(action)=action
                        && bridge.state.as_ref().is_some_and(|s|action.enabled(s))
                        && bridge.pending.len()<64 {
                        bridge.pending.push(action);
                        bridge.context.request_repaint();
                    }
                });
            }
        }
    );
    fn item(
        mtm: MainThreadMarker,
        target: &MenuHandler,
        action: Action,
        shortcut: &str,
    ) -> Retained<NSMenuItem> {
        // SAFETY: target implements a one-object AppKit action and Bridge retains it.
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(""),
                Some(sel!(musicCraftMenuAction:)),
                &NSString::from_str(shortcut),
            );
            item.setTarget(Some(target));
            item.setTag(action as isize + 7300);
            item
        }
    }
    pub fn sync(ctx: &eframe::egui::Context, state: State) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        BRIDGE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                let app = NSApplication::sharedApplication(mtm);
                let Some(main) = app.mainMenu() else { return };
                // Keep winit's application menu, standard About action, Services, Hide and Quit.
                let Some(app_menu) = main.itemAtIndex(0).and_then(|i| i.submenu()) else {
                    return;
                };
                // SAFETY: NSObject init is valid for our ivar-free subclass.
                let target: Retained<MenuHandler> =
                    unsafe { msg_send![MenuHandler::alloc(mtm), init] };
                let mut bridge = Bridge {
                    _target: target,
                    roots: Vec::new(),
                    items: Vec::new(),
                    context: ctx.clone(),
                    pending: Vec::new(),
                    state: None,
                };
                let settings = item(mtm, &bridge._target, Settings, ",");
                app_menu.insertItem_atIndex(&settings, 2.min(app_menu.numberOfItems()));
                bridge.items.push((Settings, settings));
                for (title, actions) in GROUPS {
                    let root = NSMenuItem::new(mtm);
                    let submenu =
                        NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(""));
                    submenu.setAutoenablesItems(false);
                    for action in *actions {
                        let Some(action) = action else {
                            submenu.addItem(&NSMenuItem::separatorItem(mtm));
                            continue;
                        };
                        let native = item(
                            mtm,
                            &bridge._target,
                            *action,
                            if *action == Files { "o" } else { "" },
                        );
                        submenu.addItem(&native);
                        bridge.items.push((*action, native));
                    }
                    root.setSubmenu(Some(&submenu));
                    main.addItem(&root);
                    bridge.roots.push((title, root));
                }
                *slot = Some(bridge);
            }
            let bridge = slot.as_mut().unwrap();
            if bridge.state.as_ref() == Some(&state) {
                return;
            };
            for (title, root) in &bridge.roots {
                let title = NSString::from_str(crate::i18n::logical_text(title));
                root.setTitle(&title);
                if let Some(menu) = root.submenu() {
                    menu.setTitle(&title)
                }
            }
            for (action, item) in &bridge.items {
                item.setTitle(&NSString::from_str(action.label(&state)));
                item.setEnabled(action.enabled(&state));
                item.setState(if action.checked(&state) { 1 } else { 0 });
            }
            bridge.state = Some(state);
        });
    }
    pub fn take() -> Vec<Action> {
        BRIDGE.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .map(|b| std::mem::take(&mut b.pending))
                .unwrap_or_default()
        })
    }
    #[cfg(debug_assertions)]
    pub(super) fn invoke(action: Action) {
        let item = BRIDGE.with(|slot| {
            slot.borrow().as_ref().and_then(|b| {
                b.items
                    .iter()
                    .find(|(a, _)| *a == action)
                    .map(|(_, i)| i.clone())
            })
        });
        if let Some(item) = item {
            // SAFETY: this is the installed AppKit item and its retained action target.
            unsafe {
                NSApplication::sharedApplication(MainThreadMarker::new().unwrap())
                    .sendAction_to_from(
                        item.action().unwrap(),
                        item.target().as_deref(),
                        Some(&item),
                    );
            }
        }
    }
}

/// Existing debug-preview mechanism only; invoked through the real installed menu item.
#[cfg(debug_assertions)]
pub fn preview(ctx: &eframe::egui::Context) {
    if ctx.cumulative_frame_nr() < 4 {
        ctx.request_repaint();
        return;
    }
    if let Some(action) = ctx.data_mut(|d| {
        let id = eframe::egui::Id::new("native-menu-preview");
        let action = d.get_temp::<Action>(id);
        d.remove::<Action>(id);
        action
    }) {
        #[cfg(target_os = "macos")]
        macos::invoke(action);
        #[cfg(not(target_os = "macos"))]
        let _ = action;
    }
}
