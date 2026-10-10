//! Pointer input in the map's terms. The canvas turns the frame's pointer events into gestures
//! that carry the position on screen, in the map's pixels and in cells, the button and the
//! modifiers held, so a tool is a state machine over gestures. Every pointer move is handled,
//! so a fast stroke skips no cell, and a release the canvas never saw (the button let go over
//! another window) is made up when the button is found up.

use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use glam::{DVec2, IVec2};

/// How far the pointer must move from a press, in points, before it is a drag rather than a
/// click.
const DRAG_DISTANCE: f32 = 3.0;

/// Where the pointer is.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Pointer {
    /// On screen, in points.
    pub(crate) screen: Pos2,
    /// In the map's screen space (world pixels), where objects are drawn.
    pub(crate) world: DVec2,
    /// In the current layer's pixels (object coordinates): the map's, less the layer's offset.
    pub(crate) pixel: DVec2,
    /// The current layer's cell under the pointer, the layer's offset taken off as Tiled does.
    pub(crate) cell: IVec2,
}

/// A button going down or a double click.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct PointerPress {
    /// Where.
    pub(crate) at: Pointer,
    /// Which button.
    pub(crate) button: PointerButton,
    /// The modifiers held.
    pub(crate) modifiers: Modifiers,
}

/// The pointer moving with a button held since a press on the map, or the button going up.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct PointerDrag {
    /// Where the press was.
    pub(crate) start: Pointer,
    /// Where the pointer is.
    pub(crate) at: Pointer,
    /// Which button.
    pub(crate) button: PointerButton,
    /// The modifiers held at the press.
    pub(crate) start_modifiers: Modifiers,
    /// The modifiers held now.
    pub(crate) modifiers: Modifiers,
    /// Whether the pointer went further than a click allows.
    pub(crate) moved: bool,
}

/// What the pointer did.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum Gesture {
    /// It moved with no button held.
    Hover(Pointer),
    /// A button went down over the map.
    Press(PointerPress),
    /// It moved with a button held.
    Drag(PointerDrag),
    /// The button went up.
    Release(PointerDrag),
    /// The primary button was clicked twice.
    DoubleClick(PointerPress),
    /// The modifiers held changed.
    ModifiersChanged(Modifiers),
    /// Escape: whatever the tool was doing stops.
    Cancel,
    /// Enter: whatever the tool is making is finished.
    Confirm,
    /// The pointer left the map.
    Leave,
}

/// A press under way.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Held {
    button: PointerButton,
    start: Pointer,
    start_modifiers: Modifiers,
    last: Pointer,
    moved: bool,
}

impl Held {
    fn drag(&self, at: Pointer, modifiers: Modifiers) -> PointerDrag {
        PointerDrag {
            start: self.start,
            at,
            button: self.button,
            start_modifiers: self.start_modifiers,
            modifiers,
            moved: self.moved,
        }
    }
}

/// What the canvas tells the reader about the frame.
pub(crate) struct ReadFrame<'a> {
    /// The canvas's rectangle.
    pub(crate) rect: Rect,
    /// Whether the canvas is the widget under the pointer (no panel or popup over it).
    pub(crate) hovered: bool,
    /// Whether presses go to panning rather than the tool (Space held, typing).
    pub(crate) panning: bool,
    /// Whether Escape and Enter are the tool's: no text field has the keyboard and no dialog
    /// is open.
    pub(crate) keys: bool,
    /// Where a screen point is in the map's terms.
    pub(crate) to_pointer: &'a dyn Fn(Pos2) -> Pointer,
}

/// Turns pointer events into gestures, remembering a press across frames.
#[derive(Clone, Debug, Default)]
pub(crate) struct GestureReader {
    held: Option<Held>,
    modifiers: Modifiers,
    inside: bool,
}

impl GestureReader {
    /// The gestures of a frame, in order.
    pub(crate) fn read(&mut self, input: &egui::InputState, frame: &ReadFrame<'_>) -> Vec<Gesture> {
        let mut gestures = Vec::new();
        if input.modifiers != self.modifiers {
            self.modifiers = input.modifiers;
            gestures.push(Gesture::ModifiersChanged(input.modifiers));
        }
        for event in &input.events {
            match event {
                Event::PointerMoved(pos) => self.moved(*pos, frame, &mut gestures),
                Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    modifiers,
                } => self.pressed(*pos, *button, *modifiers, frame, &mut gestures),
                Event::PointerButton {
                    pos,
                    button,
                    pressed: false,
                    modifiers,
                } => {
                    if let Some(held) = self.held.filter(|held| held.button == *button) {
                        let at = (frame.to_pointer)(*pos);
                        gestures.push(Gesture::Release(held.drag(at, *modifiers)));
                        self.held = None;
                        let double = *button == PointerButton::Primary
                            && !held.moved
                            && input.pointer.button_double_clicked(PointerButton::Primary);
                        if double {
                            gestures.push(Gesture::DoubleClick(PointerPress {
                                at,
                                button: *button,
                                modifiers: *modifiers,
                            }));
                        }
                    }
                }
                Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } if frame.keys => {
                    self.held = None;
                    gestures.push(Gesture::Cancel);
                }
                Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } if frame.keys => gestures.push(Gesture::Confirm),
                Event::PointerGone if self.held.is_none() && self.inside => {
                    self.inside = false;
                    gestures.push(Gesture::Leave);
                }
                _ => {}
            }
        }
        // A release the canvas never saw.
        if let Some(held) = self.held
            && !input.pointer.button_down(held.button)
        {
            gestures.push(Gesture::Release(held.drag(held.last, input.modifiers)));
            self.held = None;
        }
        gestures
    }

    fn moved(&mut self, pos: Pos2, frame: &ReadFrame<'_>, gestures: &mut Vec<Gesture>) {
        let at = (frame.to_pointer)(pos);
        if let Some(held) = &mut self.held {
            if pos.distance(held.start.screen) > DRAG_DISTANCE {
                held.moved = true;
            }
            held.last = at;
            gestures.push(Gesture::Drag(held.drag(at, self.modifiers)));
            return;
        }
        let inside = frame.hovered && frame.rect.contains(pos);
        if inside {
            self.inside = true;
            gestures.push(Gesture::Hover(at));
        } else if self.inside {
            self.inside = false;
            gestures.push(Gesture::Leave);
        }
    }

    fn pressed(
        &mut self,
        pos: Pos2,
        button: PointerButton,
        modifiers: Modifiers,
        frame: &ReadFrame<'_>,
        gestures: &mut Vec<Gesture>,
    ) {
        let for_tools = matches!(button, PointerButton::Primary | PointerButton::Secondary);
        if self.held.is_some()
            || !for_tools
            || frame.panning
            || !frame.hovered
            || !frame.rect.contains(pos)
        {
            return;
        }
        let at = (frame.to_pointer)(pos);
        self.held = Some(Held {
            button,
            start: at,
            start_modifiers: modifiers,
            last: at,
            moved: false,
        });
        gestures.push(Gesture::Press(PointerPress {
            at,
            button,
            modifiers,
        }));
    }
}

