//! Headless tests for input surface enumeration.
//!
//! The test app mirrors `bevy_ui`'s own layout-test bootstrap
//! (crates/`bevy_ui`/src/layout/mod.rs `setup_ui_test_app`): a
//! `Camera2d` with a synthetic `RenderTargetInfo` + explicit
//! `Viewport` provides layout a render target without a GPU, and
//! `UiPlugin` schedules the full layout/stack/clipping pipeline.
//! `bevy_ui_widgets`' plugin adds widget observers so widget-marker
//! enumeration matches a real game's world.

use bevy_app::prelude::*;
use bevy_camera::{
    visibility::Visibility, Camera, Camera2d, ComputedCameraValues, RenderTargetInfo, Viewport,
};
use bevy_ecs::prelude::*;
use bevy_input::prelude::*;
use bevy_input_surface::enumerate_input_surface;
use bevy_input_surface::schema::{DeviceClass, InteractableKind, WidgetRole};
use bevy_input_surface::ui::enumerate_ui;
use bevy_math::{UVec2, Vec2};
use bevy_picking::events::PointerClick;
use bevy_picking::Pickable;
use bevy_transform::prelude::*;
use bevy_ui::prelude::*;
use bevy_ui::UiPlugin;
use bevy_ui_widgets as widgets;
use bevy_utils::default;

const TARGET_WIDTH: u32 = 1280;
const TARGET_HEIGHT: u32 = 720;

fn setup_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        bevy_asset::AssetPlugin::default(),
        bevy_window::WindowPlugin::default(),
        TransformPlugin,
        bevy_text::TextPlugin,
        UiPlugin,
        bevy_camera::visibility::VisibilityPlugin,
        bevy_input_focus::InputFocusPlugin,
        // Registers pointer messages + `PointerState` that UiPlugin's
        // text-interaction and widget systems read.
        bevy_picking::InteractionPlugin,
        bevy_picking::PickingPlugin,
        widgets::UiWidgetsPlugins,
    ));
    app.init_resource::<bevy_text::TextPipeline>();
    app.init_resource::<bevy_text::FontCx>();
    app.init_resource::<bevy_text::ScaleCx>();
    // `update_inline_image_boxes` and `update_image_content_size_system`
    // (UiPlugin) read `Assets<Image>` / `Assets<TextureAtlasLayout>`;
    // normally registered by render's `ImagePlugin`, absent headless.
    bevy_asset::AssetApp::init_asset::<bevy_image::Image>(&mut app);
    bevy_asset::AssetApp::init_asset::<bevy_image::TextureAtlasLayout>(&mut app);
    // `VisibilityPlugin`'s auto-AABB systems read `Assets<Mesh>`.
    bevy_asset::AssetApp::init_asset::<bevy_mesh::Mesh>(&mut app);
    bevy_asset::AssetApp::init_asset::<bevy_mesh::skinning::SkinnedMeshInverseBindposes>(&mut app);
    // Resources the UiPlugin/widget systems read that are normally
    // registered by render/time/picking plugin groups, absent headless.
    app.init_resource::<bevy_time::Time<bevy_time::Real>>();
    app.init_resource::<UiScale>();
    // Resources that `ui_focus_system` requires; normally provided by
    // the windowing/input plugin group, absent in this headless test.
    app.init_resource::<ButtonInput<MouseButton>>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<Touches>();
    app.init_resource::<bevy_input_focus::InputFocus>();
    app.init_resource::<bevy_input_focus::InputFocusVisible>();

    let world = app.world_mut();
    // Dummy headless camera (the bevy_ui layout-test pattern).
    world.spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(TARGET_WIDTH, TARGET_HEIGHT),
                    scale_factor: 1.,
                }),
                ..Default::default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::new(TARGET_WIDTH, TARGET_HEIGHT),
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    app
}

fn run_frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

