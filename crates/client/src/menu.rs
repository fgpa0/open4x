//! In-game menus: ESC opens the pause menu, which leads to settings.

use crate::settings::{AntiAliasing, DisplayMode, PopUps, Settings, ShadowQuality, AUTOSAVE_EVERY, DAY_LENGTHS, FPS_CAPS, UI_SCALES};
use crate::loading::{AppState, InGameSet};
use crate::ui;
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Menu {
    #[default]
    Closed,
    Pause,
    Settings,
    /// The list of saves, over a running game.
    Load,
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Menu>()
            .add_systems(Startup, open_from_env)
            .add_systems(Update, (handle_escape.in_set(InGameSet), refresh_setting_values))
            .add_systems(OnEnter(Menu::Pause), spawn_pause_menu)
            .add_systems(OnEnter(Menu::Settings), spawn_settings_menu);
    }
}

/// FOURX_MENU=pause|settings opens a menu at startup (for screenshots and testing).
fn open_from_env(mut next: ResMut<NextState<Menu>>) {
    match std::env::var("FOURX_MENU").as_deref() {
        Ok("pause") => next.set(Menu::Pause),
        Ok("settings") => next.set(Menu::Settings),
        _ => {}
    }
}

fn handle_escape(
    keys: Res<ButtonInput<KeyCode>>,
    menu: Res<State<Menu>>,
    mut next: ResMut<NextState<Menu>>,
    mut city: ResMut<crate::play::SelectedCity>,
    mut tree: ResMut<crate::research_ui::TechTreeOpen>,
    mut merchants: ResMut<crate::merchants::MerchantScreen>,
    mut placing: ResMut<crate::placement::Placing>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        // Esc first cancels choosing a canal's, tunnel's or bridge's site.
        if *menu.get() == Menu::Closed && placing.0.is_some() {
            placing.0 = None;
            return;
        }
        // Esc first closes the trade routes screen.
        if *menu.get() == Menu::Closed && merchants.open {
            *merchants = crate::merchants::MerchantScreen::default();
            return;
        }
        // Esc first closes the tech tree, then an open city panel.
        if *menu.get() == Menu::Closed && tree.0 {
            tree.0 = false;
            return;
        }
        if *menu.get() == Menu::Closed && city.0.is_some() {
            city.0 = None;
            return;
        }
        next.set(match menu.get() {
            Menu::Closed => Menu::Pause,
            Menu::Pause => Menu::Closed,
            Menu::Settings | Menu::Load => Menu::Pause,
        });
    }
}

/// Full-screen dimmed backdrop with a centred panel, removed when leaving `state`.
fn backdrop(state: Menu) -> impl Bundle {
    (
        DespawnOnExit(state),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        // Menus draw above the HUD and debug overlay.
        GlobalZIndex(10),
    )
}

fn go_to(state: Menu) -> impl Fn(On<Activate>, ResMut<NextState<Menu>>) {
    move |_: On<Activate>, mut next: ResMut<NextState<Menu>>| next.set(state)
}

fn spawn_pause_menu(mut commands: Commands) {
    commands.spawn(backdrop(Menu::Pause)).with_children(|root| {
        root.spawn(ui::panel(px(320))).with_children(|panel| {
            panel.spawn(ui::title("Paused"));
            panel.spawn((ui::button("Resume"), observe(go_to(Menu::Closed))));
            panel.spawn((
                ui::button("Save Game"),
                observe(|_: On<Activate>, session: Res<crate::play::Session>, mut notice: ResMut<crate::play::Notice>, mut next: ResMut<NextState<Menu>>| {
                    match crate::saves::save_now(&session) {
                        Ok(_) => {
                            let civ = &session.game.civ(session.me).name;
                            notice.inform(crate::icons::Icon::Check, format!("Game saved: {civ}, turn {}", session.game.turn));
                        }
                        Err(e) => notice.show(format!("Couldn't save: {e}")),
                    }
                    next.set(Menu::Closed);
                }),
            ));
            panel.spawn((ui::button("Load Game"), observe(go_to(Menu::Load))));
            panel.spawn((ui::button("Settings"), observe(go_to(Menu::Settings))));
            panel.spawn((
                ui::button("Exit to Main Menu"),
                observe(|_: On<Activate>, mut next: ResMut<NextState<AppState>>| next.set(AppState::MainMenu)),
            ));
            panel.spawn((
                ui::button("Quit to Desktop"),
                observe(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                }),
            ));
            panel.spawn(ui::hint("Esc to resume"));
        });
    });
}

