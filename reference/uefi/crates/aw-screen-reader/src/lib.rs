#![no_std]
#![forbid(unsafe_code)]

//! The announcement engine of the native screen reader.
//!
//! This is the part of a screen reader that decides *what is spoken*: given a
//! focused semantic node (or an accessibility event on one), it produces the
//! single utterance a blind user hears. Nothing here draws, speaks or reads
//! input - it turns [`aw_accessibility`] semantics into text. The speech engine,
//! the braille transport and the focus tracker are separate layers that feed it
//! nodes and render the string it returns; keeping the wording pure makes it
//! exhaustively testable, which is what the project's accessibility contract
//! demands (a blind user must be able to operate every surface without visual
//! assistance).
//!
//! It is deliberately `no_std` and allocation-free: an utterance is composed into
//! a caller-provided byte buffer through [`Utterance`], so the same code runs in
//! the freestanding kernel, in the installer and in the recovery environment,
//! none of which can assume a heap. The output stays valid UTF-8 even when the
//! buffer is too small - it truncates on a character boundary and reports it.
//!
//! ## Utterance order
//!
//! A focus announcement is composed in a fixed, documented order, comma-separated
//! the way a speech engine phrases a pause:
//!
//! ```text
//! <name>, <role>, <states>, <value>, <position in set>, <description>
//! ```
//!
//! - the accessible **name** first, because that is what the control *is*;
//! - the **role** ("button", "check box"), skipped for plain static text, which
//!   is simply read;
//! - **states** that change how it is operated: unavailable, checked/not checked,
//!   expanded/collapsed, selected, read only;
//! - the current **value** (a slider's "40%", an edit's text, or "blank");
//! - the **position in set** ("2 of 5") when the caller knows it;
//! - a supplementary **description** last.
//!
//! Empty parts are dropped, and no separator is emitted for them, so a bare
//! static text speaks as just its text and a plain button as "Name, button".

use aw_accessibility::{AccessibilityEvent, Role, SemanticNode, State};

mod focus;
pub use focus::{FocusRing, NavCommand};

/// A speech utterance being composed into a fixed byte buffer.
///
/// Phrases are appended with [`Utterance::phrase`]; the first non-empty phrase
/// stands alone and each later one is preceded by `", "`, matching how a speech
/// engine pauses between announced facts. The buffer is never overrun: an append
/// that would not fit is truncated on a UTF-8 boundary and [`Utterance::truncated`]
/// becomes true.
pub struct Utterance<'a> {
    buffer: &'a mut [u8],
    len: usize,
    truncated: bool,
}

impl<'a> Utterance<'a> {
    /// Start composing into `buffer`.
    #[must_use]
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            len: 0,
            truncated: false,
        }
    }

    /// Append raw text, truncating on a character boundary if it does not fit.
    fn append(&mut self, text: &str) {
        let remaining = self.buffer.len() - self.len;
        let bytes = text.as_bytes();
        if bytes.len() <= remaining {
            self.buffer[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
            return;
        }
        // Copy the longest prefix that fits and ends on a character boundary, so
        // the buffer always holds valid UTF-8.
        let mut take = remaining;
        while take > 0 && !text.is_char_boundary(take) {
            take -= 1;
        }
        self.buffer[self.len..self.len + take].copy_from_slice(&bytes[..take]);
        self.len += take;
        self.truncated = true;
    }

    /// Append one phrase. Empty phrases are ignored; a non-empty phrase after an
    /// earlier one is separated by `", "`.
    pub fn phrase(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.len != 0 {
            self.append(", ");
        }
        self.append(text);
    }

    /// Append a `<position> of <set_size>` phrase, e.g. `2 of 5`.
    pub fn position(&mut self, position: u32, set_size: u32) {
        if position == 0 || set_size == 0 {
            return;
        }
        if self.len != 0 {
            self.append(", ");
        }
        self.append_u32(position);
        self.append(" of ");
        self.append_u32(set_size);
    }

    /// Append a decimal integer.
    fn append_u32(&mut self, mut value: u32) {
        let mut digits = [0u8; 10];
        let mut index = digits.len();
        if value == 0 {
            self.append("0");
            return;
        }
        while value != 0 {
            index -= 1;
            digits[index] = b'0' + (value % 10) as u8;
            value /= 10;
        }
        // SAFETY-FREE: `digits[index..]` are all ASCII digits, valid UTF-8.
        if let Ok(text) = core::str::from_utf8(&digits[index..]) {
            self.append(text);
        }
    }

    /// The composed utterance so far.
    #[must_use]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buffer[..self.len]).unwrap_or("")
    }

    /// Length in bytes of the composed utterance.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the utterance is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether any append was truncated for lack of room.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

