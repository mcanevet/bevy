//! Runtime enumeration of a Bevy game's declared input surface.
//!
//! **The problem.** An external driver — a bot explorer, a
//! Playwright-style test client, an accessibility tool, a remapping
//! UI — needs to answer "*what inputs does this game accept right
//! now, and where?*" Bevy 0.21's synthetic-input stack solved the
//! *actuation* half (how to press). Nothing upstream answers the
//! *enumeration* half (what to press): the ECS computes everything
//! needed — picking hit-tests every frame, focus systems track
//! candidates, widgets carry roles, observers declare
//! click-responsiveness — but never writes the answer down anywhere
//! a program can read it. Harnesses therefore rediscover the surface
//! empirically, game by game.
//!
//! **This crate** is the missing reader, for Bevy 0.20.1 (all APIs
//! verified against that exact tag). Three enumeration tiers, one
//! call:
//!
//! ```
//! use bevy_app::prelude::*;
//! # use bevy_input_surface::enumerate_input_surface;
//! # fn doc(app: &mut App) {
//! let surface = enumerate_input_surface(app.world());
//! # }
//! ```
//!
//! - [`ui::enumerate_ui`] — every declared interactable: explicit
//!   `Pickable` markers, `bevy_ui_widgets` widgets, `TabIndex`
//!   focusables, accessibility nodes, and entities with pointer-event
//!   observers; each with screen-space geometry, role, label,
//!   disabled and focused state.
//! - [`raw_readers::raw_reader_census`] — which device classes
//!   (keyboard, mouse, wheel, gamepad) have systems reading them,
//!   derived from schedule access sets.
//! - The combined [`schema::InputSurface`] serializes to plain JSON —
//!   the contract for bots, remote clients, and diffs over time.
//!
//! **Honesty doctrine.** Enumeration reports what the game declares,
//! not what works. A declared binding can still be dead code:
//! consumers must verify liveness empirically (differential probing).
//! Every sub-result carries a `complete` flag; `false` means the
//! census provably missed things, and absence proves nothing. Counts
//! annotate; they never license pruning probe surfaces.
//!
//! **Blind spots.** Documented, never silent: global and
//! component-scoped pointer observers are not attributed per-entity;
//! deprecated `Interaction`/`Button` are ignored; third-party
//! action-map crates (LWIM, `enhanced_input`) need their own reflection
//! readers and are out of scope here.

#![deny(missing_docs)]

pub mod raw_readers;
pub mod schema;
pub mod ui;

use bevy_ecs::world::World;

pub use schema::{DeviceClass, InputSurface};

/// Enumerate the complete declared input surface of a world.
///
/// Call on a booted app (after at least one `app.update()` so layout
/// and schedules are initialized). Cheap enough to run per-frame;
/// designed for snapshotting at decision points.
pub fn enumerate_input_surface(world: &World) -> InputSurface {
    InputSurface {
        ui: ui::enumerate_ui(world),
        readers: raw_readers::raw_reader_census(world),
    }
}
