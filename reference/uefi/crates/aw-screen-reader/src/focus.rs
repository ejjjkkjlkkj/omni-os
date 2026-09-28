//! Keyboard focus navigation: the input half of the screen reader.
//!
//! The announcement engine in the crate root decides *what* is spoken; this
//! decides *which control the user is on*. A [`FocusRing`] wraps the tab order of
//! a screen - the semantic nodes in the sequence the keyboard visits them - and
//! moves focus in response to the four navigation commands a keyboard-only user
//! relies on: Tab, Shift+Tab, Home and End. Non-interactive nodes (a heading, a
//! block of static text) and unavailable controls are skipped, and Tab wraps from
//! the last focusable control back to the first, so focus can never be lost.
//!
//! This is the "deterministic keyboard semantics" the accessibility contract
//! requires: the same keys always move focus the same way, with no pointer and no
//! visual cue. Paired with [`crate::announce_focus`], pressing Tab yields the
//! exact utterance for the newly focused control - the whole operable loop a
//! blind user needs, and all of it `no_std` and allocation-free.

use aw_accessibility::{SemanticNode, State};

use crate::{FocusContext, announce_focus};

/// A keyboard navigation command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavCommand {
    /// Tab: move to the next focusable control, wrapping to the first.
    Next,
    /// Shift+Tab: move to the previous focusable control, wrapping to the last.
    Previous,
    /// Home: move to the first focusable control.
    First,
    /// End: move to the last focusable control.
    Last,
}

/// The focusable tab order of one screen, tracking where focus currently is.
pub struct FocusRing<'a> {
    nodes: &'a [SemanticNode<'a>],
    current: Option<usize>,
}

