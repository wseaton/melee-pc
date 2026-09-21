use egui::{
    Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, pos2, vec2,
};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RawEvent {
    pub kind: u32,
    pub x: f32,
    pub y: f32,
    pub button: u32,
    pub pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    PointerMoved(Pos2),
    PointerButton {
        pos: Pos2,
        button: PointerButton,
        pressed: bool,
    },
    PointerGone,
    Scroll(Vec2),
    Nav(NavAction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavAction {
    Up,
    Down,
    Left,
    Right,
    Activate,
    Back,
}

impl NavAction {
    fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Up),
            1 => Some(Self::Down),
            2 => Some(Self::Left),
            3 => Some(Self::Right),
            4 => Some(Self::Activate),
            5 => Some(Self::Back),
            _ => None,
        }
    }

    pub fn key(self, has_focus: bool) -> Option<Key> {
        match self {
            Self::Back => None,
            Self::Activate => Some(Key::Enter),
            _ if !has_focus => Some(Key::Tab),
            Self::Up => Some(Key::ArrowUp),
            Self::Down => Some(Key::ArrowDown),
            Self::Left => Some(Key::ArrowLeft),
            Self::Right => Some(Key::ArrowRight),
        }
    }
}

pub fn key_tap(key: Key) -> [Event; 2] {
    [true, false].map(|pressed| Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    })
}

impl InputEvent {
    pub fn from_raw(raw: &RawEvent) -> Option<Self> {
        match raw.kind {
            0 => Some(Self::PointerMoved(pos2(raw.x, raw.y))),
            1 => Some(Self::PointerButton {
                pos: pos2(raw.x, raw.y),
                button: button_from_sdl(raw.button)?,
                pressed: raw.pressed,
            }),
            2 => Some(Self::PointerGone),
            3 => Some(Self::Scroll(vec2(raw.x, raw.y))),
            4 => Some(Self::Nav(NavAction::from_raw(raw.button)?)),
            _ => None,
        }
    }

    pub fn into_egui(self) -> Option<Event> {
        Some(match self {
            Self::Nav(_) => return None,
            Self::PointerMoved(pos) => Event::PointerMoved(pos),
            Self::PointerButton {
                pos,
                button,
                pressed,
            } => Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            },
            Self::PointerGone => Event::PointerGone,
            Self::Scroll(delta) => Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta,
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        })
    }
}

fn button_from_sdl(button: u32) -> Option<PointerButton> {
    match button {
        1 => Some(PointerButton::Primary),
        2 => Some(PointerButton::Middle),
        3 => Some(PointerButton::Secondary),
        4 => Some(PointerButton::Extra1),
        5 => Some(PointerButton::Extra2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, PointerButton, pos2, vec2};

    use crate::input::{InputEvent, NavAction, RawEvent, key_tap};

    fn raw(kind: u32, x: f32, y: f32, button: u32, pressed: bool) -> RawEvent {
        RawEvent {
            kind,
            x,
            y,
            button,
            pressed,
        }
    }

    #[test]
    fn pointer_moved() {
        assert_eq!(
            InputEvent::from_raw(&raw(0, 3.0, 4.0, 0, false)),
            Some(InputEvent::PointerMoved(pos2(3.0, 4.0)))
        );
    }

    #[test]
    fn pointer_buttons_follow_sdl_numbering() {
        let expected = [
            (1, PointerButton::Primary),
            (2, PointerButton::Middle),
            (3, PointerButton::Secondary),
            (4, PointerButton::Extra1),
            (5, PointerButton::Extra2),
        ];
        for (sdl, button) in expected {
            assert_eq!(
                InputEvent::from_raw(&raw(1, 1.0, 2.0, sdl, true)),
                Some(InputEvent::PointerButton {
                    pos: pos2(1.0, 2.0),
                    button,
                    pressed: true
                })
            );
        }
    }

    #[test]
    fn unknown_button_and_kind_are_dropped() {
        assert_eq!(InputEvent::from_raw(&raw(1, 0.0, 0.0, 0, true)), None);
        assert_eq!(InputEvent::from_raw(&raw(1, 0.0, 0.0, 6, true)), None);
        assert_eq!(InputEvent::from_raw(&raw(5, 0.0, 0.0, 0, false)), None);
    }

    #[test]
    fn gone_and_scroll() {
        assert_eq!(
            InputEvent::from_raw(&raw(2, 9.0, 9.0, 0, false)),
            Some(InputEvent::PointerGone)
        );
        assert_eq!(
            InputEvent::from_raw(&raw(3, 0.0, -1.0, 0, false)),
            Some(InputEvent::Scroll(vec2(0.0, -1.0)))
        );
    }

    #[test]
    fn release_converts_to_egui_event() {
        let event = InputEvent::PointerButton {
            pos: pos2(5.0, 6.0),
            button: PointerButton::Primary,
            pressed: false,
        };
        assert!(matches!(
            event.into_egui(),
            Some(Event::PointerButton {
                pressed: false,
                button: PointerButton::Primary,
                ..
            })
        ));
    }

    #[test]
    fn nav_codes_map_in_order() {
        let expected = [
            NavAction::Up,
            NavAction::Down,
            NavAction::Left,
            NavAction::Right,
            NavAction::Activate,
            NavAction::Back,
        ];
        for (code, action) in expected.into_iter().enumerate() {
            assert_eq!(
                InputEvent::from_raw(&raw(4, 0.0, 0.0, code as u32, true)),
                Some(InputEvent::Nav(action))
            );
        }
        assert_eq!(InputEvent::from_raw(&raw(4, 0.0, 0.0, 6, true)), None);
    }

    #[test]
    fn nav_is_not_a_direct_egui_event() {
        assert_eq!(InputEvent::Nav(NavAction::Up).into_egui(), None);
    }

    #[test]
    fn nav_keys_with_focus() {
        assert_eq!(NavAction::Up.key(true), Some(Key::ArrowUp));
        assert_eq!(NavAction::Down.key(true), Some(Key::ArrowDown));
        assert_eq!(NavAction::Left.key(true), Some(Key::ArrowLeft));
        assert_eq!(NavAction::Right.key(true), Some(Key::ArrowRight));
        assert_eq!(NavAction::Activate.key(true), Some(Key::Enter));
        assert_eq!(NavAction::Back.key(true), None);
    }

    #[test]
    fn directions_tab_into_the_ui_without_focus() {
        for action in [
            NavAction::Up,
            NavAction::Down,
            NavAction::Left,
            NavAction::Right,
        ] {
            assert_eq!(action.key(false), Some(Key::Tab));
        }
        assert_eq!(NavAction::Activate.key(false), Some(Key::Enter));
        assert_eq!(NavAction::Back.key(false), None);
    }

    #[test]
    fn key_tap_presses_then_releases() {
        let [down, up] = key_tap(Key::Enter);
        assert!(matches!(
            down,
            Event::Key {
                key: Key::Enter,
                pressed: true,
                repeat: false,
                ..
            }
        ));
        assert!(matches!(
            up,
            Event::Key {
                key: Key::Enter,
                pressed: false,
                ..
            }
        ));
    }
}
