//! First run, on one page: a short welcome, then a checklist on the left
//! (Screen Recording, Accessibility, a practice run of the capture menu and
//! the after-capture choice) with the item in focus on a card to the right.
//! Finished items tick themselves off and the next one slides in.

use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt, App, AppContext, Context, Div, ElementId, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, WindowControlArea, div, img, prelude::FluentBuilder, px,
};
use oshot_core::gesture::MenuGesture;
use oshot_core::settings::{AfterCapture, Hotkey, HotkeyAction};

use crate::art;
use crate::doodle::{Doodle, doodle};
use crate::hud;
use crate::permission_guide::{self, Permission};
use crate::state::{platform, state, update_settings};
use crate::theme::{Paper, Theme, UI_FONT, theme};
use crate::ui::{TITLEBAR_H, icon, listen, segmented};
use crate::windows::{self, Spec};

pub const WIDTH: f32 = 880.;
pub const HEIGHT: f32 = 620.;
/// How long a finished item stays on the card, tick drawn, before the next
/// one comes in.
const ADVANCE_AFTER: Duration = Duration::from_millis(1300);
const POLL: Duration = Duration::from_secs(1);
const INTRO_EXIT: Duration = Duration::from_millis(420);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    ScreenRecording,
    Accessibility,
    Practice,
    AfterCapture,
}

impl Item {
    pub const ALL: [Item; 4] = [
        Item::ScreenRecording,
        Item::Accessibility,
        Item::Practice,
        Item::AfterCapture,
    ];

    fn index(self) -> usize {
        self as usize
    }