impl<'a> FocusRing<'a> {
    /// Wrap a screen's tab order. Focus starts unset; the first navigation lands
    /// it on a focusable control.
    #[must_use]
    pub fn new(nodes: &'a [SemanticNode<'a>]) -> Self {
        Self {
            nodes,
            current: None,
        }
    }

    /// A control is a keyboard stop if it is focusable and not unavailable.
    fn is_focus_stop(node: &SemanticNode<'_>) -> bool {
        node.state.contains(State::FOCUSABLE) && !node.state.contains(State::DISABLED)
    }

    /// The node focus is on, if any.
    #[must_use]
    pub fn current(&self) -> Option<&SemanticNode<'a>> {
        self.current.map(|index| &self.nodes[index])
    }

    /// The index focus is on, if any.
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// The number of keyboard stops in the ring.
    #[must_use]
    pub fn stop_count(&self) -> u32 {
        self.nodes.iter().filter(|n| Self::is_focus_stop(n)).count() as u32
    }

    /// Scan for the nearest focus stop starting at `start`, moving forward or
    /// backward and wrapping, considering every position once.
    fn scan(&self, start: usize, forward: bool) -> Option<usize> {
        let len = self.nodes.len();
        if len == 0 {
            return None;
        }
        for step in 0..len {
            let index = if forward {
                (start + step) % len
            } else {
                (start + len - step) % len
            };
            if Self::is_focus_stop(&self.nodes[index]) {
                return Some(index);
            }
        }
        None
    }

    /// Apply a navigation command, updating and returning the new focus index.
    /// Returns `None` only when the screen has no focusable control at all.
    pub fn navigate(&mut self, command: NavCommand) -> Option<usize> {
        let len = self.nodes.len();
        if len == 0 {
            return None;
        }
        let next = match command {
            NavCommand::Next => {
                let start = self.current.map_or(0, |index| (index + 1) % len);
                self.scan(start, true)
            }
            NavCommand::Previous => {
                let start = self
                    .current
                    .map_or(len - 1, |index| (index + len - 1) % len);
                self.scan(start, false)
            }
            NavCommand::First => self.scan(0, true),
            NavCommand::Last => self.scan(len - 1, false),
        };
        if next.is_some() {
            self.current = next;
        }
        next
    }

    /// The position-in-set of the current focus among the keyboard stops, for a
    /// "3 of 7" announcement. [`FocusContext::NONE`] when focus is unset.
    #[must_use]
    pub fn focus_context(&self) -> FocusContext {
        let Some(current) = self.current else {
            return FocusContext::NONE;
        };
        let mut set_size = 0u32;
        let mut position = 0u32;
        for (index, node) in self.nodes.iter().enumerate() {
            if Self::is_focus_stop(node) {
                set_size += 1;
                if index == current {
                    position = set_size;
                }
            }
        }
        FocusContext::in_set(position, set_size)
    }

    /// Compose the utterance for the currently focused control, including its
    /// position in the tab order, into `buffer`. Empty when focus is unset.
    pub fn announce_current<'b>(&self, buffer: &'b mut [u8]) -> &'b str {
        match self.current() {
            Some(node) => announce_focus(node, self.focus_context(), buffer),
            None => core::str::from_utf8(&buffer[..0]).unwrap_or(""),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_accessibility::{NodeId, Rect, Role};

    const BOUNDS: Rect = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 20,
    };

    fn node(id: u64, role: Role, name: &'static str, state_bits: u32) -> SemanticNode<'static> {
        SemanticNode {
            id: NodeId(id),
            parent: Some(NodeId(0)),
            role,
            name,
            description: "",
            value: "",
            state: State::from_bits(state_bits),
            bounds: BOUNDS,
        }
    }

    /// A screen with two non-focusable nodes (a heading, static text), three
    /// focusable controls, and one disabled control that Tab must skip.
    fn screen() -> [SemanticNode<'static>; 6] {
        [
            node(1, Role::Heading, "Setup", 0),
            node(2, Role::StaticText, "Choose options", 0),
            node(3, Role::CheckBox, "Enable screen reader", State::FOCUSABLE),
            node(4, Role::Button, "Install", State::FOCUSABLE),
            node(
                5,
                Role::Button,
                "Advanced",
                State::FOCUSABLE | State::DISABLED,
            ),
            node(6, Role::Button, "Cancel", State::FOCUSABLE),
        ]
    }

    fn name_after(ring: &mut FocusRing<'_>, command: NavCommand) -> &'static str {
        let index = ring.navigate(command).expect("a focusable control exists");
        // The screen's names are all 'static, so this reference outlives the ring.
        match index {
            2 => "Enable screen reader",
            3 => "Install",
            5 => "Cancel",
            other => panic!("focus landed on a non-stop index {other}"),
        }
    }

    #[test]
    fn tab_visits_focusable_in_order_skipping_the_rest() {
        let screen = screen();
        let mut ring = FocusRing::new(&screen);
        assert_eq!(
            name_after(&mut ring, NavCommand::Next),
            "Enable screen reader"
        );
        assert_eq!(name_after(&mut ring, NavCommand::Next), "Install");
        // Index 4 (disabled "Advanced") is skipped.
        assert_eq!(name_after(&mut ring, NavCommand::Next), "Cancel");
    }

    #[test]
    fn tab_wraps_from_last_to_first() {
        let screen = screen();
        let mut ring = FocusRing::new(&screen);
        ring.navigate(NavCommand::Last); // Cancel
        assert_eq!(ring.current().unwrap().name, "Cancel");
        assert_eq!(
            name_after(&mut ring, NavCommand::Next),
            "Enable screen reader"
        );
    }

    #[test]
    fn shift_tab_goes_backward_and_wraps() {
        let screen = screen();
        let mut ring = FocusRing::new(&screen);
        // First Shift+Tab from unset lands on the last stop.
        assert_eq!(name_after(&mut ring, NavCommand::Previous), "Cancel");
        assert_eq!(name_after(&mut ring, NavCommand::Previous), "Install");
        assert_eq!(
            name_after(&mut ring, NavCommand::Previous),
            "Enable screen reader"
        );
        // Wrap back to the last.
        assert_eq!(name_after(&mut ring, NavCommand::Previous), "Cancel");
    }

    #[test]
    fn home_and_end() {
        let screen = screen();
        let mut ring = FocusRing::new(&screen);
        assert_eq!(name_after(&mut ring, NavCommand::Last), "Cancel");
        assert_eq!(
            name_after(&mut ring, NavCommand::First),
            "Enable screen reader"
        );
    }

    #[test]
    fn stop_count_excludes_non_focusable_and_disabled() {
        let screen = screen();
        let ring = FocusRing::new(&screen);
        assert_eq!(ring.stop_count(), 3);
    }

    #[test]
    fn announce_current_includes_tab_position() {
        let screen = screen();
        let mut ring = FocusRing::new(&screen);
        ring.navigate(NavCommand::Next); // Enable screen reader, 1 of 3
        let mut buffer = [0u8; 128];
        assert_eq!(
            ring.announce_current(&mut buffer),
            "Enable screen reader, check box, not checked, 1 of 3"
        );
        ring.navigate(NavCommand::Next); // Install, 2 of 3
        let mut buffer = [0u8; 128];
        assert_eq!(
            ring.announce_current(&mut buffer),
            "Install, button, 2 of 3"
        );
    }

    #[test]
    fn a_screen_with_no_stops_never_moves() {
        let screen = [
            node(1, Role::Heading, "Title", 0),
            node(2, Role::StaticText, "Body", 0),
        ];
        let mut ring = FocusRing::new(&screen);
        assert_eq!(ring.navigate(NavCommand::Next), None);
        assert_eq!(ring.current(), None);
        assert_eq!(ring.stop_count(), 0);
    }
}
