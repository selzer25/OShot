//! The capture menu's modifier gesture: both Command keys (or both Control
//! keys) pressed together, with nothing else held.
//!
//! macOS reports which physical modifier keys are down as device-dependent
//! bits in every key event's modifier flags; this turns that stream into a
//! single "fire" per press.

use serde::{Deserialize, Serialize};

pub const LEFT_CONTROL: u64 = 0x0001;
pub const LEFT_SHIFT: u64 = 0x0002;
pub const RIGHT_SHIFT: u64 = 0x0004;
pub const LEFT_COMMAND: u64 = 0x0008;
pub const RIGHT_COMMAND: u64 = 0x0010;
pub const LEFT_OPTION: u64 = 0x0020;
pub const RIGHT_OPTION: u64 = 0x0040;
pub const RIGHT_CONTROL: u64 = 0x2000;

const DEVICE_KEYS: u64 =
    LEFT_CONTROL | LEFT_SHIFT | RIGHT_SHIFT | LEFT_COMMAND | RIGHT_COMMAND | LEFT_OPTION | RIGHT_OPTION | RIGHT_CONTROL;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MenuGesture {
    BothCommand,
    BothControl,
    Off,
}

impl MenuGesture {
    pub const ALL: [MenuGesture; 3] = [MenuGesture::BothCommand, MenuGesture::BothControl, MenuGesture::Off];

    pub fn label(self) -> &'static str {
        match self {
            MenuGesture::BothCommand => "⌘ + ⌘",
            MenuGesture::BothControl => "⌃ + ⌃",
            MenuGesture::Off => "Off",
        }
    }

    /// The left and right key bits, or `None` when off.
    fn keys(self) -> Option<(u64, u64)> {
        match self {
            MenuGesture::BothCommand => Some((LEFT_COMMAND, RIGHT_COMMAND)),
            MenuGesture::BothControl => Some((LEFT_CONTROL, RIGHT_CONTROL)),
            MenuGesture::Off => None,
        }
    }
}

/// Fires once when both keys of the pair go down with no other modifier.
/// It re-arms only after both keys are up again, and a normal key typed
/// while one of them is held (⌘C, ⌘Tab…) disarms it until then, so
/// ordinary shortcuts never open the menu.
#[derive(Clone, Debug)]
pub struct GestureDetector {
    gesture: MenuGesture,
    armed: bool,
}

impl GestureDetector {
    pub fn new(gesture: MenuGesture) -> GestureDetector {
        GestureDetector { gesture, armed: true }
    }

    /// Feed the modifier flags of a flags-changed event. Returns true when
    /// the gesture completes.
    pub fn flags_changed(&mut self, flags: u64) -> bool {
        let Some((left, right)) = self.gesture.keys() else {
            return false;
        };
        let pair = left | right;
        let held = flags & DEVICE_KEYS;
        if held & pair == 0 {
            self.armed = true;
            return false;
        }
        if self.armed && held == pair {
            self.armed = false;
            return true;
        }
        if held & !pair != 0 {
            self.armed = false;
        }
        false
    }

    /// Feed a key-down event with its modifier flags.
    pub fn key_down(&mut self, flags: u64) {
        if let Some((left, right)) = self.gesture.keys()
            && flags & (left | right) != 0
        {
            self.armed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(gesture: MenuGesture, steps: &[u64]) -> usize {
        let mut d = GestureDetector::new(gesture);
        steps.iter().filter(|f| d.flags_changed(**f)).count()
    }

    #[test]
    fn both_command_keys_fire_once_in_either_order() {
        let g = MenuGesture::BothCommand;
        assert_eq!(
            run(g, &[LEFT_COMMAND, LEFT_COMMAND | RIGHT_COMMAND, RIGHT_COMMAND, 0]),
            1
        );
        assert_eq!(run(g, &[RIGHT_COMMAND, LEFT_COMMAND | RIGHT_COMMAND]), 1);
    }

    #[test]
    fn holding_does_not_repeat_and_release_rearms() {
        let both = LEFT_COMMAND | RIGHT_COMMAND;
        let steps = [LEFT_COMMAND, both, LEFT_COMMAND, both, 0, RIGHT_COMMAND, both];
        assert_eq!(
            run(MenuGesture::BothCommand, &steps),
            2,
            "second fire needs a full release"
        );
    }

    #[test]
    fn extra_modifiers_or_one_key_do_not_fire() {
        let g = MenuGesture::BothCommand;
        assert_eq!(run(g, &[LEFT_COMMAND, 0, RIGHT_COMMAND, 0]), 0);
        assert_eq!(
            run(
                g,
                &[
                    LEFT_SHIFT,
                    LEFT_SHIFT | LEFT_COMMAND,
                    LEFT_SHIFT | LEFT_COMMAND | RIGHT_COMMAND
                ]
            ),
            0
        );
        assert_eq!(
            run(
                g,
                &[LEFT_COMMAND | LEFT_OPTION, LEFT_COMMAND, LEFT_COMMAND | RIGHT_COMMAND]
            ),
            0,
            "an Option press while Command was held disarms"
        );
    }

    #[test]
    fn a_shortcut_typed_in_between_disarms() {
        let mut d = GestureDetector::new(MenuGesture::BothCommand);
        assert!(!d.flags_changed(LEFT_COMMAND));
        d.key_down(LEFT_COMMAND);
        assert!(!d.flags_changed(LEFT_COMMAND | RIGHT_COMMAND));
        assert!(!d.flags_changed(0));
        assert!(!d.flags_changed(LEFT_COMMAND));
        assert!(
            d.flags_changed(LEFT_COMMAND | RIGHT_COMMAND),
            "works again after release"
        );
    }

    #[test]
    fn control_pair_and_off() {
        assert_eq!(
            run(MenuGesture::BothControl, &[LEFT_CONTROL, LEFT_CONTROL | RIGHT_CONTROL]),
            1
        );
        assert_eq!(
            run(MenuGesture::BothControl, &[LEFT_COMMAND, LEFT_COMMAND | RIGHT_COMMAND]),
            0
        );
        assert_eq!(run(MenuGesture::Off, &[LEFT_COMMAND, LEFT_COMMAND | RIGHT_COMMAND]), 0);
    }

    #[test]
    fn device_independent_bits_are_ignored() {
        const COMMAND_MASK: u64 = 1 << 20;
        let both = LEFT_COMMAND | RIGHT_COMMAND | COMMAND_MASK;
        assert_eq!(run(MenuGesture::BothCommand, &[LEFT_COMMAND | COMMAND_MASK, both]), 1);
    }
}