/// Every adjustable setting, with how to step through its values.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum SettingKey {
    FpsCap,
    Vsync,
    DisplayMode,
    Shadows,
    AmbientOcclusion,
    AntiAliasing,
    Bloom,
    Fog,
    UiScale,
    Autosave,
    PopUps,
    Yields,
    DayLength,
    Weather,
    Ambience,
}

const ALL_SETTINGS: [(SettingKey, &str); 15] = [
    (SettingKey::DisplayMode, "Display mode"),
    (SettingKey::Vsync, "VSync"),
    (SettingKey::FpsCap, "Frame rate cap"),
    (SettingKey::Shadows, "Shadows"),
    (SettingKey::AmbientOcclusion, "Ambient occlusion"),
    (SettingKey::AntiAliasing, "Anti-aliasing"),
    (SettingKey::Bloom, "Bloom"),
    (SettingKey::Fog, "Distance fog"),
    (SettingKey::DayLength, "Day and night"),
    (SettingKey::Weather, "Weather"),
    (SettingKey::Ambience, "Wildlife and ambience"),
    (SettingKey::UiScale, "Interface scale"),
    (SettingKey::Autosave, "Autosave"),
    (SettingKey::PopUps, "Pop-up notices"),
    (SettingKey::Yields, "Tile yields (Y)"),
];

fn cycle<T: Copy + PartialEq>(values: &[T], current: T, step: i32) -> T {
    let i = values.iter().position(|v| *v == current).unwrap_or(0) as i32;
    values[(i + step).rem_euclid(values.len() as i32) as usize]
}

fn on_off(b: bool) -> String {
    if b { "On" } else { "Off" }.into()
}

impl SettingKey {
    fn step(self, s: &mut Settings, step: i32) {
        match self {
            SettingKey::FpsCap => s.fps_cap = cycle(&FPS_CAPS, s.fps_cap, step),
            SettingKey::Vsync => s.vsync = !s.vsync,
            SettingKey::DisplayMode => {
                s.display_mode = cycle(
                    &[DisplayMode::Windowed, DisplayMode::Borderless, DisplayMode::Fullscreen],
                    s.display_mode,
                    step,
                )
            }
            SettingKey::Shadows => {
                s.shadows = cycle(
                    &[ShadowQuality::Off, ShadowQuality::Low, ShadowQuality::Medium, ShadowQuality::High],
                    s.shadows,
                    step,
                )
            }
            SettingKey::AmbientOcclusion => s.ambient_occlusion = !s.ambient_occlusion,
            SettingKey::AntiAliasing => {
                s.anti_aliasing = cycle(
                    &[AntiAliasing::Off, AntiAliasing::Fxaa, AntiAliasing::Taa],
                    s.anti_aliasing,
                    step,
                )
            }
            SettingKey::Bloom => s.bloom = !s.bloom,
            SettingKey::Fog => s.fog = !s.fog,
            SettingKey::UiScale => s.ui_scale = cycle(&UI_SCALES, s.ui_scale, step),
            SettingKey::Autosave => s.autosave_every = cycle(&AUTOSAVE_EVERY, s.autosave_every, step),
            SettingKey::PopUps => s.popups = cycle(&[PopUps::All, PopUps::Alerts, PopUps::Off], s.popups, step),
            SettingKey::Yields => s.show_yields = !s.show_yields,
            SettingKey::DayLength => s.day_length = cycle(&DAY_LENGTHS, s.day_length, step),
            SettingKey::Weather => s.weather = !s.weather,
            SettingKey::Ambience => s.ambience = !s.ambience,
        }
    }

    fn display(self, s: &Settings) -> String {
        match self {
            SettingKey::FpsCap if s.fps_cap == 0 => "Unlimited".into(),
            SettingKey::FpsCap => format!("{} FPS", s.fps_cap),
            SettingKey::Vsync => on_off(s.vsync),
            SettingKey::DisplayMode => match s.display_mode {
                DisplayMode::Windowed => "Windowed",
                DisplayMode::Borderless => "Borderless",
                DisplayMode::Fullscreen => "Fullscreen",
            }
            .into(),
            SettingKey::Shadows => format!("{:?}", s.shadows),
            SettingKey::AmbientOcclusion => on_off(s.ambient_occlusion),
            SettingKey::AntiAliasing => match s.anti_aliasing {
                AntiAliasing::Off => "Off",
                AntiAliasing::Fxaa => "FXAA",
                AntiAliasing::Taa => "TAA",
            }
            .into(),
            SettingKey::Bloom => on_off(s.bloom),
            SettingKey::Fog => on_off(s.fog),
            SettingKey::UiScale => format!("{:.0}%", s.ui_scale * 100.0),
            SettingKey::Autosave => match s.autosave_every {
                0 => "Off".into(),
                1 => "Every turn".into(),
                n => format!("Every {n} turns"),
            },
            SettingKey::Yields => on_off(s.show_yields),
            SettingKey::DayLength if s.day_length == 0 => "Always day".into(),
            SettingKey::DayLength => format!("{}-minute days", s.day_length),
            SettingKey::Weather => on_off(s.weather),
            SettingKey::Ambience => on_off(s.ambience),
            SettingKey::PopUps => match s.popups {
                PopUps::All => "All news",
                PopUps::Alerts => "Alerts only",
                PopUps::Off => "Off",
            }
            .into(),
        }
    }
}

