//! Settings: a sidebar of pages, each a list of labelled controls.

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, PathPromptOptions, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    div, img, prelude::FluentBuilder, px,
};
use oshot_core::format::ImageFormat;
use oshot_core::gesture::MenuGesture;
use oshot_core::naming::{LocalTime, expand_template};
use oshot_core::settings::{AfterCapture, Corner, Hotkey, HotkeyAction, Hotkeys, OutputLocation, ThemeChoice};

use crate::commands::{apply_hotkeys, dispatch};
use crate::state::{AppState, Command, Notice, notify, platform, state, update_settings};
use crate::theme::{Theme, theme};
use crate::ui::{
    Kind, TextChanged, TextInput, button, card, divider, glass_root, icon, kbd, row, section_title, segmented, slider,
    toggle,
};
use crate::windows::{self, Spec};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Capture,
    Shortcuts,
    Saving,
    Converter,
    Organize,
    About,
}

impl Page {
    pub const ALL: [Page; 7] = [
        Page::General,
        Page::Capture,
        Page::Shortcuts,
        Page::Saving,
        Page::Converter,
        Page::Organize,
        Page::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Capture => "Capture",
            Page::Shortcuts => "Shortcuts",
            Page::Saving => "Saving",
            Page::Converter => "Converter",
            Page::Organize => "Organize",
            Page::About => "About",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::General => "settings",
            Page::Capture => "camera",
            Page::Shortcuts => "keyboard",
            Page::Saving => "download",
            Page::Converter => "arrow-right-left",
            Page::Organize => "layers",
            Page::About => "info",
        }
    }
}

pub struct SettingsWindow {
    pub page: Page,
    pub recording: Option<HotkeyAction>,
    template: Entity<TextInput>,
    focus: FocusHandle,
    _sub: Subscription,
}

impl SettingsWindow {
    pub fn new(cx: &mut Context<Self>) -> SettingsWindow {
        let template_text = state(cx).settings.filename_template.clone();
        let template = cx.new(|cx| TextInput::new(template_text, "OShot {date} at {time}", cx));
        let sub = cx.subscribe(&template, |_, _, e: &TextChanged, cx| {
            let value = e.0.clone();
            update_settings(cx, |s| s.filename_template = value);
            cx.notify();
        });
        SettingsWindow {
            page: Page::General,
            recording: None,
            template,
            focus: cx.focus_handle(),
            _sub: sub,
        }
    }