/// Where a focused node sits in a set of siblings, for a "2 of 5" announcement.
/// [`FocusContext::NONE`] suppresses it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusContext {
    /// One-based position of the node among its announced siblings; 0 = unknown.
    pub position_in_set: u32,
    /// Number of announced siblings; 0 = unknown.
    pub set_size: u32,
}

impl FocusContext {
    /// No positional information.
    pub const NONE: Self = Self {
        position_in_set: 0,
        set_size: 0,
    };

    /// A node at `position` (one-based) within a set of `set_size`.
    #[must_use]
    pub const fn in_set(position: u32, set_size: u32) -> Self {
        Self {
            position_in_set: position,
            set_size,
        }
    }
}

/// The spoken word for a role. Plain, lowercase, the way a screen reader names a
/// control after its accessible name.
#[must_use]
pub const fn role_label(role: Role) -> &'static str {
    match role {
        Role::Application => "application",
        Role::Window => "window",
        Role::Dialog => "dialog",
        Role::Button => "button",
        Role::CheckBox => "check box",
        Role::ComboBox => "combo box",
        Role::Edit => "edit",
        Role::Heading => "heading",
        Role::Image => "image",
        Role::Link => "link",
        Role::List => "list",
        Role::ListItem => "list item",
        Role::Menu => "menu",
        Role::MenuItem => "menu item",
        Role::ProgressBar => "progress bar",
        Role::Slider => "slider",
        Role::StaticText => "text",
        Role::Tab => "tab",
        Role::TabItem => "tab",
        Role::Table => "table",
        Role::TableCell => "cell",
        Role::Tree => "tree",
        Role::TreeItem => "tree item",
        Role::Terminal => "terminal",
    }
}

/// Does this role carry its content in its name and speak with no role word
/// (plain text a screen reader simply reads aloud)?
const fn is_content_text(role: Role) -> bool {
    matches!(role, Role::StaticText)
}

/// Can a node of this role expand and collapse, so "collapsed" is meaningful even
/// when the expanded bit is clear?
const fn is_always_expandable(role: Role) -> bool {
    matches!(role, Role::ComboBox)
}

/// Append the state phrases for a focused node, in operating-relevant order.
fn append_states(utterance: &mut Utterance<'_>, node: &SemanticNode<'_>) {
    let state = node.state;

    if state.contains(State::DISABLED) {
        utterance.phrase("unavailable");
    }

    if matches!(node.role, Role::CheckBox) {
        utterance.phrase(if state.contains(State::CHECKED) {
            "checked"
        } else {
            "not checked"
        });
    }

    // Expanded/collapsed: always for a combo box; for tree and menu items only
    // when actually expanded, since a leaf that never expands has the bit clear
    // and must not be announced as "collapsed".
    if is_always_expandable(node.role) {
        utterance.phrase(if state.contains(State::EXPANDED) {
            "expanded"
        } else {
            "collapsed"
        });
    } else if state.contains(State::EXPANDED)
        && matches!(node.role, Role::TreeItem | Role::MenuItem)
    {
        utterance.phrase("expanded");
    }

    if state.contains(State::SELECTED)
        && matches!(
            node.role,
            Role::TabItem | Role::ListItem | Role::TreeItem | Role::TableCell
        )
    {
        utterance.phrase("selected");
    }

    if matches!(node.role, Role::Edit) && state.contains(State::READ_ONLY) {
        utterance.phrase("read only");
    }
}

