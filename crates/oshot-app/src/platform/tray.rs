//! The menu bar item and the Finder "Convert with OShot" service.

use std::cell::RefCell;
use std::path::PathBuf;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, NSObject, Sel};
use objc2::{ClassType, class, define_class, msg_send, sel};
use objc2_foundation::{NSSize, NSString};
use oshot_core::settings::Hotkey;

use crate::state::{Command, MenuEntry, PlatformEvent};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "OShotMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(itemClicked:))]
        fn item_clicked(&self, sender: &AnyObject) {
            // SAFETY: senders are NSMenuItems, which have a tag.
            let tag: isize = unsafe { msg_send![sender, tag] };
            let command = COMMANDS.with(|c| c.borrow().get(tag as usize).cloned());
            if let Some(command) = command {
                super::send(PlatformEvent::Command(command));
            }
        }

        /// Finder Services entry point, declared as `convertFiles` in Info.plist.
        #[unsafe(method(convertFiles:userData:error:))]
        fn convert_files(&self, pasteboard: &AnyObject, _user_data: *mut AnyObject, _error: *mut *mut AnyObject) {
            let paths = file_paths(pasteboard);
            if !paths.is_empty() {
                super::send(PlatformEvent::Command(Command::ConvertFiles(paths)));
            }
        }
    }
);

thread_local! {
    static TARGET: RefCell<Option<Retained<MenuTarget>>> = const { RefCell::new(None) };
    static STATUS_ITEM: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    static COMMANDS: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
}

fn target() -> Retained<MenuTarget> {
    TARGET.with(|t| {
        t.borrow_mut()
            // SAFETY: plain NSObject subclass with no ivars.
            .get_or_insert_with(|| unsafe { msg_send![MenuTarget::class(), new] })
            .clone()
    })
}

fn file_paths(pasteboard: &AnyObject) -> Vec<PathBuf> {
    // SAFETY: reading NSURL objects from a pasteboard.
    unsafe {
        let classes: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: class!(NSURL)];
        let urls: Option<Retained<AnyObject>> =
            msg_send![pasteboard, readObjectsForClasses: &*classes, options: std::ptr::null::<AnyObject>()];
        let Some(urls) = urls else { return Vec::new() };
        let count: usize = msg_send![&*urls, count];
        (0..count)
            .filter_map(|i| {
                let url: Retained<AnyObject> = msg_send![&*urls, objectAtIndex: i];
                let path: Option<Retained<NSString>> = msg_send![&*url, path];
                path.map(|p| PathBuf::from(p.to_string()))
            })
            .collect()
    }
}

pub fn install_services() {
    // SAFETY: main-thread AppKit calls.
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let t = target();
        let _: () = msg_send![&*app, setServicesProvider: &*t];
    }
}

fn key_equivalent(shortcut: &str) -> Option<(String, usize)> {
    let hk = Hotkey::parse(shortcut)?;
    let mut mask = 0usize;
    if hk.shift {
        mask |= 1 << 17;
    }
    if hk.ctrl {
        mask |= 1 << 18;
    }
    if hk.alt {
        mask |= 1 << 19;
    }
    if hk.cmd {
        mask |= 1 << 20;
    }
    (hk.key.chars().count() == 1).then(|| (hk.key.clone(), mask))
}

/// # Safety
/// Main thread only.
unsafe fn item(title: &str, action: Option<Sel>, key: &str) -> Retained<AnyObject> {
    let title = NSString::from_str(title);
    let key = NSString::from_str(key);
    // SAFETY: standard NSMenuItem initializer.
    unsafe {
        let alloc: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
        msg_send![alloc, initWithTitle: &*title, action: action, keyEquivalent: &*key]
    }
}

