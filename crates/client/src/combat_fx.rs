//! Combat feedback: floating damage numbers, arrows for ranged attacks and
//! notices when our units or cities are involved.

use crate::icons::Icon;
use crate::layout::tile_center;
use crate::loading::InGameSet;
use crate::play::{GameEvent, NewsKind, Notice, Session};
use crate::terrain::{height::WATER_LEVEL, Terrain};
use crate::ui::{self, palette};
use crate::wrap::{nearest_shift, WrapPeriod};
use bevy::prelude::*;
use sim::combat::{Attacker, CombatReport};
use sim::game::Event;
use sim::hex::TileId;

/// Seconds a damage number stays up.
const NUMBER_LIFE: f32 = 1.6;
/// Seconds an arrow flies.
const ARROW_TIME: f32 = 0.45;

#[derive(Component)]
struct DamageNumber {
    world: Vec3,
    age: f32,
}

#[derive(Component)]
struct Arrow {
    from: Vec3,
    to: Vec3,
    age: f32,
}

pub struct CombatFxPlugin;

impl Plugin for CombatFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_effects, animate_numbers, draw_arrows).chain().after(crate::play::PlaySet).in_set(InGameSet));
    }
}

fn ground(terrain: &Terrain, session: &Session, tile: TileId) -> Vec3 {
    let p = tile_center(&session.game.map.grid, tile);
    Vec3::new(p.x, terrain.heights.sample(p).max(WATER_LEVEL), p.y)
}

fn spawn_effects(
    mut commands: Commands,
    session: Res<Session>,
    terrain: Res<Terrain>,
    mut events: MessageReader<GameEvent>,
    mut notice: ResMut<Notice>,
) {
    let me = session.me;
    let viewer = &session.game.players[me as usize];
    for GameEvent(event) in events.read() {
        match event {
            Event::Combat(r) => {
                let involved = r.attacker_owner == me || r.defender_owner == me;
                if !involved && !viewer.sees(r.tile) && !viewer.sees(r.from) {
                    continue;
                }
                let target = ground(&terrain, &session, r.tile);
                let source = ground(&terrain, &session, r.from);
                if r.damage_dealt > 0 {
                    number(&mut commands, target + Vec3::Y * 1.4, r.damage_dealt);
                }
                if r.damage_taken > 0 {
                    number(&mut commands, source + Vec3::Y * 1.4, r.damage_taken);
                }
                if r.ranged {
                    commands.spawn(Arrow { from: source + Vec3::Y * 0.6, to: target + Vec3::Y * 0.4, age: 0.0 });
                }
                if let Some((icon, text)) = describe(&session, r) {
                    let turn = session.game.turn;
                    if icon == Icon::Warning {
                        notice.alert(turn, NewsKind::Combat, text, Some(r.tile));
                    } else {
                        notice.news(turn, NewsKind::Combat, icon, text, Some(r.tile));
                    }
                }
            }
            Event::CityCaptured { city, from } => {
                let Some(c) = session.game.cities.get(city) else { continue };
                let turn = session.game.turn;
                if c.owner == me {
                    notice.news(turn, NewsKind::Combat, Icon::FoundCity, format!("You captured {}!", c.name), Some(c.tile));
                } else if *from == me {
                    let by = &session.game.players[c.owner as usize].name;
                    notice.alert(turn, NewsKind::Combat, format!("{} was captured by {by}!", c.name), Some(c.tile));
                }
            }
            Event::CityRazed { name, tile, by, from, .. } => {
                let turn = session.game.turn;
                if *by == me {
                    notice.news(turn, NewsKind::Combat, Icon::Warning, format!("You razed {name} to the ground"), Some(*tile));
                } else if *from == me {
                    let by = &session.game.players[*by as usize].name;
                    notice.alert(turn, NewsKind::Combat, format!("{by} razed {name} to the ground!"), Some(*tile));
                }
            }
            Event::PostDestroyed { tile, owner, by, fort } if *owner == me || *by == me => {
                let game = &session.game;
                let what = if *fort { "fort" } else { "watchtower" };
                if *owner == me {
                    let name = &game.players[*by as usize].name;
                    notice.alert(game.turn, NewsKind::Combat, format!("{name} pulled down your {what}!"), Some(*tile));
                } else {
                    let of = &game.players[*owner as usize].name;
                    notice.news(game.turn, NewsKind::Combat, Icon::Strength, format!("Your army pulled down {of}'s {what}"), Some(*tile));
                }
            }
            Event::LostWithCity { kind, owner, city } if *owner == me => {
                let Some(c) = session.game.cities.get(city) else { continue };
                let name = &session.game.rules.unit(*kind).name;
                notice.alert(session.game.turn, NewsKind::Combat, format!("Your {name} in {} had nowhere to go and was lost", c.name), Some(c.tile));
            }
            Event::UnitCaptured { unit, from } => {
                let Some(u) = session.game.units.get(unit) else { continue };
                let name = &session.game.unit_def(u).name;
                let turn = session.game.turn;
                if u.owner == me {
                    notice.news(turn, NewsKind::Combat, Icon::unit(&session.game.unit_def(u).art), format!("Captured a {name}"), Some(u.tile));
                } else if *from == me {
                    notice.alert(turn, NewsKind::Combat, format!("Your {name} was captured"), Some(u.tile));
                }
            }
            _ => {}
        }
    }
}

