//! Civilizations: who the players are, from `data/civs.ron`. Each civ has a
//! leader with drives and an agenda (read by the AI), two perks whose effects
//! the simulation applies, and unique units and buildings that replace
//! standard ones for that civ only.

use crate::game::{Game, PlayerId};
use crate::hex::TileId;
use crate::map::Elevation;
use crate::rules::{BuildingId, Rules, UnitTypeId, Yields};
use serde::{Deserialize, Serialize};

pub type CivId = u8;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CivColors {
    pub primary: (u8, u8, u8),
    pub secondary: (u8, u8, u8),
}

/// Terrain a civ likes to start near.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartBias {
    Coast,
    River,
    Lake,
    Desert,
    Plains,
    Grassland,
    Tundra,
    /// Forest in the cold boreal belt.
    Taiga,
    Hills,
    Mountains,
    Forest,
    Jungle,
    Marsh,
}

/// How strongly a leader leans towards each kind of behaviour, 0..10. The AI
/// combines them; no single drive means "aggressive".
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Drives {
    /// Land hunger: settling, wanting contiguous land.
    pub expansion: u8,
    /// Size of the standing army it wants relative to its rivals.
    pub military: u8,
    /// Attacks neighbours whose armies are weak.
    pub opportunism: u8,
    /// How strongly and how long it remembers being attacked.
    pub grudge: u8,
    /// Risk tolerance: high attacks at bad odds.
    pub risk: u8,
    /// High: becomes warlike only later in the game.
    pub patience: u8,
    /// Buildings over units.
    pub builder: u8,
    pub wealth: u8,
    pub science: u8,
    /// Walls and garrisons.
    pub defense: u8,
    pub seafaring: u8,
}

impl Drives {
    /// Every drive with its display name, in a fixed order.
    pub fn named(&self) -> [(&'static str, u8); 11] {
        [
            ("Expansion", self.expansion),
            ("Military", self.military),
            ("Opportunism", self.opportunism),
            ("Grudges", self.grudge),
            ("Risk", self.risk),
            ("Patience", self.patience),
            ("Building", self.builder),
            ("Wealth", self.wealth),
            ("Science", self.science),
            ("Defense", self.defense),
            ("Seafaring", self.seafaring),
        ]
    }
}

/// The kinds of troops a leader likes in its armies, 0..10 each (relative
/// weights). Siege engines only count when it besieges a city.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ArmyMix {
    pub melee: u8,
    pub ranged: u8,
    pub cavalry: u8,
    pub siege: u8,
}

impl Default for ArmyMix {
    fn default() -> Self {
        ArmyMix { melee: 5, ranged: 4, cavalry: 3, siege: 3 }
    }
}

impl ArmyMix {
    /// Each kind with its display name, in a fixed order.
    pub fn named(&self) -> [(&'static str, u8); 4] {
        [("Infantry", self.melee), ("Ranged", self.ranged), ("Cavalry", self.cavalry), ("Siege", self.siege)]
    }
}

/// The leader's personal reason to go to war (or stay out of one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgendaKind {
    MercantileRival,
    PaxRomana,
    RiverWarden,
    StrongTake,
    SleepingGiant,
    Plunder,
    NeverForgive,
    FlowerWars,
    GraciousConqueror,
    Tawantinsuyu,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Agenda {
    pub kind: AgendaKind,
    pub name: String,
    pub description: String,
}

/// A condition on a tile (all of a list must hold).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TileCond {
    Terrain(String),
    Feature(String),
    River,
    /// Next to a mountain.
    NearMountain,
    /// With this improvement built.
    Improvement(String),
}

/// A condition on a city.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CityCond {
    /// Next to the sea or a lake.
    Coastal,
}

/// When a combat bonus applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombatCond {
    /// The unit stands inside its owner's borders.
    OwnTerritory,
    /// Attacking from or onto a land tile next to water.
    Coastal,
    AttackingUnits,
    AttackingCities,
    /// The opponent is damaged.
    VsWounded,
    /// Flat land without forest, jungle or marsh.
    OpenGround,
    /// (For enemies) standing on flat land next to a river or water inside the
    /// civ's borders.
    InOwnLowlands,
    /// The opponent is a mounted unit.
    VsMounted,
    /// Being attacked (for General traits, see experience.ron).
    Defending,
}