/// Text showing the current value of a setting.
#[derive(Component)]
struct SettingValue(SettingKey);

fn step_setting(key: SettingKey, step: i32) -> impl Fn(On<Activate>, ResMut<Settings>) {
    move |_: On<Activate>, mut settings: ResMut<Settings>| key.step(&mut settings, step)
}

fn spawn_settings_menu(mut commands: Commands, settings: Res<Settings>) {
    commands.spawn(backdrop(Menu::Settings)).with_children(|root| {
        root.spawn(ui::panel(px(520))).with_children(|panel| {
            panel.spawn(ui::title("Settings"));
            for (key, label) in ALL_SETTINGS {
                panel
                    .spawn(Node {
                        width: percent(100),
                        align_items: AlignItems::Center,
                        column_gap: px(8),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Node { flex_grow: 1.0, ..default() },
                            children![ui::label(label)],
                        ));
                        row.spawn((ui::small_button("<"), observe(step_setting(key, -1))));
                        row.spawn((
                            Node { width: px(130), justify_content: JustifyContent::Center, ..default() },
                            children![(ui::value_text(&key.display(&settings)), SettingValue(key))],
                        ));
                        row.spawn((ui::small_button(">"), observe(step_setting(key, 1))));
                    });
            }
            panel
                .spawn(Node { column_gap: px(12), margin: UiRect::top(px(10)), ..default() })
                .with_children(|row| {
                    row.spawn((
                        Node { width: px(210), ..default() },
                        children![(
                            ui::button("Reset to defaults"),
                            observe(|_: On<Activate>, mut settings: ResMut<Settings>| *settings = Settings::default()),
                        )],
                    ));
                    row.spawn((
                        Node { width: px(210), ..default() },
                        children![(
                            ui::button("Back"),
                            observe(|_: On<Activate>, state: Res<State<AppState>>, mut next: ResMut<NextState<Menu>>| {
                                // From the main menu there is no pause menu to return to.
                                next.set(if *state.get() == AppState::InGame { Menu::Pause } else { Menu::Closed });
                            }),
                        )],
                    ));
                });
            panel.spawn(ui::hint("Changes apply immediately and are saved.  Esc to go back"));
        });
    });
}

fn refresh_setting_values(settings: Res<Settings>, mut values: Query<(&SettingValue, &mut Text)>) {
    if settings.is_changed() {
        for (v, mut text) in &mut values {
            text.0 = v.0.display(&settings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_cycles_back_to_start() {
        for (key, _) in ALL_SETTINGS {
            let start = Settings::default();
            let mut s = start.clone();
            let mut seen = vec![key.display(&s)];
            for _ in 0..20 {
                key.step(&mut s, 1);
                if s == start {
                    break;
                }
                seen.push(key.display(&s));
            }
            assert_eq!(s, start, "{key:?} never returned to its default");
            assert!(seen.len() >= 2, "{key:?} has only one value");
            // Stepping back undoes stepping forward.
            key.step(&mut s, 1);
            key.step(&mut s, -1);
            assert_eq!(s, start, "{key:?} back/forward mismatch");
        }
    }

    #[test]
    fn settings_roundtrip_through_ron() {
        let s = Settings { fps_cap: 144, anti_aliasing: AntiAliasing::Fxaa, ..Default::default() };
        let text = ron::to_string(&s).unwrap();
        assert_eq!(ron::from_str::<Settings>(&text).unwrap(), s);
        // Missing fields fall back to defaults, so old settings files keep working.
        assert_eq!(ron::from_str::<Settings>("(fps_cap: 60)").unwrap().fps_cap, 60);
    }
}