/// A message for a fight we were part of (captures have their own).
fn describe(session: &Session, r: &CombatReport) -> Option<(Icon, String)> {
    let game = &session.game;
    let me = session.me;
    let attacker = match r.attacker {
        Attacker::Unit { kind, .. } => game.rules.unit(kind).name.clone(),
        Attacker::City(c) => game.cities.get(&c).map_or("city".into(), |c| c.name.clone()),
        Attacker::Fort(_) => "fort".into(),
    };
    let owner = |p| game.players[p as usize].name.clone();
    let defender = match (r.city, r.defender_kind) {
        (Some(c), _) => game.cities.get(&c).map_or("the city".into(), |c| c.name.clone()),
        (None, Some(k)) => game.rules.unit(k).name.clone(),
        (None, None) => return None,
    };
    if r.city.is_some() && r.defender_destroyed {
        return None;
    }
    // "Your Warrior", but a city by its name.
    let subject = match r.attacker {
        Attacker::Unit { .. } => format!("Your {attacker}"),
        Attacker::City(_) => attacker.clone(),
        Attacker::Fort(_) if r.attacker_owner == me => "Your fort".into(),
        Attacker::Fort(_) => format!("{}'s fort", game.players[r.attacker_owner as usize].name),
    };
    if r.attacker_owner == me {
        Some(if r.defender_destroyed {
            (Icon::Strength, format!("{subject} destroyed {}'s {defender}", owner(r.defender_owner)))
        } else if r.attacker_died {
            (Icon::Warning, format!("{subject} died attacking {}'s {defender}", owner(r.defender_owner)))
        } else {
            (Icon::Strength, format!("{subject} hit {defender} for {} damage", r.damage_dealt))
        })
    } else if r.defender_owner == me {
        let who = format!("{}'s {attacker}", owner(r.attacker_owner));
        Some(if r.defender_destroyed {
            (Icon::Warning, format!("Your {defender} was destroyed by {who}"))
        } else {
            (Icon::Warning, format!("{who} attacked your {defender} ({} damage)", r.damage_dealt))
        })
    } else {
        None
    }
}

fn number(commands: &mut Commands, world: Vec3, damage: i32) {
    commands.spawn((
        DamageNumber { world, age: 0.0 },
        Text::new(format!("-{damage}")),
        ui::font_bold(22.0),
        TextColor(palette::DANGER),
        TextShadow { offset: Vec2::new(1.5, 1.5), color: Color::BLACK },
        Node { position_type: PositionType::Absolute, ..default() },
        UiTransform::from_translation(Val2::percent(-50.0, -50.0)),
        Visibility::Hidden,
        GlobalZIndex(-1),
        Pickable::IGNORE,
    ));
}

fn animate_numbers(
    mut commands: Commands,
    time: Res<Time>,
    period: Res<WrapPeriod>,
    ui_scale: Res<UiScale>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    mut numbers: Query<(Entity, &mut DamageNumber, &mut Node, &mut TextColor, &mut Visibility)>,
) {
    let (cam, cam_tf) = *camera;
    let cam_x = cam_tf.translation().x;
    for (entity, mut n, mut node, mut color, mut vis) in &mut numbers {
        n.age += time.delta_secs();
        if n.age > NUMBER_LIFE {
            commands.entity(entity).despawn();
            continue;
        }
        let mut world = n.world;
        world.x += nearest_shift(world.x, cam_x, period.0);
        let Ok(screen) = cam.world_to_viewport(cam_tf, world) else { continue };
        // Rise, and fade out over the last half second.
        let rise = 40.0 * (n.age / NUMBER_LIFE).sqrt();
        node.left = px(screen.x / ui_scale.0);
        node.top = px((screen.y - rise) / ui_scale.0);
        color.0 = palette::DANGER.with_alpha(((NUMBER_LIFE - n.age) * 2.0).clamp(0.0, 1.0));
        *vis = Visibility::Inherited;
    }
}

/// Arrows fly on an arc from the archer to the target.
fn draw_arrows(
    mut commands: Commands,
    mut gizmos: Gizmos,
    time: Res<Time>,
    period: Res<WrapPeriod>,
    camera: Single<&GlobalTransform, With<crate::camera::MainCamera>>,
    mut arrows: Query<(Entity, &mut Arrow)>,
) {
    let cam_x = camera.translation().x;
    for (entity, mut a) in &mut arrows {
        a.age += time.delta_secs();
        if a.age > ARROW_TIME {
            commands.entity(entity).despawn();
            continue;
        }
        let shift = Vec3::X * nearest_shift(a.from.x, cam_x, period.0);
        let mut to = a.to;
        to.x += nearest_shift(to.x, a.from.x, period.0);
        let at = |t: f32| a.from.lerp(to, t) + Vec3::Y * (4.0 * t * (1.0 - t)) * 1.2 + shift;
        let t = a.age / ARROW_TIME;
        let head = at(t);
        let tail = at((t - 0.12).max(0.0));
        gizmos.line(tail, head, Color::srgb(0.25, 0.18, 0.1));
        gizmos.sphere(Isometry3d::from_translation(head), 0.03, Color::srgb(0.8, 0.8, 0.85));
    }
}