    pub fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.stop_recording(cx);
        self.page = page;
        cx.notify();
    }

    pub fn start_recording(&mut self, action: HotkeyAction, window: &mut Window, cx: &mut Context<Self>) {
        // Global hotkeys would fire instead of reaching this window, so
        // they are paused while a new one is typed.
        platform(cx).register_hotkeys(&[]);
        self.recording = Some(action);
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording.take().is_some() {
            apply_hotkeys(cx);
        }
    }

    /// Records `keystroke` (GPUI syntax, e.g. "alt-shift-8") for the action
    /// being recorded.
    pub fn record(&mut self, keystroke: &str, cx: &mut Context<Self>) {
        let Some(action) = self.recording else { return };
        if keystroke == "escape" {
            self.stop_recording(cx);
            cx.notify();
            return;
        }
        if keystroke == "backspace" || keystroke == "delete" {
            self.recording = None;
            update_settings(cx, |s| s.hotkeys.set(action, None));
            apply_hotkeys(cx);
            cx.notify();
            return;
        }
        let Some(hk) = Hotkey::parse(keystroke) else {
            notify(cx, Notice::Info("Shortcuts need ⌘, ⌃ or ⌥".into()));
            return;
        };
        if let Some(other) = state(cx).settings.hotkeys.conflict(&hk, action) {
            notify(
                cx,
                Notice::Error(format!("{} is already used by {}", hk.symbols(), other.label())),
            );
            return;
        }
        if hk.is_system_reserved() {
            notify(
                cx,
                Notice::Info(
                    "macOS also uses this. Turn it off in System Settings › Keyboard › Shortcuts › Screenshots".into(),
                ),
            );
        }
        self.recording = None;
        let value = hk.to_keystroke();
        update_settings(cx, |s| s.hotkeys.set(action, Some(value)));
        apply_hotkeys(cx);
        cx.notify();
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if self.recording.is_some() {
            if matches!(
                k.key.as_str(),
                "shift" | "control" | "alt" | "platform" | "cmd" | "function"
            ) {
                return;
            }
            self.record(&keystroke_string(k), cx);
            cx.stop_propagation();
            return;
        }
        if k.modifiers.platform && k.key == "w" {
            window.remove_window();
        }
    }

    fn general(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let s = &state(cx).settings;
        let theme_idx = match s.theme {
            ThemeChoice::System => 0,
            ThemeChoice::Light => 1,
            ThemeChoice::Dark => 2,
        };
        let after_idx = AfterCapture::ALL
            .iter()
            .position(|a| *a == s.after_capture)
            .unwrap_or(0);
        let corner_idx = [
            Corner::TopLeft,
            Corner::TopRight,
            Corner::BottomLeft,
            Corner::BottomRight,
        ]
        .iter()
        .position(|c| *c == s.thumbnail_corner)
        .unwrap_or(3);
        let secs = [3u32, 6, 10, 0];
        let secs_idx = secs.iter().position(|v| *v == s.thumbnail_seconds).unwrap_or(1);
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                card(t)
                    .flex()
                    .flex_col()
                    .child(row(
                        "Appearance",
                        Some("Glass follows the system unless you pick one"),
                        segmented(
                            "theme",
                            &["System".into(), "Light".into(), "Dark".into()],
                            theme_idx,
                            t,
                            |i, _, cx| {
                                update_settings(cx, |s| {
                                    s.theme = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark][i]
                                })
                            },
                        ),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Launch at login",
                        None,
                        toggle("login", s.launch_at_login, t)
                            .debug_selector(|| "login".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.launch_at_login = !s.launch_at_login)),
                        t,
                    )),
            )
            .child(
                card(t).child(row(
                    "Close editor after ⌘C",
                    Some("Close only after the image is copied; the Copy button stays open"),
                    toggle("close-on-copy-shortcut", s.close_on_copy_shortcut, t)
                        .debug_selector(|| "close-on-copy-shortcut".into())
                        .on_click(|_, _, cx| {
                            update_settings(cx, |s| s.close_on_copy_shortcut = !s.close_on_copy_shortcut)
                        }),
                    t,
                )),
            )
            .child(section_title("AFTER A CAPTURE", t))
            .child(
                card(t)
                    .flex()
                    .flex_col()
                    .child(row(
                        "Then",
                        Some("Open the editor, show a corner thumbnail, or just copy"),
                        segmented(
                            "after",
                            &AfterCapture::ALL.map(|a| SharedString::from(a.label())),
                            after_idx,
                            t,
                            |i, _, cx| update_settings(cx, |s| s.after_capture = AfterCapture::ALL[i]),
                        ),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Copy to clipboard",
                        Some("Every capture is copied right away"),
                        toggle("copy-on-capture", s.copy_on_capture, t)
                            .debug_selector(|| "copy-on-capture".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.copy_on_capture = !s.copy_on_capture)),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Save automatically",
                        Some("Also save every capture to your folder"),
                        toggle("save-on-capture", s.save_on_capture, t)
                            .debug_selector(|| "save-on-capture".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.save_on_capture = !s.save_on_capture)),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Thumbnail corner",
                        None,
                        segmented(
                            "corner",
                            &["↖".into(), "↗".into(), "↙".into(), "↘".into()],
                            corner_idx,
                            t,
                            |i, _, cx| {
                                update_settings(cx, |s| {
                                    s.thumbnail_corner = [
                                        Corner::TopLeft,
                                        Corner::TopRight,
                                        Corner::BottomLeft,
                                        Corner::BottomRight,
                                    ][i]
                                })
                            },
                        ),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Thumbnail stays for",
                        None,
                        segmented(
                            "thumb-secs",
                            &["3 s".into(), "6 s".into(), "10 s".into(), "Until closed".into()],
                            secs_idx,
                            t,
                            move |i, _, cx| update_settings(cx, |s| s.thumbnail_seconds = secs[i]),
                        ),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Esc copies in the editor",
                        Some("Closing the editor with Esc copies the image first"),
                        toggle("esc-copies", s.escape_copies, t)
                            .debug_selector(|| "esc-copies".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.escape_copies = !s.escape_copies)),
                        t,
                    )),
            )
    }

    fn capture(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let s = &state(cx).settings;
        let granted = platform(cx).has_screen_permission();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(card(t).flex().flex_col().child(row(
                "Screen Recording",
                Some(if granted {
                    "Allowed. OShot can see your screen."
                } else {
                    "Needed to capture. macOS asks once."
                }),
                if granted {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_color(t.success)
                        .child(icon("check").text_color(t.success))
                        .child("Allowed")
                        .into_any_element()
                } else {
                    button("grant", "Open Settings", t, Kind::Primary)
                        .debug_selector(|| "grant".into())
                        .on_click(|_, _, cx| {
                            crate::permission_guide::ask(crate::permission_guide::Permission::ScreenRecording, cx)
                        })
                        .into_any_element()
                },
                t,
            )))
            .child(
                card(t)
                    .flex()
                    .flex_col()
                    .child(row(
                        "Finish on mouse release",
                        Some("Skip ⌘C / ↩ and finish an area capture as soon as you let go"),
                        toggle("finish-on-release", s.finish_on_release, t)
                            .debug_selector(|| "finish-on-release".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.finish_on_release = !s.finish_on_release)),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Window shadow",
                        Some("Keep the soft macOS shadow around captured windows"),
                        toggle("window-shadow", s.window_shadow, t)
                            .debug_selector(|| "window-shadow".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.window_shadow = !s.window_shadow)),
                        t,
                    )),
            )
            .child(
                card(t).flex().flex_col().child(row(
                    "Welcome guide",
                    Some("See the first-run walkthrough again"),
                    button("show-welcome", "Show", t, Kind::Secondary)
                        .debug_selector(|| "show-welcome".into())
                        .on_click(|_, _, cx| dispatch(Command::Onboarding, cx)),
                    t,
                )),
            )
    }

    fn shortcuts(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let st = state(cx);
        let hotkeys = st.settings.hotkeys.clone();
        let failures = st.hotkey_failures.clone();
        let mut list = card(t).flex().flex_col();
        for (i, action) in HotkeyAction::ALL.into_iter().enumerate() {
            if i > 0 {
                list = list.child(divider(t));
            }
            let recording = self.recording == Some(action);
            let current = hotkeys.get(action).and_then(|s| Hotkey::parse(s)).map(|h| h.symbols());
            let label = if recording {
                "Type a shortcut…".to_string()
            } else {
                current.clone().unwrap_or_else(|| "Record shortcut".into())
            };
            let failed = failures.contains(&action);
            let id: SharedString = format!("hotkey-{i}").into();
            let sel = id.to_string();
            let control = div()
                .id(id)
                .debug_selector(move || sel.clone())
                .min_w(px(140.))
                .h(px(28.))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .border_1()
                .border_color(if recording { t.accent } else { t.border })
                .bg(if recording { t.selected } else { t.panel })
                .text_color(if current.is_some() || recording {
                    t.text
                } else {
                    t.text_faint
                })
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| this.start_recording(action, window, cx)))
                .child(label);
            list = list.child(row(
                action.label(),
                failed.then_some("Another app already uses this shortcut"),
                control,
                t,
            ));
        }
        let gesture = st.settings.menu_gesture;
        let trusted = platform(cx).has_accessibility();
        let gesture_labels: Vec<SharedString> = MenuGesture::ALL.iter().map(|g| g.label().into()).collect();
        let gesture_idx = MenuGesture::ALL.iter().position(|g| *g == gesture).unwrap_or(0);
        let gesture_card = card(t)
            .flex()
            .flex_col()
            .child(row(
                "Open the capture menu",
                Some("Press both keys together, then a number. Recommended: other apps can take a key chord, but not a pair of modifiers."),
                segmented("gesture", &gesture_labels, gesture_idx, t, |i, _, cx| {
                    update_settings(cx, |s| s.menu_gesture = MenuGesture::ALL[i]);
                }),
                t,
            ))
            .when(gesture != MenuGesture::Off && !trusted, |d| {
                d.child(divider(t)).child(row(
                    "Needs Accessibility",
                    Some("macOS only tells apps which modifier keys are down once they are allowed under Accessibility."),
                    button("allow-accessibility", "Allow…", t, Kind::Primary)
                        .debug_selector(|| "allow-accessibility".into())
                        .on_click(|_, _, cx| crate::permission_guide::ask(crate::permission_guide::Permission::Accessibility, cx)),
                    t,
                ))
            });
        div().flex().flex_col().gap(px(12.))
            .child(gesture_card)
            .child(div().text_color(t.text_muted).child("Click a shortcut and type the new keys. Backspace clears it, Esc cancels. ⌘⇧3–5 belong to macOS unless you turn them off in System Settings."))
            .child(list)
            .child(div().flex().child(button("reset-hotkeys", "Restore defaults", t, Kind::Secondary).debug_selector(|| "reset-hotkeys".into()).on_click(|_, _, cx| {
                update_settings(cx, |s| s.hotkeys = Hotkeys::default());
            })))
    }

    fn saving(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let s = state(cx).settings.clone();
        let folder = state(cx).save_folder();
        let formats = [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::WebP,
            ImageFormat::Heic,
            ImageFormat::Avif,
        ];
        let fmt_idx = formats.iter().position(|f| *f == s.file_format).unwrap_or(0);
        let preview = expand_template(&s.filename_template, LocalTime::now());
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                card(t)
                    .flex()
                    .flex_col()
                    .child(row(
                        "Folder",
                        Some(&folder.display().to_string()),
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(
                                button("choose-folder", "Choose…", t, Kind::Secondary)
                                    .debug_selector(|| "choose-folder".into())
                                    .on_click(|_, _, cx| choose_folder(cx)),
                            )
                            .child(
                                button("open-folder", "Open", t, Kind::Ghost)
                                    .debug_selector(|| "open-folder".into())
                                    .on_click(|_, _, cx| dispatch(Command::OpenSaveFolder, cx)),
                            ),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Format",
                        None,
                        segmented(
                            "save-format",
                            &formats.map(|f| SharedString::from(f.label())),
                            fmt_idx,
                            t,
                            move |i, _, cx| update_settings(cx, |s| s.file_format = formats[i]),
                        ),
                        t,
                    ))
                    .when(s.file_format.is_lossy(), |d| {
                        d.child(divider(t)).child(row(
                            format!("Quality {}%", (s.save_quality * 100.0).round()),
                            None,
                            div()
                                .w(px(200.))
                                .child(slider("save-quality", s.save_quality, t, |v, _, cx| {
                                    update_settings(cx, |s| s.save_quality = (v * 100.0).round() / 100.0)
                                })),
                            t,
                        ))
                    })
                    .child(divider(t))
                    .child(row(
                        "File name",
                        Some(&format!("Next file: {preview}")),
                        self.template.clone(),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Shrink PNGs",
                        Some("Lossless recompression, usually 20–50% smaller"),
                        toggle("shrink-png", s.shrink_png, t)
                            .debug_selector(|| "shrink-png".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.shrink_png = !s.shrink_png)),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Save Retina at 1×",
                        Some("Halve the pixels of Retina captures when saving"),
                        toggle("downscale", s.downscale_retina, t)
                            .debug_selector(|| "downscale".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.downscale_retina = !s.downscale_retina)),
                        t,
                    )),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_faint)
                    .child("Name tokens: {date} {time} {year} {month} {day} {hour} {minute} {second}"),
            )
    }

    fn converter(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let c = state(cx).settings.converter.clone();
        let fmt_idx = ImageFormat::WRITABLE.iter().position(|f| *f == c.format).unwrap_or(0);
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .text_color(t.text_muted)
                    .child("Defaults for the converter. Changes you make there are remembered here too."),
            )
            .child(
                card(t)
                    .flex()
                    .flex_col()
                    .child(row(
                        "Format",
                        None,
                        segmented(
                            "conv-format",
                            &ImageFormat::WRITABLE.map(|f| SharedString::from(f.label())),
                            fmt_idx,
                            t,
                            |i, _, cx| update_settings(cx, |s| s.converter.format = ImageFormat::WRITABLE[i]),
                        ),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        format!("Quality {}%", (c.quality * 100.0).round()),
                        None,
                        div()
                            .w(px(200.))
                            .child(slider("conv-quality", c.quality, t, |v, _, cx| {
                                update_settings(cx, |s| s.converter.quality = (v * 100.0).round() / 100.0)
                            })),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Remove metadata",
                        None,
                        toggle("conv-strip", c.strip_metadata, t)
                            .debug_selector(|| "conv-strip".into())
                            .on_click(|_, _, cx| {
                                update_settings(cx, |s| s.converter.strip_metadata = !s.converter.strip_metadata)
                            }),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Save to",
                        None,
                        segmented(
                            "conv-output",
                            &["Next to original".into(), "OShot folder".into()],
                            (c.output == OutputLocation::SaveFolder) as usize,
                            t,
                            |i, _, cx| {
                                update_settings(cx, |s| {
                                    s.converter.output = if i == 1 {
                                        OutputLocation::SaveFolder
                                    } else {
                                        OutputLocation::NextToOriginal
                                    }
                                })
                            },
                        ),
                        t,
                    )),
            )
            .child(
                div().flex().child(
                    button("open-converter", "Open Converter", t, Kind::Primary)
                        .debug_selector(|| "open-converter".into())
                        .on_click(|_, _, cx| dispatch(Command::ConvertImage, cx)),
                ),
            )
    }

    fn organize(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let s = state(cx).settings.clone();
        let source = platform(cx).system_screenshot_folder();
        let days = [None, Some(7u32), Some(30), Some(90)];
        let days_idx = days.iter().position(|d| *d == s.auto_clean_days).unwrap_or(0);
        let counts = [5u32, 10, 20];
        let count_idx = counts.iter().position(|c| *c == s.recent_limit).unwrap_or(1);
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                card(t)
                    .flex()
                    .flex_col()
                    .child(row(
                        "Tidy macOS screenshots",
                        Some(&format!(
                            "Move new screenshots from {} into your OShot folder",
                            source.display()
                        )),
                        toggle("tidy", s.tidy_desktop, t)
                            .debug_selector(|| "tidy".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.tidy_desktop = !s.tidy_desktop)),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Convert while tidying",
                        Some("Use the converter format for moved screenshots"),
                        toggle("tidy-convert", s.tidy_convert, t)
                            .debug_selector(|| "tidy-convert".into())
                            .on_click(|_, _, cx| update_settings(cx, |s| s.tidy_convert = !s.tidy_convert)),
                        t,
                    )),
            )
            .child(
                card(t)
                    .flex()
                    .flex_col()
                    .child(row(
                        "Delete old captures",
                        Some("Only files OShot named, in your OShot folder"),
                        segmented(
                            "auto-clean",
                            &["Never".into(), "7 days".into(), "30 days".into(), "90 days".into()],
                            days_idx,
                            t,
                            move |i, _, cx| {
                                update_settings(cx, |s| s.auto_clean_days = days[i]);
                                crate::organize::auto_clean(cx);
                            },
                        ),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Recent captures in menu",
                        None,
                        segmented(
                            "recent-limit",
                            &["5".into(), "10".into(), "20".into()],
                            count_idx,
                            t,
                            move |i, _, cx| update_settings(cx, |s| s.recent_limit = counts[i]),
                        ),
                        t,
                    ))
                    .child(divider(t))
                    .child(row(
                        "Clear recent captures",
                        None,
                        button("clear-recent", "Clear", t, Kind::Danger)
                            .debug_selector(|| "clear-recent".into())
                            .on_click(|_, _, cx| {
                                let dir = state(cx).paths.recent.clone();
                                let _ = std::fs::remove_dir_all(dir);
                                crate::commands::rebuild_menu(cx);
                                notify(cx, Notice::Info("Recent captures cleared".into()));
                            }),
                        t,
                    )),
            )
    }

    fn about(&self, t: &Theme) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .pt(px(30.))
            .child(img("brand/mark.svg").size(px(72.)))
            .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child("OShot"))
            .child(
                div()
                    .text_color(t.text_muted)
                    .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
            )
            .child(
                div()
                    .text_color(t.text_muted)
                    .child("Free and open source under the MIT License."),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_faint)
                    .child("Built with GPUI. Icons by Lucide (ISC)."),
            )
    }
}

