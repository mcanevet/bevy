//! Serializable schema for input surface enumeration results.
//!
//! The enumeration output is designed to be the *contract* between a
//! game's declared interactive surface and any external driver: bots,
//! test harnesses, accessibility tooling, remapping UIs, and
//! Playwright-style remote clients (serialized over BRP or any other
//! transport). Every field is plain data — no Bevy-internal handles —
//! so results survive serialization and can be diffed between frames
//! or between game versions.
//!
//! Honest-declaration doctrine (mirrors the fail-open posture of the
//! playtesting harness this was designed alongside):
//!
//! - Enumeration reports what the game *declares*, not what is
//!   guaranteed to work. A declared binding may still be dead code;
//!   consumers must verify liveness empirically (the audit half).
//! - Every sub-result carries a `complete` flag: a `false` value means
//!   the enumeration *provably missed things* (e.g. uninitialized
//!   schedules hid systems), and absent entries prove nothing.
//! - Counts and lists are annotations, never licenses to prune probe
//!   surfaces.

use bevy_ecs::entity::Entity;
use bevy_math::Vec2;
use serde::{Deserialize, Serialize};

/// How an interactable was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractableKind {
    /// Explicit `Pickable` component (`bevy_picking` opt-in).
    Pickable,
    /// A `bevy_ui_widgets` widget component (Button, Checkbox, ...).
    Widget,
    /// Keyboard-focusable via `TabIndex` (`bevy_input_focus`).
    Focusable,
    /// Carries an accessibility node (role + label): a semantic
    /// declaration of interactivity.
    Accessible,
    /// Has an observer watching at least one pointer event type
    /// (`PointerClick`, `PointerPress`, ...). Declarative by
    /// construction: someone wrote code reacting to clicks here.
    Observed,
}

impl InteractableKind {
    /// Stable snake-case name, useful for serialized consumers.
    pub fn as_str(self) -> &'static str {
        match self {
            InteractableKind::Pickable => "pickable",
            InteractableKind::Widget => "widget",
            InteractableKind::Focusable => "focusable",
            InteractableKind::Accessible => "accessible",
            InteractableKind::Observed => "observed",
        }
    }
}

/// Best-effort semantic classification of an interactable, derived
/// from known widget components. `Unknown` means "some declared
/// surface exists but its role is not standardized".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetRole {
    /// A `bevy_ui_widgets::Button`.
    Button,
    /// A `bevy_ui_widgets::Checkbox`.
    Checkbox,
    /// A `bevy_ui_widgets::RadioButton`.
    Radio,
    /// A `bevy_ui_widgets::Slider`.
    Slider,
    /// A `bevy_ui_widgets::Tab`.
    Tab,
    /// A `bevy_ui_widgets::TextInput`.
    TextInput,
    /// A `bevy_ui_widgets::ListBox`.
    ListBox,
    /// A `bevy_ui_widgets::MenuItem`.
    MenuItem,
    /// A `bevy_ui_widgets::MenuButton`.
    MenuButton,
    /// A `bevy_ui_widgets::ScrollArea`.
    ScrollArea,
    /// Declared surface with no recognized widget component.
    Unknown,
}

impl WidgetRole {
    /// Stable snake-case name, useful for serialized consumers.
    pub fn as_str(self) -> &'static str {
        match self {
            WidgetRole::Button => "button",
            WidgetRole::Checkbox => "checkbox",
            WidgetRole::Radio => "radio",
            WidgetRole::Slider => "slider",
            WidgetRole::Tab => "tab",
            WidgetRole::TextInput => "text_input",
            WidgetRole::ListBox => "list_box",
            WidgetRole::MenuItem => "menu_item",
            WidgetRole::MenuButton => "menu_button",
            WidgetRole::ScrollArea => "scroll_area",
            WidgetRole::Unknown => "unknown",
        }
    }
}

/// One declared interactive surface point.
///
/// Positions are **physical pixels, window-space** (origin top-left,
/// y-down) — the same space a native cursor event reports and the same
/// space synthetic-input drivers inject at. Consumers clicking here
/// reproduce picking behavior exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interactable {
    /// Generational entity id (serialize/deserialize requires the
    /// `serialize` feature of `bevy_ecs`, enabled by this crate).
    pub entity: Entity,
    /// Every distinct declaration signal found on this entity,
    /// unioned. An entity can be both `Widget` and `Focusable`.
    pub kinds: Vec<InteractableKind>,
    /// Semantic role if identifiable from a known widget component.
    pub role: WidgetRole,
    /// Screen-space center in physical pixels, if resolvable.
    pub center: Option<Vec2>,
    /// Screen-space axis-aligned bounding rect (min/max corners,
    /// physical pixels), if resolvable. Always in window space.
    pub rect: Option<Rect>,
    /// Disabled flag: `InteractionDisabled` (`bevy_ui`) present.
    pub disabled: bool,
    /// Best-effort label: accessible label > child `Text` text > `Name`.
    pub label: Option<String>,
    /// Currently keyboard-focused (input focus resource points here).
    pub focused: bool,
}

/// Screen-space rectangle, min corner inclusive, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Top-left corner of the rect.
    pub min: Vec2,
    /// Bottom-right corner of the rect.
    pub max: Vec2,
}

/// Result of enumerating the UI/interactable surface.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiSurfaceResult {
    /// Every declared interactable found, ordered deterministically
    /// by entity index.
    pub interactables: Vec<Interactable>,
    /// Whether the enumeration saw every signal source it knows
    /// about: `false` means the result provably missed possible
    /// declarations (fail-open: absent ≠ non-existent).
    pub complete: bool,
    /// Non-fatal caveats discovered during enumeration.
    pub notes: Vec<String>,
}

/// Result of the raw-input reader census (schedule-access scan).
/// Annotation grade only.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReaderCensusResult {
    /// Device class -> number of systems whose access set includes a
    /// read or write of one of that class's signal types.
    pub readers: Vec<(DeviceClass, usize)>,
    /// Same fail-open completeness doctrine as the UI scan.
    pub complete: bool,
    /// Caveats (exclusive systems present, uninitialized schedules...).
    pub notes: Vec<String>,
}

/// Device classes tracked by the reader census.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    /// Keyboard: key codes, logical keys, keyboard input messages.
    Keyboard,
    /// Mouse buttons: `ButtonInput<MouseButton>`, `MouseButtonInput`.
    MouseButton,
    /// Mouse motion: `MouseMotion`, `CursorMoved`, accumulators.
    MouseMotion,
    /// Mouse wheel: `MouseWheel`, accumulated scroll.
    MouseWheel,
    /// Gamepads: resources, buttons, axes, message channels.
    Gamepad,
}

impl DeviceClass {
    /// Stable snake-case name, useful for serialized consumers.
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceClass::Keyboard => "keyboard",
            DeviceClass::MouseButton => "mouse_button",
            DeviceClass::MouseMotion => "mouse_motion",
            DeviceClass::MouseWheel => "mouse_wheel",
            DeviceClass::Gamepad => "gamepad",
        }
    }

    /// Every tracked device class.
    pub const ALL: [Self; 5] = [
        Self::Keyboard,
        Self::MouseButton,
        Self::MouseMotion,
        Self::MouseWheel,
        Self::Gamepad,
    ];
}

/// The complete input surface of a game at one instant.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSurface {
    /// The declared UI/interactable surface.
    pub ui: UiSurfaceResult,
    /// The raw-input device-reader census.
    pub readers: ReaderCensusResult,
}