/// Append the current value of a node that has one.
fn append_value(utterance: &mut Utterance<'_>, node: &SemanticNode<'_>) {
    match node.role {
        Role::Slider | Role::ProgressBar => utterance.phrase(node.value),
        Role::Edit => {
            if node.value.trim().is_empty() {
                utterance.phrase("blank");
            } else {
                utterance.phrase(node.value);
            }
        }
        Role::ComboBox => utterance.phrase(node.value),
        _ => {}
    }
}

/// Compose the utterance spoken when focus lands on `node`, writing it into
/// `buffer` and returning it. `context` supplies a "position of size" phrase when
/// known; pass [`FocusContext::NONE`] otherwise.
pub fn announce_focus<'a>(
    node: &SemanticNode<'_>,
    context: FocusContext,
    buffer: &'a mut [u8],
) -> &'a str {
    let length = {
        let mut utterance = Utterance::new(buffer);
        utterance.phrase(node.name);
        if !is_content_text(node.role) {
            utterance.phrase(role_label(node.role));
        }
        append_states(&mut utterance, node);
        append_value(&mut utterance, node);
        utterance.position(context.position_in_set, context.set_size);
        utterance.phrase(node.description);
        utterance.len()
    };
    core::str::from_utf8(&buffer[..length]).unwrap_or("")
}