    fn slug(self) -> &'static str {
        match self {
            Item::ScreenRecording => "row-screen",
            Item::Accessibility => "row-accessibility",
            Item::Practice => "row-practice",
            Item::AfterCapture => "row-after",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Practice {
    /// Waiting for the gesture (or the menu shortcut).
    Waiting,
    /// The menu is "open" on the card; a number key picks a mode.
    Open,
    /// Picked the mode at this index.
    Done(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Todo,
    Active,
    Done,
}

pub struct Onboarding {
    /// The welcome screen in front of the checklist.
    pub intro: bool,
    intro_leaving: bool,
    opened_at: Instant,
    /// Pinned by a click, or by a just-finished item while its tick shows.
    pub focus: Option<Item>,
    pub practice: Practice,
    /// Set once the user picks an after-capture option here.
    pub after_chosen: bool,
    /// Asked macOS for this permission; waiting on System Settings.
    pub waiting: Option<Item>,
    /// Permissions as last seen: screen, accessibility.
    granted: (bool, bool),
    done: [bool; 4],
    marks: [(Mark, Instant); 4],
    stage: (Option<Item>, Instant),
    stage_revision: usize,
    focus_handle: FocusHandle,
    advance: Option<Task<()>>,
    _poll: Task<()>,
    _intro_exit: Option<Task<()>>,
    _activation: Option<Subscription>,
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Fades `el` in while it rises 10 pt, starting `delay` ms after it mounts.
fn rise(el: Div, id: impl Into<ElementId>, delay: u32, ms: u32) -> impl IntoElement {
    let total = (delay + ms) as f32;
    el.with_animation(id, Animation::new(Duration::from_millis(total as u64)), move |el, t| {
        let e = ease_out((t * total - delay as f32) / ms as f32);
        el.relative().top(px(10.0 * (1.0 - e))).opacity(e)
    })
}

fn pill(id: &'static str, label: impl Into<SharedString>, p: &Paper, primary: bool) -> Stateful<Div> {
    let (bg, fg, border) = if primary {
        (p.ink, p.bg, p.ink)
    } else {
        (gpui::transparent_black(), p.ink, p.track)
    };
    let hover = p.soft;
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(42.))
        .px(px(22.))
        .rounded_full()
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| {
            if primary {
                s.opacity(0.86)
            } else {
                s.border_color(hover)
            }
        })
        .child(label.into())
}

fn title(text: impl Into<SharedString>, p: &Paper) -> Div {
    div()
        .mt(px(18.))
        .text_size(px(22.))
        .line_height(px(28.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.ink)
        .text_center()
        .child(text.into())
}

fn body(text: impl Into<SharedString>, p: &Paper) -> Div {
    div()
        .mt(px(8.))
        .max_w(px(360.))
        .text_size(px(14.))
        .line_height(px(21.))
        .text_color(p.soft)
        .text_center()
        .child(text.into())
}

fn keycap(symbol: &'static str, lit: bool, p: &Paper) -> Div {
    div()
        .size(px(46.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(11.))
        .border_1()
        .border_color(if lit { p.accent } else { p.track })
        .bg(if lit { p.accent } else { p.card })
        .text_size(px(22.))
        .text_color(if lit { p.bg } else { p.ink })
        .child(symbol)
}

fn menu_shortcut(cx: &App) -> String {
    state(cx)
        .settings
        .hotkeys
        .get(HotkeyAction::CaptureMenu)
        .and_then(|s| Hotkey::parse(s))
        .map(|h| h.symbols())
        .unwrap_or_else(|| "⌥⇧5".into())
}

impl Onboarding {
    pub fn new(intro: bool, window: &mut Window, cx: &mut Context<Self>) -> Onboarding {
        let now = Instant::now();
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        });
        let activation = cx.observe_window_activation(window, |this, _, cx| this.refresh(cx));
        let p = platform(cx);
        Onboarding {
            intro,
            intro_leaving: false,
            opened_at: now,
            focus: None,
            practice: Practice::Waiting,
            after_chosen: false,
            waiting: None,
            granted: (p.has_screen_permission(), p.has_accessibility()),
            done: [false; 4],
            marks: [(Mark::Todo, now); 4],
            stage: (None, now),
            stage_revision: 0,
            focus_handle: cx.focus_handle(),
            advance: None,
            _poll: poll,
            _intro_exit: None,
            _activation: Some(activation),
        }
    }

    /// Re-reads the permissions; called every second and on activation, so
    /// a grant made in System Settings shows up without a click.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let p = platform(cx);
        let now = (p.has_screen_permission(), p.has_accessibility());
        if now == self.granted {
            return;
        }
        if now.1 && !self.granted.1 {
            // Event monitors installed before the grant see no other app's keys.
            crate::commands::apply_gesture(cx);
        }
        self.granted = now;
        cx.notify();
    }

    fn gesture(cx: &App) -> MenuGesture {
        state(cx).settings.menu_gesture
    }

    fn is_done(&self, item: Item, cx: &App) -> bool {
        match item {
            Item::ScreenRecording => self.granted.0,
            Item::Accessibility => self.granted.1 || Self::gesture(cx) == MenuGesture::Off,
            Item::Practice => matches!(self.practice, Practice::Done(_)),
            Item::AfterCapture => self.after_chosen,
        }
    }

    /// The item on the card: the pinned one, else the first not done.
    /// `None` once everything is done.
    pub fn current(&self, cx: &App) -> Option<Item> {
        self.focus
            .or_else(|| Item::ALL.into_iter().find(|i| !self.is_done(*i, cx)))
    }

    /// Reconciles ticks, focus and entrance clocks with the latest state.
    /// Runs at the start of every render, so every path that changes state
    /// is covered.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        for item in Item::ALL {
            let done = self.is_done(item, cx);
            if done && !self.done[item.index()] {
                if self.waiting == Some(item) {
                    self.waiting = None;
                }
                if self.stage.0 == Some(item) {
                    self.pin_briefly(item, cx);
                }
            }
            self.done[item.index()] = done;
        }
        let current = self.current(cx);
        if self.stage.0 != current {
            self.stage = (current, now);
            self.stage_revision += 1;
        }
        for item in Item::ALL {
            let mark = if self.done[item.index()] {
                Mark::Done
            } else if current == Some(item) {
                Mark::Active
            } else {
                Mark::Todo
            };
            if self.marks[item.index()].0 != mark {
                self.marks[item.index()] = (mark, now);
            }
        }
    }

    /// Keeps a just-finished item on the card long enough to see its tick.
    fn pin_briefly(&mut self, item: Item, cx: &mut Context<Self>) {
        self.focus = Some(item);
        self.advance = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ADVANCE_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                if this.focus == Some(item) {
                    this.focus = None;
                }
                cx.notify();
            });
        }));
    }

    pub fn focus_item(&mut self, item: Item, cx: &mut Context<Self>) {
        self.advance = None;
        self.focus = Some(item);
        cx.notify();
    }

    pub fn continue_from_intro(&mut self, cx: &mut Context<Self>) {
        if !self.intro || self.intro_leaving {
            return;
        }
        self.intro_leaving = true;
        self._intro_exit = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(INTRO_EXIT).await;
            let _ = this.update(cx, |this, cx| {
                this.intro = false;
                this.intro_leaving = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn ask(&mut self, item: Item, cx: &mut Context<Self>) {
        let permission = match item {
            Item::ScreenRecording => Permission::ScreenRecording,
            Item::Accessibility => Permission::Accessibility,
            _ => return,
        };
        self.waiting = Some(item);
        permission_guide::ask(permission, cx);
        cx.notify();
    }

    /// The capture menu was summoned. Taken over while its practice is on
    /// the card, so the real menu does not pop up over the onboarding.
    pub fn take_menu(&mut self, cx: &mut Context<Self>) -> bool {
        if self.intro || self.current(cx) != Some(Item::Practice) || self.practice != Practice::Waiting {
            return false;
        }
        self.practice = Practice::Open;
        cx.notify();
        true
    }

    fn on_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = e.keystroke.key.as_str();
        if self.intro {
            if matches!(key, "space" | "enter") {
                self.continue_from_intro(cx);
            }
            return;
        }
        if self.practice == Practice::Open {
            match key {
                "escape" => self.practice = Practice::Waiting,
                k => match k.parse::<usize>() {
                    Ok(n) if (1..=hud::items(3).len()).contains(&n) => self.practice = Practice::Done(n - 1),
                    _ => return,
                },
            }
            cx.notify();
        }
    }

    pub fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        update_settings(cx, |s| s.onboarding_done = true);
        permission_guide::stop(cx);
        window.remove_window();
    }

    fn row(&self, item: Item, p: &Paper, cx: &mut Context<Self>) -> impl IntoElement {
        let (name, caption) = match item {
            Item::ScreenRecording => (
                "Screen Recording",
                if self.granted.0 { "Allowed" } else { "Needed to capture" },
            ),
            Item::Accessibility => (
                "Accessibility",
                if self.granted.1 {
                    "Allowed"
                } else if Self::gesture(cx) == MenuGesture::Off {
                    "Not needed"
                } else {
                    "For the two-key menu"
                },
            ),
            Item::Practice => (
                "Try the capture menu",
                if self.done[item.index()] { "Done" } else { "Ten seconds" },
            ),
            Item::AfterCapture => ("After you capture", state(cx).settings.after_capture.label()),
        };
        let (mark, at) = self.marks[item.index()];
        let art = match mark {
            Mark::Todo => &art::MARK_TODO,
            Mark::Active => &art::MARK_ACTIVE,
            Mark::Done => &art::MARK_DONE,
        };
        let active = self.stage.0 == Some(item);
        let done = mark == Mark::Done;
        let card = p.card;
        let slug = item.slug();
        div()
            .id(slug)
            .debug_selector(move || slug.to_string())
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(54.))
            .px(px(10.))
            .rounded(px(14.))
            .border_1()
            .border_color(if active { p.ink } else { gpui::transparent_black() })
            .when(active, |d| d.bg(p.card))
            .cursor_pointer()
            .hover(move |s| s.bg(card))
            .on_click(cx.listener(move |this, _, _, cx| this.focus_item(item, cx)))
            .child(doodle(art, *p, at, 28., 28.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.ink)
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if done { p.green } else { p.soft })
                            .child(caption),
                    ),
            )
    }

    fn checklist(&self, p: &Paper, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(300.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .child(rise(div().child(img("brand/mark.svg").size(px(44.))), "logo", 0, 400))
            .child(rise(
                div()
                    .mt(px(14.))
                    .text_size(px(28.))
                    .line_height(px(34.))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Set up OShot"),
                "heading",
                60,
                400,
            ))
            .child(
                div()
                    .mt(px(2.))
                    .child(doodle(&art::UNDERLINE, *p, self.opened_at, 132., 12.)),
            )
            .child(rise(
                div()
                    .mt(px(10.))
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .text_color(p.soft)
                    .child("A minute of setup, then OShot lives in your menu bar. Nothing leaves your Mac."),
                "lede",
                120,
                400,
            ))
            .child(
                div().mt(px(22.)).flex().flex_col().gap(px(4.)).children(
                    Item::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(i, item)| rise(div().child(self.row(item, p, cx)), ("row", i), 180 + i as u32 * 70, 380))
                        .collect::<Vec<_>>(),
                ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        pill("finish", "Start using OShot", p, true)
                            .on_click(cx.listener(|this, _, window, cx| this.finish(window, cx))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(p.soft)
                            .child("All of this lives in Settings too."),
                    ),
            )
    }

    fn permission_card(&self, item: Item, p: &Paper, cx: &mut Context<Self>) -> Div {
        let shown = self.stage.1;
        let granted = self.done[item.index()];
        let screen = item == Item::ScreenRecording;
        let symbol = if Self::gesture(cx) == MenuGesture::BothControl {
            "⌃"
        } else {
            "⌘"
        };
        let art: &'static Doodle = if screen { &art::SCREEN } else { &art::KEYS };
        let picture = div()
            .relative()
            .child(doodle(art, *p, shown, 168., 146.))
            .when(!screen, |d| {
                // The symbols sit on the drawn key caps, centred at 29 and 91 of
                // the 120-wide drawing, which is drawn 1.4× here.
                d.children([29.0 * 1.4 - 22.0, 91.0 * 1.4 - 22.0].map(|left| {
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(56.))
                        .w(px(44.))
                        .text_center()
                        .text_size(px(26.))
                        .text_color(p.ink)
                        .child(symbol)
                }))
            });
        let card = div().flex().flex_col().items_center().child(picture);
        if granted {
            let (t, b) = if screen {
                (
                    "Screen Recording is on",
                    "OShot can capture now. It only looks when you ask it to.",
                )
            } else if self.granted.1 {
                (
                    "Accessibility is on",
                    "Both keys now open the capture menu, from any app.",
                )
            } else {
                (
                    "Not needed",
                    "The two-key gesture is off, so OShot does not need Accessibility.",
                )
            };
            return card.child(title(t, p)).child(body(b, p)).child(
                div()
                    .mt(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(40.))
                    .px(px(16.))
                    .rounded_full()
                    .border_1()
                    .border_color(p.green)
                    .text_color(p.green)
                    .child(doodle(&art::MARK_DONE, *p, self.marks[item.index()].1, 20., 20.))
                    .child("Allowed"),
            );
        }
        if self.waiting == Some(item) {
            let list = if screen { "Screen Recording" } else { "Accessibility" };
            return card
                .child(title("Switch on OShot", p))
                .child(body(
                    format!(
                        "In System Settings, turn on OShot under {list}. Not listed? Drag the OShot icon from the helper under the window into the list."
                    ),
                    p,
                ))
                .child(div().mt(px(18.)).child(doodle(&art::WAITING, *p, shown, 150., 15.)))
                .child(
                    div()
                        .mt(px(4.))
                        .text_size(px(12.))
                        .text_color(p.soft)
                        .child("Waiting for System Settings…"),
                )
                .child(
                    div().mt(px(16.)).child(
                        pill("reopen-settings", "Open System Settings again", p, false)
                            .on_click(cx.listener(move |this, _, _, cx| this.ask(item, cx))),
                    ),
                );
        }
        let (t, b, button, id) = if screen {
            (
                "Let OShot see your screen".to_string(),
                "macOS asks every screenshot app for Screen Recording once. OShot only looks when you capture, and nothing leaves your Mac.".to_string(),
                "Allow Screen Recording",
                "allow-screen",
            )
        } else {
            (
                format!("Open the menu with {symbol} {symbol}"),
                format!(
                    "Press both {} keys together to open the capture menu from anywhere. macOS shares which modifier keys are down only with apps allowed under Accessibility.",
                    if symbol == "⌃" { "Control" } else { "Command" }
                ),
                "Allow Accessibility",
                "allow-accessibility",
            )
        };
        card.child(title(t, p)).child(body(b, p)).child(
            div()
                .mt(px(22.))
                .child(pill(id, button, p, true).on_click(cx.listener(move |this, _, _, cx| this.ask(item, cx)))),
        )
    }

    fn practice_card(&self, p: &Paper, t: &Theme, cx: &mut Context<Self>) -> Div {
        let gesture = Self::gesture(cx);
        let symbol = if gesture == MenuGesture::BothControl {
            "⌃"
        } else {
            "⌘"
        };
        let shortcut = menu_shortcut(cx);
        let items = hud::items(3);
        let lit = self.practice != Practice::Waiting;
        let (heading, b) = match self.practice {
            Practice::Waiting if gesture == MenuGesture::Off => (
                format!("Try it: press {shortcut}"),
                "That opens the capture menu from any app.".to_string(),
            ),
            Practice::Waiting => (
                format!("Try it: press both {symbol} keys"),
                format!("Together, and let go. {shortcut} works too."),
            ),
            Practice::Open => ("Now press a number".into(), "Each mode has one. 1 is Area.".into()),
            Practice::Done(i) => (
                "That's the whole trick".into(),
                format!("{} starts {} capture. Use it from any app.", i + 1, items[i].2),
            ),
        };
        let bar = div()
            .mt(px(18.))
            .flex()
            .gap(px(2.))
            .p(px(4.))
            .rounded(px(14.))
            .border_1()
            .border_color(p.track)
            .bg(p.bg)
            .children(items.iter().enumerate().map(|(i, (_, name, _, _))| {
                let picked = self.practice == Practice::Done(i);
                div()
                    .w(px(44.))
                    .h(px(50.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .rounded(px(10.))
                    .when(picked, |d| d.bg(p.accent))
                    .child(icon(name).size(px(16.)).text_color(if picked { p.bg } else { p.ink }))
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(if picked { p.bg } else { p.soft })
                            .child(format!("{}", i + 1)),
                    )
            }));
        let needs_trust = gesture != MenuGesture::Off && !self.granted.1;
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(keycap(symbol, lit, p))
                    .child(div().text_size(px(18.)).text_color(p.soft).child("+"))
                    .child(keycap(symbol, lit, p)),
            )
            .child(title(heading, p))
            .child(body(b, p))
            .when(lit, |d| {
                d.child(rise(div().child(bar), ("practice-bar", self.stage_revision), 0, 260))
            })
            .when(needs_trust && !lit, |d| {
                d.child(div().mt(px(14.)).text_size(px(12.)).text_color(p.soft).child(format!(
                    "The two keys need Accessibility first. Until then, use {shortcut}."
                )))
            })
            .child(
                div()
                    .mt(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(px(12.)).text_color(p.soft).child("Keys"))
                    .child(segmented(
                        "practice-gesture",
                        &MenuGesture::ALL.map(|g| SharedString::from(g.label())),
                        MenuGesture::ALL.iter().position(|g| *g == gesture).unwrap_or(0),
                        t,
                        listen(cx, |this, i: usize, _, cx| {
                            update_settings(cx, |s| s.menu_gesture = MenuGesture::ALL[i]);
                            this.practice = Practice::Waiting;
                            cx.notify();
                        }),
                    )),
            )
    }

    fn after_card(&self, p: &Paper, cx: &mut Context<Self>) -> Div {
        let current = state(cx).settings.after_capture;
        let options: [(AfterCapture, &'static Doodle, &str); 3] = [
            (AfterCapture::Editor, &art::PEN, "Mark it up right away"),
            (AfterCapture::Thumbnail, &art::CORNER, "A preview in the corner"),
            (AfterCapture::ClipboardOnly, &art::CLIP, "Copy, nothing else"),
        ];
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(title("After you capture…", p))
            .child(body(
                "Pick what happens next. In the capture overlay, ⌘C always copies straight away.",
                p,
            ))
            .child(
                div()
                    .mt(px(22.))
                    .flex()
                    .gap(px(10.))
                    .children(options.into_iter().enumerate().map(|(i, (choice, art, sub))| {
                        let active = choice == current;
                        let id: &'static str = match choice {
                            AfterCapture::Editor => "after-open-editor",
                            AfterCapture::Thumbnail => "after-show-thumbnail",
                            AfterCapture::ClipboardOnly => "after-copy-only",
                        };
                        let hover = p.soft;
                        rise(
                            div().child(
                                div()
                                    .id(id)
                                    .debug_selector(move || id.to_string())
                                    .w(px(122.))
                                    .p(px(12.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(6.))
                                    .rounded(px(16.))
                                    .border_1()
                                    .border_color(if active { p.ink } else { p.track })
                                    .bg(if active { p.card } else { gpui::transparent_black() })
                                    .cursor_pointer()
                                    .hover(move |s| s.border_color(hover))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        update_settings(cx, |s| s.after_capture = choice);
                                        this.after_chosen = true;
                                        cx.notify();
                                    }))
                                    .child(doodle(art, *p, self.stage.1, 64., 56.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.ink)
                                            .child(choice.label()),
                                    )
                                    .child(div().text_size(px(11.)).text_color(p.soft).text_center().child(sub)),
                            ),
                            ("after", i),
                            80 + i as u32 * 70,
                            360,
                        )
                    })),
            )
    }

    fn all_set_card(&self, p: &Paper, cx: &App) -> Div {
        let how = match Self::gesture(cx) {
            MenuGesture::BothCommand => "both ⌘ keys".to_string(),
            MenuGesture::BothControl => "both ⌃ keys".to_string(),
            MenuGesture::Off => menu_shortcut(cx),
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(doodle(&art::ALL_SET, *p, self.stage.1, 168., 146.))
            .child(title("You're all set", p))
            .child(body(
                format!("OShot is in your menu bar. Press {how} any time, then a number."),
                p,
            ))
    }

    fn stage_card(&self, p: &Paper, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.stage.0 {
            Some(item @ (Item::ScreenRecording | Item::Accessibility)) => self.permission_card(item, p, cx),
            Some(Item::Practice) => self.practice_card(p, t, cx),
            Some(Item::AfterCapture) => self.after_card(p, cx),
            None => self.all_set_card(p, cx),
        };
        div()
            .id("stage")
            .debug_selector(|| "stage".into())
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(32.))
            .py(px(28.))
            .rounded(px(24.))
            .border_1()
            .border_color(p.track)
            .bg(p.card)
            .child(rise(content, ("stage", self.stage_revision), 0, 320))
    }

    fn intro(&self, p: &Paper, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = div()
            .absolute()
            .inset_0()
            .bg(p.bg)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(doodle(&art::APERTURE, *p, self.opened_at, 132., 132.))
            .child(rise(
                div()
                    .mt(px(22.))
                    .text_size(px(44.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("OShot"),
                "intro-name",
                900,
                500,
            ))
            .child(rise(
                div()
                    .mt(px(6.))
                    .text_size(px(17.))
                    .text_color(p.soft)
                    .child("Screenshots, marked up in a blink."),
                "intro-tagline",
                1150,
                500,
            ))
            .child(rise(
                div()
                    .mt(px(36.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        pill("get-started", "Get started", p, true)
                            .w(px(220.))
                            .on_click(cx.listener(|this, _, _, cx| this.continue_from_intro(cx))),
                    )
                    .child(div().text_size(px(11.)).text_color(p.soft).child("or press Space")),
                "intro-button",
                1450,
                450,
            ));
        if !self.intro_leaving {
            return screen.into_any_element();
        }
        let ms = INTRO_EXIT.as_millis() as u64;
        screen
            .with_animation("intro-leave", Animation::new(Duration::from_millis(ms)), |el, t| {
                let e = ease_out(t);
                el.top(px(-18.0 * e)).opacity(1.0 - e)
            })
            .into_any_element()
    }
}

impl Render for Onboarding {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(cx);
        if !self.focus_handle.is_focused(window) {
            self.focus_handle.focus(window, cx);
        }
        let t = theme(window, cx);
        let p = Paper::for_mode(t.mode);
        let page = !self.intro || self.intro_leaving;
        div()
            .id("onboarding")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.ink)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .child(
                div()
                    .id("onboarding-titlebar")
                    .h(px(TITLEBAR_H))
                    .flex_none()
                    .window_control_area(WindowControlArea::Drag),
            )
            .when(page, |d| {
                d.child(
                    div()
                        .id("onboarding-body")
                        .debug_selector(|| "window-body".into())
                        .flex_1()
                        .min_h(px(0.))
                        .flex()
                        .gap(px(36.))
                        .px(px(40.))
                        .pt(px(8.))
                        .pb(px(32.))
                        .child(self.checklist(&p, cx))
                        .child(self.stage_card(&p, &t, cx)),
                )
            })
            .when(self.intro, |d| d.child(self.intro(&p, cx)))
    }
}

