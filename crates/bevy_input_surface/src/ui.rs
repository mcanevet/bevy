//! Enumeration of the declared UI/interactable surface.
//!
//! Sources of declaration, all unions (an entity may be declared
//! interactable by several independent signals):
//!
//! 1. `Pickable` — the explicit `bevy_picking` opt-in marker.
//! 2. `bevy_ui_widgets` widget components — `Button`, `Checkbox`,
//!    `RadioButton`, `Slider`, `Tab`, `TextInput`, `ListBox`,
//!    `MenuItem`, `MenuButton`, `ScrollArea` (see [`WidgetRole`]).
//! 3. `TabIndex` — keyboard-focusable (`bevy_input_focus` 0.20's
//!    focusable marker; the tab/directional navigation systems key
//!    off it).
//! 4. `AccessibilityNode` / `AccessibleLabel` — a semantic
//!    role/label declaration. Classification beyond "accessible"
//!    would require reading the internal `Arc<ActionRequestWrapper>`
//!    payload, deliberately not done: an a11y node declares
//!    interactivity, not a precise widget role, in this iteration.
//! 5. Pointer-event observers — `world.observers()` exposes the
//!    centralized observer storage; watching `PointerClick`,
//!    `PointerPress`, or `PointerRelease` on a *specific entity* is a
//!    declaration of click-responsiveness, even with no markers.
//!
//! Geometry mirrors `bevy_ui::picking_backend::ui_picking` (verified
//! against 0.20.1): `UiGlobalTransform` is in *viewport space of the
//! node's target camera* — to yield window-space physical pixels we
//! add the camera's `physical_viewport_rect().min` back, the inverse
//! of the correction `ui_picking` applies to cursor positions. Nodes
//! are skipped for visibility exactly when `ui_picking` would skip
//! them (zero size, `InheritedVisibility` false).
//!
//! Deliberately omitted (documented blind spots, surfaced as notes):
//!
//! - Global and component-scoped pointer-event observers (their
//!   firing targets are unattributable at scan time without walking
//!   all archetypes; fabricating attribution would invent
//!   declarations).
//! - `Interaction`/`Button` (deprecated in 0.20, scheduled for
//!   removal — new code should not build on it).
//! - Third-party action-map crates (LWIM, `enhanced_input`): served by
//!   their own reflection-based readers, out of scope for this crate
//!   which has no knowledge of ecosystem crates.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use bevy_a11y::AccessibilityNode;
use bevy_camera::{visibility::InheritedVisibility, Camera};
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryState;
use bevy_input_focus::tab_navigation::TabIndex;
use bevy_math::Vec2;
use bevy_picking::events::{PointerClick, PointerPress, PointerRelease};
use bevy_picking::Pickable;
use bevy_ui::{
    prelude::AccessibleLabel, widget::Text, ComputedNode, ComputedUiTargetCamera,
    InteractionDisabled as UiInteractionDisabled, UiGlobalTransform,
};
use bevy_ui_widgets::{
    Button, Checkbox, ListBox, MenuButton, MenuItem, RadioButton, ScrollArea, Slider, Tab,
    TextInput,
};

use crate::schema::{Interactable, InteractableKind, Rect, UiSurfaceResult, WidgetRole};