#[test]
fn buttons_are_enumerated_with_geometry_and_labels() {
    let mut app = setup_app();

    let world = app.world_mut();
    let button = world
        .spawn((
            Node {
                width: Val::Px(200.),
                height: Val::Px(80.),
                position_type: PositionType::Absolute,
                left: Val::Px(100.),
                top: Val::Px(50.),
                ..default()
            },
            widgets::Button,
            Pickable::default(),
        ))
        .with_children(|parent| {
            parent.spawn((Text::new("Resume"),));
        })
        .id();

    run_frames(&mut app, 2);

    let surface = enumerate_ui(app.world());
    let hit = surface
        .interactables
        .iter()
        .find(|i| i.entity == button)
        .expect("button must be enumerated");
    assert!(hit.kinds.contains(&InteractableKind::Widget));
    assert!(hit.kinds.contains(&InteractableKind::Pickable));
    assert_eq!(hit.role, WidgetRole::Button);
    assert_eq!(hit.label.as_deref(), Some("Resume"));

    let rect = hit.rect.expect("geometry must resolve");
    // Absolute (100, 50), 200x80 → window-space rect (100,50)-(300,130).
    assert!((rect.min.x - 100.).abs() < 0.5, "got {rect:?}");
    assert!((rect.min.y - 50.).abs() < 0.5, "got {rect:?}");
    assert!((rect.max.x - 300.).abs() < 0.5, "got {rect:?}");
    assert!((rect.max.y - 130.).abs() < 0.5, "got {rect:?}");
    assert_eq!(hit.center, Some(Vec2::new(200., 90.)));
}

#[test]
fn tab_index_focusable_is_enumerated() {
    let mut app = setup_app();

    app.world_mut().spawn((
        Node {
            width: Val::Px(50.),
            height: Val::Px(50.),
            position_type: PositionType::Absolute,
            left: Val::Px(10.),
            top: Val::Px(10.),
            ..default()
        },
        bevy_input_focus::tab_navigation::TabIndex(0),
    ));

    run_frames(&mut app, 2);

    let surface = enumerate_ui(app.world());
    assert!(
        surface
            .interactables
            .iter()
            .any(|i| i.kinds.contains(&InteractableKind::Focusable)),
        "TabIndex entity must be enumerated"
    );
}

#[test]
fn pointer_observer_declares_interactable() {
    let mut app = setup_app();

    // A bare Node with a click observer: no markers at all. This is
    // the "game wired clicks imperatively" case that marker-based
    // enumeration misses entirely.
    app.world_mut()
        .spawn(Node {
            width: Val::Px(64.),
            height: Val::Px(64.),
            position_type: PositionType::Absolute,
            left: Val::Px(0.),
            top: Val::Px(0.),
            ..default()
        })
        .observe(|_: On<PointerClick>| {});

    run_frames(&mut app, 2);

    let surface = enumerate_ui(app.world());
    assert!(
        surface
            .interactables
            .iter()
            .any(|i| i.kinds.contains(&InteractableKind::Observed)),
        "entity with a click observer must be enumerated"
    );
}

#[test]
fn census_detects_keyboard_reader() {
    let mut app = setup_app();

    fn keyboard_reader(keys: Res<ButtonInput<KeyCode>>) {
        let _ = keys.get_just_pressed().count();
    }
    app.add_systems(Update, keyboard_reader);
    run_frames(&mut app, 2);

    let surface = enumerate_input_surface(app.world());
    assert!(
        surface
            .readers
            .readers
            .iter()
            .any(|(class, count)| *class == DeviceClass::Keyboard && *count > 0),
        "keyboard reader system must be counted: {:?}",
        surface.readers.readers
    );
}

#[test]
fn hidden_nodes_are_excluded() {
    let mut app = setup_app();

    let world = app.world_mut();
    let hidden = world
        .spawn((
            Node {
                width: Val::Px(50.),
                height: Val::Px(50.),
                position_type: PositionType::Absolute,
                left: Val::Px(10.),
                top: Val::Px(10.),
                ..default()
            },
            Visibility::Hidden,
            widgets::Button,
        ))
        .id();

    run_frames(&mut app, 2);

    let surface = enumerate_ui(app.world());
    assert!(
        !surface.interactables.iter().any(|i| i.entity == hidden),
        "hidden button must be excluded"
    );
}

#[test]
fn focused_flag_tracks_input_focus() {
    let mut app = setup_app();

    let world = app.world_mut();
    let target = world
        .spawn((
            Node {
                width: Val::Px(50.),
                height: Val::Px(50.),
                position_type: PositionType::Absolute,
                ..default()
            },
            widgets::Button,
        ))
        .id();
    world.insert_resource(bevy_input_focus::InputFocus::from_entity(target));

    run_frames(&mut app, 2);

    let surface = enumerate_ui(app.world());
    let hit = surface
        .interactables
        .iter()
        .find(|i| i.entity == target)
        .expect("focused button enumerated");
    assert!(hit.focused);
}