/// Hands a capture-menu request to the onboarding's practice when that is
/// what the user is doing. Returns whether it was taken.
pub fn take_menu(cx: &mut App) -> bool {
    let Some(handle) = windows::find::<Onboarding>(cx) else {
        return false;
    };
    let taken = handle
        .update(cx, |o, window, cx| {
            let taken = o.take_menu(cx);
            if taken {
                window.activate_window();
            }
            taken
        })
        .unwrap_or(false);
    if taken {
        cx.activate(true);
    }
    taken
}

pub fn open(cx: &mut App) {
    show(true, None, cx);
}

/// Opens straight at Screen Recording, used when a capture is refused.
pub fn open_permission(cx: &mut App) {
    show(false, Some(Item::ScreenRecording), cx);
}

fn show(intro: bool, focus: Option<Item>, cx: &mut App) {
    let handle = windows::find::<Onboarding>(cx).or_else(|| {
        let mut spec = Spec::window("Welcome to OShot", WIDTH, HEIGHT);
        spec.resizable = false;
        spec.glass = None;
        windows::open(spec, cx, |window, cx| cx.new(|cx| Onboarding::new(intro, window, cx)))
    });
    if let Some(handle) = handle {
        let _ = handle.update(cx, |o, window, cx| {
            if focus.is_some() {
                o.intro = false;
                o.focus = focus;
            }
            window.activate_window();
            cx.notify();
        });
        cx.activate(true);
    }
}
