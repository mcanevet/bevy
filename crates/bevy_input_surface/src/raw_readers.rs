//! Raw-input reader census: which device classes have systems reading
//! them, by resolving input resource/message types to `ComponentId`s
//! and consulting each initialized schedule system's [`Access`] set.
//!
//! Adapted from a verified-in-production schedule-access scan (the
//! same `bevy_ecs` 0.20.1 mechanisms: `Schedules` resource,
//! `Schedule::systems_with_access`, `Access::has_read`/`has_write`,
//! `SystemAccess::Exclusive` fail-open arms). Doctrine carried over
//! unchanged:
//!
//! - Classify by `ComponentId` reads, NEVER by system name (names are
//!   masked in consumer builds). Known engine drivers are identified
//!   by EXACT name and skipped; unknown exclusives over-count.
//! - Keyboard is NEVER pruned based on this scan (run conditions can
//!   hold the access invisibly in 0.20.1 — conditions drain into
//!   executor storage on first build and become unreadable).
//! - Counts are annotation-grade: they license probe hints at most,
//!   never "this input does nothing".
//! - Completeness is separate from content: an uninitialized schedule
//!   hides its systems, so its zeros prove nothing and force
//!   `complete: false`.

use bevy_ecs::component::ComponentId;
use bevy_ecs::message::Messages;
use bevy_ecs::prelude::Schedules;
use bevy_ecs::query::Access;
use bevy_ecs::system::SystemAccess;
use bevy_input::gamepad::{
    Gamepad, GamepadAxis, GamepadAxisChangedEvent, GamepadButton, GamepadButtonChangedEvent,
    GamepadButtonStateChangedEvent, GamepadConnectionEvent, GamepadEvent, GamepadRumbleRequest,
    RawGamepadEvent,
};
use bevy_input::keyboard::{Key, KeyCode, KeyboardFocusLost, KeyboardInput};
use bevy_input::mouse::{
    AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton, MouseButtonInput, MouseMotion,
    MouseWheel,
};
use bevy_input::prelude::ButtonInput;
use bevy_input::Axis;
use bevy_window::CursorMoved;
use bevy_window::WindowEvent;
use std::any::TypeId;

use crate::schema::{DeviceClass, ReaderCensusResult};

/// Exact names of the engine's Exclusive driver systems (verified in
/// `bevy_ecs` 0.20.1): every App has `Main::run_main`,
/// `FixedMain::run_fixed_main`, and `message_update_system`. Skipping
/// them prevents every census returning phantom readers. Renaming or
/// masking degrades to over-counting — fail-open.
const ENGINE_DRIVER_NAMES: &[&str] = &[
    "bevy_app::main_schedule::Main::run_main",
    "bevy_app::main_schedule::FixedMain::run_fixed_main",
    "bevy_ecs::message::update::message_update_system",
];