#[cfg(test)]
mod tests {
    use egui::{pos2, vec2};

    use super::*;

    fn pointer(pos: Pos2) -> Pointer {
        Pointer {
            screen: pos,
            world: DVec2::new(f64::from(pos.x), f64::from(pos.y)),
            pixel: DVec2::new(f64::from(pos.x), f64::from(pos.y)),
            cell: IVec2::new((pos.x / 10.0) as i32, (pos.y / 10.0) as i32),
        }
    }

    /// A frame's input with `events`; the pointer state is what those events leave.
    fn input(events: Vec<Event>) -> egui::InputState {
        let raw = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(200.0, 200.0))),
            ..egui::RawInput::default()
        };
        egui::InputState::default().begin_pass(raw, false, 1.0, egui::InputOptions::default())
    }

    fn frame(to_pointer: &dyn Fn(Pos2) -> Pointer) -> ReadFrame<'_> {
        ReadFrame {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)),
            hovered: true,
            panning: false,
            keys: true,
            to_pointer,
        }
    }

    #[test]
    fn a_press_drag_and_release_carry_the_start_and_the_modifiers() {
        let mut reader = GestureReader::default();
        let to = |p: Pos2| pointer(p);
        let frame = frame(&to);
        let gestures = reader.read(
            &input(vec![
                Event::PointerMoved(pos2(15.0, 15.0)),
                Event::PointerButton {
                    pos: pos2(15.0, 15.0),
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::SHIFT,
                },
                Event::PointerMoved(pos2(35.0, 15.0)),
                Event::PointerButton {
                    pos: pos2(35.0, 15.0),
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ]),
            &frame,
        );
        assert!(matches!(gestures[0], Gesture::Hover(at) if at.cell == IVec2::new(1, 1)));
        assert!(matches!(gestures[1], Gesture::Press(p) if p.modifiers.shift));
        let Gesture::Drag(drag) = gestures[2] else {
            panic!("a drag: {gestures:?}");
        };
        assert!(
            drag.moved && drag.start.cell == IVec2::new(1, 1) && drag.at.cell == IVec2::new(3, 1)
        );
        assert!(drag.start_modifiers.shift);
        assert!(matches!(gestures[3], Gesture::Release(r) if r.moved));
    }

    #[test]
    fn presses_outside_the_canvas_or_while_panning_go_elsewhere() {
        let to = |p: Pos2| pointer(p);
        let press = |pos| {
            input(vec![Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }])
        };
        let mut reader = GestureReader::default();
        assert!(
            reader
                .read(&press(pos2(150.0, 10.0)), &frame(&to))
                .is_empty()
        );
        let panning = ReadFrame {
            panning: true,
            ..frame(&to)
        };
        assert!(reader.read(&press(pos2(10.0, 10.0)), &panning).is_empty());
        let middle = input(vec![Event::PointerButton {
            pos: pos2(10.0, 10.0),
            button: PointerButton::Middle,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert!(reader.read(&middle, &frame(&to)).is_empty(), "middle pans");
    }

    #[test]
    fn a_lost_release_is_made_up_and_escape_cancels() {
        let to = |p: Pos2| pointer(p);
        let mut reader = GestureReader::default();
        let pressed = reader.read(
            &input(vec![Event::PointerButton {
                pos: pos2(10.0, 10.0),
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }]),
            &frame(&to),
        );
        assert!(matches!(pressed[..], [Gesture::Press(_)]), "{pressed:?}");
        // The next frame finds the button up: the release happened elsewhere.
        let released = reader.read(&input(Vec::new()), &frame(&to));
        assert!(
            matches!(released[..], [Gesture::Release(_)]),
            "{released:?}"
        );
        let escape = reader.read(
            &input(vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]),
            &frame(&to),
        );
        assert_eq!(escape, [Gesture::Cancel]);
        let enter = || {
            input(vec![Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }])
        };
        assert_eq!(reader.read(&enter(), &frame(&to)), [Gesture::Confirm]);
        let elsewhere = ReadFrame {
            keys: false,
            ..frame(&to)
        };
        assert!(
            reader.read(&enter(), &elsewhere).is_empty(),
            "Enter in a text field or a dialog is theirs"
        );
    }
}