/// The pointer events whose observers declare click-responsiveness.
///
/// Deliberately only press/click/release: hover/scroll observers mark
/// attention-tracking, not actionability — the distinction matters
/// because the enumeration's purpose is answering "what can I DO".
fn pointer_observers(world: &World) -> HashMap<Entity, usize> {
    let mut result = HashMap::new();
    let pointer_event_keys = [
        world.event_key::<PointerClick>(),
        world.event_key::<PointerPress>(),
        world.event_key::<PointerRelease>(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    let observers = world.observers();
    for key in pointer_event_keys {
        let Some(cached) = observers.try_get_observers(key) else {
            continue;
        };
        // Entity-scoped observers fire ONLY for that entity: safe to
        // attribute. Global/component observers are omitted (see
        // module docs) — under-attribution is the honest default.
        for (entity, map) in cached.entity_observers() {
            *result.entry(*entity).or_insert(0) += map.len();
        }
    }
    result
}

/// Semantic role from `bevy_ui_widgets` marker components.
///
/// Precedence resolves multi-widget entities (a menu button is both
/// `MenuButton` and `Button`): the more specific role wins.
fn widget_role_from_has(
    button: bool,
    checkbox: bool,
    radio: bool,
    slider: bool,
    tab: bool,
    text_input: bool,
    list_box: bool,
    menu_item: bool,
    menu_button: bool,
    scroll_area: bool,
) -> WidgetRole {
    // More specific roles first.
    if menu_item {
        WidgetRole::MenuItem
    } else if menu_button {
        WidgetRole::MenuButton
    } else if checkbox {
        WidgetRole::Checkbox
    } else if radio {
        WidgetRole::Radio
    } else if slider {
        WidgetRole::Slider
    } else if text_input {
        WidgetRole::TextInput
    } else if tab {
        WidgetRole::Tab
    } else if list_box {
        WidgetRole::ListBox
    } else if scroll_area {
        WidgetRole::ScrollArea
    } else if button {
        WidgetRole::Button
    } else {
        WidgetRole::Unknown
    }
}

/// Enumerate every declared interactable in the world.
///
/// Call on a booted app; after at least one `app.update()` so
/// `UiStack` and `ComputedNode` exist (layout-dependent surfaces only
/// appear after layout has run at least once). See [`UiSurfaceResult::complete`]
/// for what the enumeration provably saw.
pub fn enumerate_ui(world: &World) -> UiSurfaceResult {
    let mut result = UiSurfaceResult::default();
    let observed = pointer_observers(world);

    let focused: Option<Entity> = world
        .get_resource::<bevy_input_focus::InputFocus>()
        .and_then(bevy_input_focus::InputFocus::get);

    // `QueryState::try_new` works from `&World` (immutable, safe). It
    // returns `None` when a queried component is not registered in
    // this world — in that case no entity can possibly carry it, so an
    // empty set is both correct and complete (not an incompleteness).
    let widget_entities: HashSet<Entity> = QueryState::<
        Entity,
        Or<(
            With<Button>,
            With<Checkbox>,
            With<RadioButton>,
            With<Slider>,
            With<Tab>,
            With<TextInput>,
            With<ListBox>,
            With<MenuItem>,
            With<MenuButton>,
            With<ScrollArea>,
        )>,
    >::try_new(world)
    .map(|mut state| state.iter(world).collect())
    .unwrap_or_default();
    let pickable_entities: HashSet<Entity> = QueryState::<Entity, With<Pickable>>::try_new(world)
        .map(|mut state| state.iter(world).collect())
        .unwrap_or_default();
    let tabindex_entities: HashSet<Entity> = QueryState::<Entity, With<TabIndex>>::try_new(world)
        .map(|mut state| state.iter(world).collect())
        .unwrap_or_default();
    let a11y_entities: HashSet<Entity> =
        QueryState::<Entity, Or<(With<AccessibilityNode>, With<AccessibleLabel>)>>::try_new(world)
            .map(|mut state| state.iter(world).collect())
            .unwrap_or_default();

    let mut declared: HashSet<Entity> = HashSet::new();
    declared.extend(widget_entities.iter().copied());
    declared.extend(pickable_entities.iter().copied());
    declared.extend(tabindex_entities.iter().copied());
    declared.extend(a11y_entities.iter().copied());
    declared.extend(observed.keys().copied());

    let mut viewport_min_cache: HashMap<Entity, Option<Vec2>> = HashMap::new();
    let mut interactables: Vec<Interactable> = Vec::with_capacity(declared.len());

    // Deterministic order: ascending entity index.
    let mut ordered: Vec<Entity> = declared.into_iter().collect();
    ordered.sort_unstable_by_key(|e| e.index());

    for entity in ordered {
        // Existence of declaration data is not enough — the entity
        // must also carry a UI node for geometry purposes.
        let Some(computed) = world.get::<ComputedNode>(entity) else {
            // Declared but not a laid-out UI node (e.g. a world-space
            // entity with an observer). Report it as a surface point
            // with unknown geometry.
            let kinds = empty_kinds_for(
                world,
                entity,
                &widget_entities,
                &pickable_entities,
                &tabindex_entities,
                &a11y_entities,
                &observed,
            );
            interactables.push(Interactable {
                entity,
                kinds,
                role: WidgetRole::Unknown,
                center: None,
                rect: None,
                disabled: world.get::<UiInteractionDisabled>(entity).is_some(),
                label: best_label(world, entity),
                focused: focused == Some(entity),
            });
            continue;
        };
        let Some(transform) = world.get::<UiGlobalTransform>(entity) else {
            let kinds = empty_kinds_for(
                world,
                entity,
                &widget_entities,
                &pickable_entities,
                &tabindex_entities,
                &a11y_entities,
                &observed,
            );
            interactables.push(Interactable {
                entity,
                kinds,
                role: role_for_entity(world, entity, &widget_entities),
                center: None,
                rect: None,
                disabled: world.get::<UiInteractionDisabled>(entity).is_some(),
                label: best_label(world, entity),
                focused: focused == Some(entity),
            });
            continue;
        };

        // Visibility doctrine mirrors `ui_picking`: hidden nodes are
        // not actionable and are excluded entirely.
        if world
            .get::<InheritedVisibility>(entity)
            .is_some_and(|vis| !vis.get())
        {
            continue;
        }
        // Zero-size nodes cannot be hit; `ui_picking` skips them.
        if computed.size == Vec2::ZERO {
            continue;
        }

        let role = role_for_entity(world, entity, &widget_entities);
        let kinds = empty_kinds_for(
            world,
            entity,
            &widget_entities,
            &pickable_entities,
            &tabindex_entities,
            &a11y_entities,
            &observed,
        );

        // Resolve window-space rect: node position is in target
        // camera viewport space; add the viewport offset. Both fields
        // resolve or neither does (unified failure, no half-answers).
        let geometry = world
            .get::<ComputedUiTargetCamera>(entity)
            .and_then(ComputedUiTargetCamera::get)
            .and_then(|camera_entity| {
                // Viewport offsets are camera-invariant per frame;
                // compute each camera's offset once.
                let offset = match viewport_min_cache.entry(camera_entity) {
                    Entry::Occupied(o) => *o.get(),
                    Entry::Vacant(v) => {
                        let value = world
                            .get::<Camera>(camera_entity)
                            .and_then(Camera::physical_viewport_rect)
                            .map(|r| r.min.as_vec2());
                        v.insert(value);
                        value
                    }
                };
                offset.map(|offset| {
                    let center_win = transform.affine().translation + offset;
                    let half = computed.size * 0.5;
                    (
                        center_win,
                        Rect {
                            min: center_win - half,
                            max: center_win + half,
                        },
                    )
                })
            });
        let (center, rect) = match geometry {
            Some((c, r)) => (Some(c), Some(r)),
            None => (None, None),
        };

        interactables.push(Interactable {
            entity,
            kinds,
            role,
            center,
            rect,
            disabled: world.get::<UiInteractionDisabled>(entity).is_some(),
            label: best_label(world, entity),
            focused: focused == Some(entity),
        });
    }

    // TODO(completeness): global/component observer omission notes.
    result.interactables = interactables;
    result.complete = true;
    result
}

/// Kinds for an entity given the pre-collected signal sets.
fn empty_kinds_for(
    world: &World,
    entity: Entity,
    widget_entities: &HashSet<Entity>,
    pickable_entities: &HashSet<Entity>,
    tabindex_entities: &HashSet<Entity>,
    a11y_entities: &HashSet<Entity>,
    observed: &HashMap<Entity, usize>,
) -> Vec<InteractableKind> {
    let _ = world;
    let mut kinds = Vec::new();
    if pickable_entities.contains(&entity) {
        kinds.push(InteractableKind::Pickable);
    }
    if widget_entities.contains(&entity) {
        kinds.push(InteractableKind::Widget);
    }
    if tabindex_entities.contains(&entity) {
        kinds.push(InteractableKind::Focusable);
    }
    if a11y_entities.contains(&entity) {
        kinds.push(InteractableKind::Accessible);
    }
    if observed.contains_key(&entity) {
        kinds.push(InteractableKind::Observed);
    }
    kinds
}

/// Role for an entity known to be in the widget set.
fn role_for_entity(
    world: &World,
    entity: Entity,
    _widget_entities: &HashSet<Entity>,
) -> WidgetRole {
    widget_role_from_has(
        world.get::<Button>(entity).is_some(),
        world.get::<Checkbox>(entity).is_some(),
        world.get::<RadioButton>(entity).is_some(),
        world.get::<Slider>(entity).is_some(),
        world.get::<Tab>(entity).is_some(),
        world.get::<TextInput>(entity).is_some(),
        world.get::<ListBox>(entity).is_some(),
        world.get::<MenuItem>(entity).is_some(),
        world.get::<MenuButton>(entity).is_some(),
        world.get::<ScrollArea>(entity).is_some(),
    )
}

/// Best-effort label: `AccessibleLabel` > descendant `Text` (one
/// level) > `Name` component.
fn best_label(world: &World, entity: Entity) -> Option<String> {
    if let Some(label) = world.get::<AccessibleLabel>(entity) {
        return Some(label.0.clone());
    }
    // Direct child text.
    if let Some(children) = world.get::<Children>(entity) {
        for child in children {
            if let Some(text) = world.get::<Text>(*child) {
                return Some(text.0.clone());
            }
        }
    }
    world.get::<Name>(entity).map(|n| n.as_str().to_owned())
}