/// The input signal resource/message `TypeId`s per device class.
///
/// Resources and messages share the `ComponentId` namespace
/// (`Components::get_valid_id` resolves both), so one lookup covers
/// `Res<ButtonInput<KeyCode>>`-style readers and
/// `MessageReader<KeyboardInput>`-style readers alike.
fn device_signal_types(world: &bevy_ecs::world::World) -> Vec<(DeviceClass, ComponentId)> {
    let mut types: Vec<(DeviceClass, TypeId)> = vec![
        (DeviceClass::Keyboard, TypeId::of::<ButtonInput<KeyCode>>()),
        // Logical key channel (`ButtonInput<Key>`), 0.20's enhanced-input era
        (DeviceClass::Keyboard, TypeId::of::<ButtonInput<Key>>()),
        (
            DeviceClass::Keyboard,
            TypeId::of::<Messages<KeyboardInput>>(),
        ),
        (
            DeviceClass::Keyboard,
            TypeId::of::<Messages<KeyboardFocusLost>>(),
        ),
        (DeviceClass::Keyboard, TypeId::of::<Messages<WindowEvent>>()),
        (
            DeviceClass::MouseButton,
            TypeId::of::<Messages<WindowEvent>>(),
        ),
        (
            DeviceClass::MouseMotion,
            TypeId::of::<Messages<WindowEvent>>(),
        ),
        (
            DeviceClass::MouseWheel,
            TypeId::of::<Messages<WindowEvent>>(),
        ),
        (
            DeviceClass::MouseButton,
            TypeId::of::<ButtonInput<MouseButton>>(),
        ),
        (
            DeviceClass::MouseButton,
            TypeId::of::<Messages<MouseButtonInput>>(),
        ),
        (
            DeviceClass::MouseMotion,
            TypeId::of::<AccumulatedMouseMotion>(),
        ),
        (
            DeviceClass::MouseWheel,
            TypeId::of::<AccumulatedMouseScroll>(),
        ),
        (
            DeviceClass::MouseMotion,
            TypeId::of::<Messages<CursorMoved>>(),
        ),
        (
            DeviceClass::MouseMotion,
            TypeId::of::<Messages<MouseMotion>>(),
        ),
        (
            DeviceClass::MouseWheel,
            TypeId::of::<Messages<MouseWheel>>(),
        ),
    ];

    types.extend([
        (DeviceClass::Gamepad, TypeId::of::<Gamepad>()),
        (
            DeviceClass::Gamepad,
            TypeId::of::<ButtonInput<GamepadButton>>(),
        ),
        (DeviceClass::Gamepad, TypeId::of::<Axis<GamepadAxis>>()),
    ]);

    types.extend([
        (DeviceClass::Gamepad, TypeId::of::<Messages<GamepadEvent>>()),
        (
            DeviceClass::Gamepad,
            TypeId::of::<Messages<RawGamepadEvent>>(),
        ),
        (
            DeviceClass::Gamepad,
            TypeId::of::<Messages<GamepadConnectionEvent>>(),
        ),
        (
            DeviceClass::Gamepad,
            TypeId::of::<Messages<GamepadButtonStateChangedEvent>>(),
        ),
        (
            DeviceClass::Gamepad,
            TypeId::of::<Messages<GamepadButtonChangedEvent>>(),
        ),
        (
            DeviceClass::Gamepad,
            TypeId::of::<Messages<GamepadAxisChangedEvent>>(),
        ),
        (
            DeviceClass::Gamepad,
            TypeId::of::<Messages<GamepadRumbleRequest>>(),
        ),
    ]);

    types
        .into_iter()
        .filter_map(|(class, ty)| {
            // Type-erased variant of valid_component_id::<T>: an
            // unregistered type yields None and is simply not tracked
            // (the game never touched that input plugin).
            world.components().get_valid_id(ty).map(|cid| (class, cid))
        })
        .collect()
}

/// Whether an access set touches the given `ComponentId`.
/// `Access::add_write` implies the read bit in 0.20.1 (verified), so
/// `has_read` alone suffices; the write check documents intent and
/// guards future semantics changes.
fn access_observes(access: &Access, cid: ComponentId) -> bool {
    access.has_read(cid) || access.has_write(cid)
}

/// Census of systems reading raw input signals.
pub fn raw_reader_census(world: &bevy_ecs::world::World) -> ReaderCensusResult {
    let mut result = ReaderCensusResult::default();
    let signal_ids = device_signal_types(world);

    let Some(schedules) = world.get_resource::<Schedules>() else {
        result
            .notes
            .push("no Schedules resource found at census time".to_owned());
        return result;
    };

    let mut complete = true;
    for (_label, schedule) in schedules.iter() {
        let Ok(systems) = schedule.systems_with_access() else {
            if schedule
                .graph()
                .systems
                .iter()
                .any(|(_key, system, _conds)| {
                    !ENGINE_DRIVER_NAMES.contains(&system.name().to_string().as_str())
                })
            {
                result.notes.push(
                    "at least one nonempty schedule was not initialized at census time".to_owned(),
                );
                complete = false;
            }
            continue;
        };

        for (_key, swa) in systems {
            let is_engine_driver =
                ENGINE_DRIVER_NAMES.contains(&swa.system().name().to_string().as_str());
            match swa.access() {
                SystemAccess::Exclusive => {
                    if is_engine_driver {
                        continue;
                    }
                    bump_all(&mut result);
                    result.notes.push(
                        "exclusive (&mut World) system present: every class over-counted"
                            .to_owned(),
                    );
                }
                SystemAccess::Shared(filtered) => {
                    let access = filtered.combined_access();
                    if access.has_read_all() || access.has_write_all() {
                        bump_all(&mut result);
                        result.notes.push(
                            "read-all system present (e.g. &World): every class over-counted"
                                .to_owned(),
                        );
                    } else {
                        let mut counted: Vec<DeviceClass> = Vec::new();
                        for (class, cid) in &signal_ids {
                            if !counted.contains(class) && access_observes(access, *cid) {
                                counted.push(*class);
                                bump(&mut result, *class);
                            }
                        }
                    }
                }
                SystemAccess::None => {}
            }
        }
    }

    result.complete = complete;
    result
}

fn bump(result: &mut ReaderCensusResult, class: DeviceClass) {
    if let Some(entry) = result.readers.iter_mut().find(|(c, _)| *c == class) {
        entry.1 += 1;
    } else {
        result.readers.push((class, 1));
    }
}

fn bump_all(result: &mut ReaderCensusResult) {
    for class in DeviceClass::ALL {
        bump(result, class);
    }
}