/// What a perk (or a technology) does. The simulation applies these; see `Game::civ_effects`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    /// Extra yields on worked tiles matching all conditions.
    TileYield {
        when: Vec<TileCond>,
        #[serde(default)]
        food: i32,
        #[serde(default)]
        production: i32,
        #[serde(default)]
        gold: i32,
    },
    /// Extra yields for cities matching the condition.
    CityYield {
        when: CityCond,
        #[serde(default)]
        food: i32,
        #[serde(default)]
        production: i32,
        #[serde(default)]
        gold: i32,
    },
    /// Extra yields from a building (or the civ's replacement for it).
    BuildingBonus {
        building: String,
        #[serde(default)]
        food: i32,
        #[serde(default)]
        production: i32,
        #[serde(default)]
        gold: i32,
        #[serde(default)]
        science: i32,
        #[serde(default)]
        culture: i32,
    },
    /// New cities start with this building (or the civ's replacement).
    FreeBuilding(String),
    /// Faster buildings in cities of at least this size.
    BuildingProduction { percent: i32, min_population: i32 },
    /// A combat bonus for the civ's units.
    Combat { percent: i32, when: CombatCond },
    /// A combat modifier for units fighting against the civ.
    EnemyCombat { percent: i32, when: CombatCond },
    /// Extra movement for units starting their turn inside the civ's borders.
    MovesInTerritory(i32),
    /// Every land tile inside the civ's borders costs 1 movement.
    IgnoreTerrainInTerritory,
    /// New cities get a road to the capital.
    RoadToCapital,
    /// Gold per citizen of a captured city.
    CaptureGold(i32),
    /// Capturing a city fully heals the civ's units next to it.
    CaptureHeal,
    /// Gold for each enemy unit destroyed.
    KillGold(i32),
    /// Production to the nearest city for each enemy unit destroyed.
    KillProduction(i32),
    /// Units heal outside the civ's borders as fast as inside.
    HealAnywhere,
    /// Captured cities keep their citizens and stored production.
    KeepCapturedCities,
    GoldPerCity(i32),
    /// Percent more gold in every city.
    GoldPercent(i32),
    /// Percent off technologies another civ already knows.
    TechDiscount(i32),
    /// Percent off buying units and buildings with gold.
    BuyDiscount(i32),
    /// Percent more gold from trade routes.
    TradeGold(i32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Perk {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// Parts that need systems the game doesn't have yet.
    #[serde(default)]
    pub planned: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CivDef {
    pub id: String,
    pub name: String,
    pub adjective: String,
    pub leader: String,
    pub leader_title: String,
    pub colors: CivColors,
    pub emblem: String,
    pub description: String,
    #[serde(default)]
    pub start_bias: Vec<StartBias>,
    /// Where the civ arose on Earth, as (longitude, latitude) in degrees: its
    /// start on the Earth map with True Start Locations.
    #[serde(default)]
    pub home: Option<(f64, f64)>,
    pub drives: Drives,
    /// The troops the leader favours (read by the AI).
    #[serde(default)]
    pub army: ArmyMix,
    pub agenda: Agenda,
    pub ability: Perk,
    pub leader_ability: Perk,
    #[serde(default)]
    pub unique_units: Vec<String>,
    #[serde(default)]
    pub unique_buildings: Vec<String>,
    pub cities: Vec<String>,
    /// Names for the Generals it earns, in order.
    #[serde(default)]
    pub generals: Vec<String>,
    /// Names for its Admirals, in order.
    #[serde(default)]
    pub admirals: Vec<String>,
}

impl CivDef {
    /// Effects of both perks.
    pub fn effects(&self) -> impl Iterator<Item = &Effect> {
        self.ability.effects.iter().chain(&self.leader_ability.effects)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CivRules {
    pub civs: Vec<CivDef>,
}

impl Rules {
    pub fn civ(&self, id: CivId) -> &CivDef {
        &self.civs.civs[id as usize]
    }

    pub fn civ_count(&self) -> usize {
        self.civs.civs.len()
    }

    pub fn civ_id(&self, id: &str) -> Option<CivId> {
        self.civs.civs.iter().position(|c| c.id == id).map(|i| i as CivId)
    }

    /// Whether `tile`, with `improvement` on it, matches every condition.
    pub fn tile_matches(&self, map: &crate::map::Map, tile: TileId, improvement: Option<crate::rules::ImprovementId>, when: &[TileCond]) -> bool {
        let t = map.tile(tile);
        when.iter().all(|c| match c {
            TileCond::Terrain(id) => self.terrain(t.terrain).id == *id,
            TileCond::Feature(id) => t.feature.is_some_and(|f| self.feature(f).id == *id),
            TileCond::River => t.rivers != 0,
            TileCond::NearMountain => map.grid.neighbors(tile).any(|n| map.tile(n).elevation == Elevation::Mountains),
            TileCond::Improvement(id) => improvement.is_some_and(|i| self.improvement(i).id == *id),
        })
    }
}

impl Game {
    pub fn civ(&self, player: PlayerId) -> &CivDef {
        self.rules.civ(self.players[player as usize].civ)
    }

    /// The effects of a player's civ perks and of the technologies they know.
    pub fn civ_effects(&self, player: PlayerId) -> impl Iterator<Item = &Effect> {
        let known = &self.players[player as usize].techs;
        let techs = self.rules.techs.techs.iter().enumerate().filter(|&(i, _)| known[i]).flat_map(|(_, t)| &t.effects);
        self.civ(player).effects().chain(techs)
    }

    /// The unit type `player` builds in place of `base` (its unique unit, or
    /// `base` itself).
    pub fn civ_unit(&self, player: PlayerId, base: &str) -> Option<UnitTypeId> {
        let civ = self.players[player as usize].civ;
        self.rules.civ_unit(civ, base)
    }

    /// The building `player` builds in place of `base`.
    pub fn civ_building(&self, player: PlayerId, base: &str) -> Option<BuildingId> {
        let civ = self.players[player as usize].civ;
        self.rules.civ_building(civ, base)
    }

    /// Extra yields a player's perks give a worked tile.
    pub fn civ_tile_bonus(&self, player: PlayerId, tile: TileId) -> Yields {
        let mut y = Yields::default();
        for e in self.civ_effects(player) {
            if let Effect::TileYield { when, food, production, gold } = e {
                if self.rules.tile_matches(&self.map, tile, self.improvement_at(tile), when) {
                    y += Yields { food: *food, production: *production, gold: *gold };
                }
            }
        }
        y
    }

    /// Whether a land tile is next to water.
    pub fn is_coastal(&self, tile: TileId) -> bool {
        self.map.grid.neighbors(tile).any(|n| self.map.is_water(&self.rules, n))
    }

    /// The player owning a tile.
    pub fn territory_owner(&self, tile: TileId) -> Option<PlayerId> {
        self.territory[tile as usize]
    }

    /// Whether `player` has a perk effect matching `pred`.
    pub fn civ_has(&self, player: PlayerId, pred: impl Fn(&Effect) -> bool) -> bool {
        self.civ_effects(player).any(pred)
    }
}

impl Rules {
    /// The unit type civ `civ` builds in place of `base`.
    pub fn civ_unit(&self, civ: CivId, base: &str) -> Option<UnitTypeId> {
        let base_id = self.unit_type_id(base)?;
        Some(self.civ_units[civ as usize].iter().copied().find(|&u| self.unit_replaces(u) == Some(base_id)).unwrap_or(base_id))
    }

    pub fn civ_building(&self, civ: CivId, base: &str) -> Option<BuildingId> {
        let base_id = self.building_id(base)?;
        Some(
            self.civ_buildings[civ as usize]
                .iter()
                .copied()
                .find(|&b| self.building_replaces(b) == Some(base_id))
                .unwrap_or(base_id),
        )
    }

    /// Whether civ `civ` can build this unit type at all: not another civ's
    /// unique unit, and not a standard unit it has replaced.
    pub fn civ_can_build_unit(&self, civ: CivId, unit: UnitTypeId) -> bool {
        match self.unit_owner_civ[unit as usize] {
            Some(c) => c == civ,
            None => !self.civ_units[civ as usize].iter().any(|&u| self.unit_replaces(u) == Some(unit)),
        }
    }

    pub fn civ_can_build_building(&self, civ: CivId, b: BuildingId) -> bool {
        match self.building_owner_civ[b as usize] {
            Some(c) => c == civ,
            None => !self.civ_buildings[civ as usize].iter().any(|&u| self.building_replaces(u) == Some(b)),
        }
    }

    /// The civ a unique unit belongs to.
    pub fn unit_civ(&self, unit: UnitTypeId) -> Option<CivId> {
        self.unit_owner_civ[unit as usize]
    }

    pub fn building_civ(&self, b: BuildingId) -> Option<CivId> {
        self.building_owner_civ[b as usize]
    }

    /// Whether building `b` is `base` or a unique replacement of it.
    pub fn building_is(&self, b: BuildingId, base: &str) -> bool {
        self.building(b).id == base || self.building_replaces(b).is_some_and(|r| self.building(r).id == base)
    }
}

#[cfg(test)]
mod tests {
    use crate::city::Build;
    use crate::game::{Command, Game, GameOptions, GameSetup, Order};
    use crate::hex::TileId;
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::{Ability, Rules};

    /// A game between the given civs, with everything researched.
    fn game(civs: &[&str]) -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let civs = civs.iter().map(|c| rules.civ_id(c).unwrap()).collect();
        let mut g = Game::with_setup(rules, map, &GameSetup { civs, options: GameOptions::default(), starts: Vec::new() });
        for p in &mut g.players {
            p.techs.fill(true);
        }
        g
    }

    fn found(g: &mut Game, player: u8) -> crate::city::CityId {
        let settler = g.units.values().find(|u| u.owner == player && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
        g.apply(player, Command::FoundCity { unit: settler }).unwrap();
        *g.cities.iter().find(|(_, c)| c.owner == player).unwrap().0
    }

    #[test]
    fn players_get_their_civs_names_and_colours() {
        let g = game(&["netherlands", "rome"]);
        assert_eq!(g.players[0].name, "Netherlands");
        assert_eq!(g.players[0].color, [238, 122, 20]);
        assert_eq!(g.civ(1).leader, "Augustus");
    }

    #[test]
    fn cities_take_names_from_their_civ() {
        let mut g = game(&["netherlands", "rome"]);
        let c = found(&mut g, 0);
        assert_eq!(g.cities[&c].name, "Amsterdam");
        let c = found(&mut g, 1);
        assert_eq!(g.cities[&c].name, "Roma");
    }

    #[test]
    fn unique_builds_replace_standard_ones() {
        let mut g = game(&["netherlands", "rome"]);
        let city = found(&mut g, 0);
        let builds = g.available_builds(city);
        let has = |g: &Game, id: &str| {
            builds.iter().any(|&b| match b {
                Build::Unit(u) => g.rules.unit(u).id == id,
                Build::Building(b) => g.rules.building(b).id == id,
                Build::Project(_) | Build::Passage(..) => false,
            })
        };
        assert!(has(&g, "waag"));
        assert!(!has(&g, "market"), "replaced");
        assert!(!has(&g, "forum"), "Rome's");
        let rules = g.rules.clone();
        let unit = |id: &str| Build::Unit(rules.unit_type_id(id).unwrap());
        assert!(g.civ_builds(0, unit("schutter")));
        assert!(!g.civ_builds(0, unit("spearman")), "replaced");
        assert!(!g.civ_builds(0, unit("legion")), "Rome's");
    }

    #[test]
    fn rome_founds_cities_with_a_forum() {
        let mut g = game(&["rome", "netherlands"]);
        let city = found(&mut g, 0);
        let forum = g.rules.building_id("forum").unwrap();
        assert_eq!(g.cities[&city].buildings, vec![forum]);
    }

    #[test]
    fn tile_perks_add_yields() {
        let g = game(&["netherlands", "rome"]);
        let coast = (0..g.map.tiles.len() as TileId)
            .find(|&t| g.rules.terrain(g.map.tile(t).terrain).id == "coast" && g.map.tile(t).resource.is_none())
            .unwrap();
        let base = g.map.yields(&g.rules, coast);
        assert_eq!(g.tile_yields(0, coast).production, base.production + 1, "Polders");
        assert_eq!(g.tile_yields(1, coast), base);
    }

    #[test]
    fn schutter_is_stronger_at_home() {
        let mut g = game(&["netherlands", "rome"]);
        let city = found(&mut g, 0);
        let home = g.cities[&city].tile;
        let land = |g: &Game, t: TileId| g.map.land_move_cost(&g.rules, t).is_some() && g.units_at(t).next().is_none();
        // A Schutter inside Dutch borders, a Roman warrior next to it.
        let at = g.map.grid.neighbors(home).find(|&t| land(&g, t)).unwrap();
        let from = g.map.grid.neighbors(at).find(|&t| t != home && land(&g, t)).unwrap();
        let schutter = g.spawn_unit(0, g.rules.unit_type_id("schutter").unwrap(), at);
        let roman = g.spawn_unit(1, g.rules.unit_type_id("warrior").unwrap(), from);
        g.update_visibility(1);
        g.apply(1, Command::DeclareWar { on: 0 }).unwrap();
        let p = g.combat_preview(roman, at).unwrap();
        assert!(p.defense_mods.iter().any(|m| m.label == "Schutter" && m.percent == 30), "{:?}", p.defense_mods);
        // Outside Dutch borders the bonus is gone.
        g.set_territory(at, None);
        let p = g.combat_preview(roman, at).unwrap();
        assert!(!p.defense_mods.iter().any(|m| m.label == "Schutter"));
        let _ = schutter;
    }

    #[test]
    fn unit_limit_stops_military_builds() {
        let mut g = game(&["netherlands", "rome"]);
        g.options.unit_limit = Some(2);
        let city = found(&mut g, 0);
        // Warrior and scout: already at the limit.
        assert!(g.at_unit_limit(0));
        let builds = g.available_builds(city);
        assert!(builds.iter().all(|&b| !matches!(b, Build::Unit(u) if g.rules.unit(u).class == crate::rules::UnitClass::Military)));
        assert!(builds.iter().any(|&b| matches!(b, Build::Unit(u) if g.rules.unit(u).id == "settler")));
        for _ in 0..30 {
            g.apply(0, Command::EndTurn).unwrap();
        }
        assert!(g.military_units(0) <= 2);
    }

    #[test]
    fn mongols_move_further_at_home() {
        let mut g = game(&["mongols", "rome"]);
        let city = found(&mut g, 0);
        let warrior = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).id == "warrior").unwrap().id;
        assert_eq!(g.units[&warrior].tile, g.cities[&city].tile);
        g.apply(0, Command::SetOrder { unit: warrior, order: Order::Fortify }).unwrap();
        g.apply(0, Command::EndTurn).unwrap();
        assert_eq!(g.units[&warrior].moves, crate::path::points(3));
    }

    #[test]
    fn korea_researches_known_techs_cheaper() {
        let mut g = game(&["korea", "rome"]);
        let pottery = g.rules.tech_id("pottery").unwrap();
        g.players[0].techs.fill(false);
        g.players[1].techs.fill(false);
        let full = g.tech_cost(0, pottery);
        g.players[1].techs[pottery as usize] = true;
        assert_eq!(g.tech_cost(0, pottery), full - full / 4);
        assert_eq!(g.tech_cost(1, pottery), full);
    }

    #[test]
    fn hwacha_cannot_fire_after_moving() {
        let mut g = game(&["korea", "rome"]);
        let scout = g.units.values().find(|u| u.owner == 0 && g.unit_def(u).id == "scout").unwrap().id;
        g.units.get_mut(&scout).unwrap().kind = g.rules.unit_type_id("hwacha").unwrap();
        g.units.get_mut(&scout).unwrap().moved = true;
        let warrior = g.units.values().find(|u| u.owner == 1 && g.unit_def(u).id == "warrior").unwrap().tile;
        let err = g.attack_target(scout, warrior).unwrap_err();
        assert!(matches!(err, crate::game::CommandError::CantAttack(_) | crate::game::CommandError::NoMovesLeft));
    }

    #[test]
    fn random_setups_have_distinct_civs() {
        let rules = Rules::load_default().unwrap();
        let dutch = rules.civ_id("netherlands");
        for seed in 0..20 {
            let s = GameSetup::random(&rules, 6, &[dutch], seed);
            assert_eq!(s.civs[0], dutch.unwrap());
            let mut sorted = s.civs.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), 6);
        }
    }

    #[test]
    fn coastal_civs_start_by_the_sea() {
        // Over a few maps, the Dutch and Norse get coastal starts at least as
        // often as the Mongols.
        let rules = Rules::load_default().unwrap();
        let (mut sea, mut steppe) = (0, 0);
        for seed in 1..6 {
            let map = generate(&rules, &MapSettings { seed, ..Default::default() });
            let civs = ["netherlands", "mongols", "norse", "rome"].iter().map(|c| rules.civ_id(c).unwrap()).collect();
            let g = Game::with_setup(rules.clone(), map, &GameSetup { civs, options: GameOptions::default(), starts: Vec::new() });
            let start = |p: u8| g.units.values().find(|u| u.owner == p && g.unit_def(u).has(Ability::FoundCity)).unwrap().tile;
            sea += g.is_coastal(start(0)) as i32 + g.is_coastal(start(2)) as i32;
            steppe += 2 * g.is_coastal(start(1)) as i32;
        }
        assert!(sea >= steppe, "sea civs {sea}, steppe {steppe}");
    }

    #[test]
    fn civs_start_in_their_own_climate() {
        // Over a few maps, Egypt starts by the desert and the Norse in the
        // taiga, and neither start goes hungry.
        let rules = Rules::load_default().unwrap();
        let desert = rules.terrain_id("desert").unwrap();
        let forest = rules.feature_id("forest").unwrap();
        let taiga = |t: &crate::map::Tile| t.feature == Some(forest) && (crate::climate::TUNDRA..crate::climate::BOREAL).contains(&t.temperature);
        let (mut egypt_desert, mut rome_desert, mut norse_taiga, mut rome_taiga) = (0, 0, 0, 0);
        let maps = 6;
        for seed in 1..=maps {
            let map = generate(&rules, &MapSettings { seed, ..Default::default() });
            let civs = ["egypt", "norse", "rome", "mongols"].iter().map(|c| rules.civ_id(c).unwrap()).collect();
            let g = Game::with_setup(rules.clone(), map, &GameSetup { civs, options: GameOptions::default(), starts: Vec::new() });
            let area = |p: u8| {
                let start = g.units.values().find(|u| u.owner == p && g.unit_def(u).has(Ability::FoundCity)).unwrap().tile;
                g.map.grid.within(start, 2)
            };
            let count = |p: u8, f: &dyn Fn(&crate::map::Tile) -> bool| area(p).iter().filter(|&&t| f(g.map.tile(t))).count();
            egypt_desert += count(0, &|t| t.terrain == desert);
            norse_taiga += count(1, &taiga);
            rome_desert += count(2, &|t| t.terrain == desert);
            rome_taiga += count(2, &taiga);
            for p in 0..4 {
                let food: i32 = area(p).iter().map(|&t| g.tile_yields(p, t).food).sum();
                assert!(food >= 28, "seed {seed}: {} start has {food} food", g.players[p as usize].name);
            }
        }
        assert!(egypt_desert >= 3 * maps as usize && egypt_desert > 2 * rome_desert, "desert: Egypt {egypt_desert}, Rome {rome_desert}");
        assert!(norse_taiga >= 6 * maps as usize && norse_taiga > rome_taiga, "taiga: Norse {norse_taiga}, Rome {rome_taiga}");
    }
}