/// # Safety
/// Main thread only.
unsafe fn command_item(title: &str, command: Command, shortcut: Option<&str>, enabled: bool) -> Retained<AnyObject> {
    let tag = COMMANDS.with(|c| {
        let mut c = c.borrow_mut();
        c.push(command);
        c.len() - 1
    });
    let (key, mask) = shortcut.and_then(key_equivalent).unwrap_or_default();
    // SAFETY: configuring a fresh menu item.
    unsafe {
        let it = item(title, Some(sel!(itemClicked:)), &key);
        let t = target();
        let _: () = msg_send![&*it, setTarget: &*t];
        let _: () = msg_send![&*it, setTag: tag as isize];
        let _: () = msg_send![&*it, setEnabled: enabled];
        if !key.is_empty() {
            let _: () = msg_send![&*it, setKeyEquivalentModifierMask: mask];
        }
        it
    }
}

/// # Safety
/// Main thread only.
unsafe fn build(entries: &[MenuEntry]) -> Retained<AnyObject> {
    // SAFETY: building a menu from fresh items.
    unsafe {
        let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
        let _: () = msg_send![&*menu, setAutoenablesItems: false];
        for entry in entries {
            let it: Retained<AnyObject> = match entry {
                MenuEntry::Item {
                    title,
                    shortcut,
                    command,
                    enabled,
                } => command_item(title, command.clone(), shortcut.as_deref(), *enabled),
                MenuEntry::Separator => msg_send![class!(NSMenuItem), separatorItem],
                MenuEntry::Header(title) => {
                    let t = NSString::from_str(title);
                    msg_send![class!(NSMenuItem), sectionHeaderWithTitle: &*t]
                }
                MenuEntry::Submenu { title, entries } => {
                    let it = item(title, None, "");
                    let sub = build(entries);
                    let _: () = msg_send![&*it, setSubmenu: &*sub];
                    it
                }
                MenuEntry::Recent {
                    title,
                    thumbnail_png,
                    path,
                } => {
                    let it = item(title, None, "");
                    let data: Retained<AnyObject> =
                        msg_send![class!(NSData), dataWithBytes: thumbnail_png.as_ptr(), length: thumbnail_png.len()];
                    let alloc: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
                    let image: Option<Retained<AnyObject>> = msg_send![alloc, initWithData: &*data];
                    if let Some(image) = image {
                        let _: () = msg_send![&*image, setSize: NSSize::new(44.0, 28.0)];
                        let _: () = msg_send![&*it, setImage: &*image];
                    }
                    let sub = build(&[
                        MenuEntry::Item {
                            title: "Open in Editor".into(),
                            shortcut: None,
                            command: Command::OpenRecent(path.clone()),
                            enabled: true,
                        },
                        MenuEntry::Item {
                            title: "Copy".into(),
                            shortcut: None,
                            command: Command::CopyRecent(path.clone()),
                            enabled: true,
                        },
                        MenuEntry::Item {
                            title: "Show in Finder".into(),
                            shortcut: None,
                            command: Command::RevealRecent(path.clone()),
                            enabled: true,
                        },
                    ]);
                    let _: () = msg_send![&*it, setSubmenu: &*sub];
                    it
                }
            };
            let _: () = msg_send![&*menu, addItem: &*it];
        }
        menu
    }
}

pub fn set_menu(entries: &[MenuEntry]) {
    COMMANDS.with(|c| c.borrow_mut().clear());
    // SAFETY: called from the GPUI main thread.
    unsafe {
        let menu = build(entries);
        STATUS_ITEM.with(|s| {
            let mut s = s.borrow_mut();
            let item = s.get_or_insert_with(|| {
                let bar: Retained<AnyObject> = msg_send![class!(NSStatusBar), systemStatusBar];
                let item: Retained<AnyObject> = msg_send![&*bar, statusItemWithLength: -1.0f64];
                let button: Option<Retained<AnyObject>> = msg_send![&*item, button];
                if let Some(button) = button {
                    let name = NSString::from_str("camera.viewfinder");
                    let desc = NSString::from_str("OShot");
                    let image: Option<Retained<AnyObject>> =
                        msg_send![class!(NSImage), imageWithSystemSymbolName: &*name, accessibilityDescription: &*desc];
                    if let Some(image) = image {
                        let _: () = msg_send![&*image, setTemplate: true];
                        let _: () = msg_send![&*button, setImage: &*image];
                    }
                }
                item
            });
            let _: () = msg_send![&**item, setMenu: &*menu];
        });
    }
}
