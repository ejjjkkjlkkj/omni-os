#![no_std]
#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct NodeId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Application,
    Window,
    Dialog,
    Button,
    CheckBox,
    ComboBox,
    Edit,
    Heading,
    Image,
    Link,
    List,
    ListItem,
    Menu,
    MenuItem,
    ProgressBar,
    Slider,
    StaticText,
    Tab,
    TabItem,
    Table,
    TableCell,
    Tree,
    TreeItem,
    Terminal,
}

impl Role {
    #[must_use]
    pub const fn is_interactive(self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::CheckBox
                | Self::ComboBox
                | Self::Edit
                | Self::Link
                | Self::MenuItem
                | Self::Slider
                | Self::TabItem
                | Self::TreeItem
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct State(u32);

impl State {
    pub const FOCUSABLE: u32 = 1 << 0;
    pub const FOCUSED: u32 = 1 << 1;
    pub const DISABLED: u32 = 1 << 2;
    pub const CHECKED: u32 = 1 << 3;
    pub const EXPANDED: u32 = 1 << 4;
    pub const SELECTED: u32 = 1 << 5;
    pub const READ_ONLY: u32 = 1 << 6;

    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn contains(self, flag: u32) -> bool {
        self.0 & flag == flag
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticNode<'a> {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub role: Role,
    pub name: &'a str,
    pub description: &'a str,
    pub value: &'a str,
    pub state: State,
    pub bounds: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError {
    InteractiveNodeHasNoName,
    InteractiveNodeIsNotFocusable,
    FocusedNodeIsNotFocusable,
    ZeroSizedInteractiveNode,
}

/// Enforces accessibility invariants before a native UI node can be exposed.
/// This is deliberately usable without allocation or a standard library.
pub fn validate_node(node: &SemanticNode<'_>) -> Result<(), ValidationError> {
    if node.role.is_interactive() && node.name.trim().is_empty() {
        return Err(ValidationError::InteractiveNodeHasNoName);
    }

    if node.role.is_interactive() && !node.state.contains(State::FOCUSABLE) {
        return Err(ValidationError::InteractiveNodeIsNotFocusable);
    }

    if node.state.contains(State::FOCUSED) && !node.state.contains(State::FOCUSABLE) {
        return Err(ValidationError::FocusedNodeIsNotFocusable);
    }

    if node.role.is_interactive() && (node.bounds.width == 0 || node.bounds.height == 0) {
        return Err(ValidationError::ZeroSizedInteractiveNode);
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessibilityEvent {
    FocusChanged(NodeId),
    NameChanged(NodeId),
    ValueChanged(NodeId),
    StateChanged(NodeId),
    ChildrenChanged(NodeId),
    LiveRegionChanged(NodeId),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button<'a>(name: &'a str, state: State) -> SemanticNode<'a> {
        SemanticNode {
            id: NodeId(1),
            parent: Some(NodeId(0)),
            role: Role::Button,
            name,
            description: "",
            value: "",
            state,
            bounds: Rect {
                x: 10,
                y: 10,
                width: 100,
                height: 32,
            },
        }
    }

    #[test]
    fn interactive_node_requires_accessible_name() {
        let node = button("", State::from_bits(State::FOCUSABLE));
        assert_eq!(
            validate_node(&node),
            Err(ValidationError::InteractiveNodeHasNoName)
        );
    }

    #[test]
    fn interactive_node_requires_keyboard_focus_contract() {
        let node = button("Install", State::default());
        assert_eq!(
            validate_node(&node),
            Err(ValidationError::InteractiveNodeIsNotFocusable)
        );
    }

    #[test]
    fn valid_button_passes() {
        let node = button("Install", State::from_bits(State::FOCUSABLE));
        assert_eq!(validate_node(&node), Ok(()));
    }
}