pub fn keystroke_string(k: &gpui::Keystroke) -> String {
    let m = k.modifiers;
    let mut s = String::new();
    for (on, name) in [
        (m.control, "ctrl-"),
        (m.alt, "alt-"),
        (m.shift, "shift-"),
        (m.platform, "cmd-"),
    ] {
        if on {
            s.push_str(name);
        }
    }
    s.push_str(&k.key);
    s
}

fn choose_folder(cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Choose".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = rx.await
            && let Some(p) = paths.into_iter().next()
        {
            cx.update(|cx| update_settings(cx, |s| s.save_folder = Some(p)));
        }
    })
    .detach();
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let page = match self.page {
            Page::General => self.general(&t, cx),
            Page::Capture => self.capture(&t, cx),
            Page::Shortcuts => self.shortcuts(&t, cx),
            Page::Saving => self.saving(&t, cx),
            Page::Converter => self.converter(&t, cx),
            Page::Organize => self.organize(&t, cx),
            Page::About => self.about(&t),
        };
        let _ = cx.global::<AppState>();
        glass_root(&t)
            .id("settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .flex_row()
            .child(
                div()
                    .w(px(196.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .pt(px(52.))
                    .px(px(10.))
                    .border_r_1()
                    .border_color(t.border)
                    .children(Page::ALL.into_iter().map(|p| {
                        let active = p == self.page;
                        let id: SharedString = format!("nav-{}", p.title().to_lowercase()).into();
                        let sel = id.to_string();
                        div()
                            .id(id)
                            .debug_selector(move || sel.clone())
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .h(px(32.))
                            .px(px(10.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .when(active, |d| d.bg(t.accent).text_color(t.on_accent))
                            .when(!active, |d| d.hover(|s| s.bg(t.hover)))
                            .on_click(cx.listener(move |this, _, _, cx| this.set_page(p, cx)))
                            .child(icon(p.icon()).text_color(if active { t.on_accent } else { t.text_muted }))
                            .child(div().font_weight(FontWeight::MEDIUM).child(p.title()))
                    })),
            )
            .child(
                div()
                    .id("settings-page")
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .pt(px(48.))
                    .px(px(28.))
                    .pb(px(24.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::BOLD)
                            .child(self.page.title()),
                    )
                    .child(page)
                    .child(div().flex().justify_end().child(kbd("⌘W to close", &t))),
            )
    }
}

pub fn open(cx: &mut App) {
    open_at(Page::General, cx);
}

pub fn open_at(page: Page, cx: &mut App) {
    let handle = windows::find::<SettingsWindow>(cx).or_else(|| {
        windows::open(Spec::window("Settings", 820.0, 640.0), cx, |_, cx| {
            cx.new(SettingsWindow::new)
        })
    });
    if let Some(handle) = handle {
        let _ = handle.update(cx, |s, window, cx| {
            s.set_page(page, cx);
            window.activate_window();
            s.focus.focus(window, cx);
        });
        cx.activate(true);
    }
}