/// Compose the utterance spoken when `event` fires on `node`, writing it into
/// `buffer`. Returns the text, which is empty for events a screen reader does not
/// voice on their own (for example a silent children-changed).
pub fn announce_event<'a>(
    event: AccessibilityEvent,
    node: &SemanticNode<'_>,
    buffer: &'a mut [u8],
) -> &'a str {
    match event {
        AccessibilityEvent::FocusChanged(_) => announce_focus(node, FocusContext::NONE, buffer),
        AccessibilityEvent::StateChanged(_) => {
            let length = {
                let mut utterance = Utterance::new(buffer);
                append_states(&mut utterance, node);
                utterance.len()
            };
            core::str::from_utf8(&buffer[..length]).unwrap_or("")
        }
        AccessibilityEvent::ValueChanged(_) => {
            let length = {
                let mut utterance = Utterance::new(buffer);
                append_value(&mut utterance, node);
                utterance.len()
            };
            core::str::from_utf8(&buffer[..length]).unwrap_or("")
        }
        // A live region and a renamed node both speak their current text; a bare
        // name change on the focused control is voiced the same way.
        AccessibilityEvent::NameChanged(_) | AccessibilityEvent::LiveRegionChanged(_) => {
            let length = {
                let mut utterance = Utterance::new(buffer);
                utterance.phrase(node.name);
                utterance.len()
            };
            core::str::from_utf8(&buffer[..length]).unwrap_or("")
        }
        // Structural churn is not spoken on its own; the next focus move is.
        AccessibilityEvent::ChildrenChanged(_) => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_accessibility::{NodeId, Rect};

    const BOUNDS: Rect = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 20,
    };

    fn node<'a>(role: Role, name: &'a str, value: &'a str, state_bits: u32) -> SemanticNode<'a> {
        SemanticNode {
            id: NodeId(1),
            parent: Some(NodeId(0)),
            role,
            name,
            description: "",
            value,
            state: State::from_bits(state_bits),
            bounds: BOUNDS,
        }
    }

    #[test]
    fn button_speaks_name_then_role() {
        let n = node(Role::Button, "Install", "", State::FOCUSABLE);
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Install, button"
        );
    }

    #[test]
    fn checked_check_box() {
        let n = node(
            Role::CheckBox,
            "Enable screen reader at boot",
            "",
            State::FOCUSABLE | State::CHECKED,
        );
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Enable screen reader at boot, check box, checked"
        );
    }

    #[test]
    fn unchecked_check_box() {
        let n = node(Role::CheckBox, "Encrypt disk", "", State::FOCUSABLE);
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Encrypt disk, check box, not checked"
        );
    }

    #[test]
    fn slider_speaks_value() {
        let n = node(Role::Slider, "Speech rate", "40%", State::FOCUSABLE);
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Speech rate, slider, 40%"
        );
    }

    #[test]
    fn static_text_is_read_without_role() {
        let n = node(
            Role::StaticText,
            "Welcome to Accessible Windows setup",
            "",
            0,
        );
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Welcome to Accessible Windows setup"
        );
    }

    #[test]
    fn empty_edit_is_blank() {
        let n = node(Role::Edit, "Computer name", "", State::FOCUSABLE);
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Computer name, edit, blank"
        );
    }

    #[test]
    fn edit_with_value_and_read_only() {
        let n = node(
            Role::Edit,
            "Version",
            "0.1.0",
            State::FOCUSABLE | State::READ_ONLY,
        );
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Version, edit, read only, 0.1.0"
        );
    }

    #[test]
    fn selected_list_item_with_position() {
        let n = node(
            Role::ListItem,
            "English",
            "",
            State::FOCUSABLE | State::SELECTED,
        );
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::in_set(2, 5), &mut buffer),
            "English, list item, selected, 2 of 5"
        );
    }

    #[test]
    fn disabled_button_is_unavailable() {
        let n = node(Role::Button, "Next", "", State::FOCUSABLE | State::DISABLED);
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Next, button, unavailable"
        );
    }

    #[test]
    fn collapsed_and_expanded_combo_box() {
        let collapsed = node(Role::ComboBox, "Keyboard layout", "US", State::FOCUSABLE);
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&collapsed, FocusContext::NONE, &mut buffer),
            "Keyboard layout, combo box, collapsed, US"
        );

        let expanded = node(
            Role::ComboBox,
            "Keyboard layout",
            "US",
            State::FOCUSABLE | State::EXPANDED,
        );
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&expanded, FocusContext::NONE, &mut buffer),
            "Keyboard layout, combo box, expanded, US"
        );
    }

    #[test]
    fn dialog_announces_role() {
        let n = node(Role::Dialog, "Install Accessible Windows", "", 0);
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Install Accessible Windows, dialog"
        );
    }

    #[test]
    fn description_comes_last() {
        let mut n = node(Role::Button, "Install", "", State::FOCUSABLE);
        n.description = "installs to the selected disk";
        let mut buffer = [0u8; 256];
        assert_eq!(
            announce_focus(&n, FocusContext::NONE, &mut buffer),
            "Install, button, installs to the selected disk"
        );
    }

    #[test]
    fn value_changed_event_speaks_only_value() {
        let n = node(Role::Slider, "Speech rate", "55%", State::FOCUSABLE);
        let mut buffer = [0u8; 64];
        let text = announce_event(AccessibilityEvent::ValueChanged(n.id), &n, &mut buffer);
        assert_eq!(text, "55%");
    }

    #[test]
    fn state_changed_event_speaks_only_state() {
        let n = node(
            Role::CheckBox,
            "Enable screen reader at boot",
            "",
            State::FOCUSABLE | State::CHECKED,
        );
        let mut buffer = [0u8; 64];
        let text = announce_event(AccessibilityEvent::StateChanged(n.id), &n, &mut buffer);
        assert_eq!(text, "checked");
    }

    #[test]
    fn children_changed_is_silent() {
        let n = node(Role::List, "Disks", "", 0);
        let mut buffer = [0u8; 64];
        let text = announce_event(AccessibilityEvent::ChildrenChanged(n.id), &n, &mut buffer);
        assert_eq!(text, "");
    }

    #[test]
    fn truncation_keeps_valid_utf8_and_reports() {
        // A multi-byte name that cannot fully fit, to prove we never split a char.
        let n = node(Role::Button, "café-au-lait", "", State::FOCUSABLE);
        let mut buffer = [0u8; 5];
        let mut utterance = Utterance::new(&mut buffer);
        utterance.phrase(n.name);
        assert!(utterance.truncated());
        // Still valid UTF-8 (the accented byte pair was not cut in half).
        let _ = utterance.as_str();
        assert!(utterance.as_str().is_char_boundary(utterance.len()));
    }
}
