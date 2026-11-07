//! Data-driven game rules loaded from `data/*.ron`.

use serde::{Deserialize, Serialize};
use std::{fmt, fs, path::{Path, PathBuf}};

pub type TerrainId = u8;
pub type FeatureId = u8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Yields {
    pub food: i32,
    pub production: i32,
    pub gold: i32,
}

impl std::ops::Add for Yields {
    type Output = Yields;
    fn add(self, o: Yields) -> Yields {
        Yields {
            food: self.food + o.food,
            production: self.production + o.production,
            gold: self.gold + o.gold,
        }
    }
}

impl std::ops::Sub for Yields {
    type Output = Yields;
    fn sub(self, o: Yields) -> Yields {
        Yields { food: self.food - o.food, production: self.production - o.production, gold: self.gold - o.gold }
    }
}

impl std::ops::AddAssign for Yields {
    fn add_assign(&mut self, o: Yields) {
        *self = *self + o;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerrainDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
    #[serde(default)]
    pub gold: i32,
    pub move_cost: i32,
    pub defense: i32,
    pub water: bool,
    pub art: String,
    /// Open ocean: crossing it needs a [`TechGrant::DeepWater`] tech.
    #[serde(default)]
    pub deep: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Modifier {
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
    #[serde(default)]
    pub gold: i32,
    pub move_cost: i32,
    pub defense: i32,
    #[serde(default)]
    pub impassable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeatureDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
    #[serde(default)]
    pub gold: i32,
    pub move_cost: i32,
    pub defense: i32,
    pub art: String,
    pub allowed_on: Vec<String>,
    #[serde(default)]
    pub impassable: bool,
    /// Extra turns a Worker needs to clear it when building an improvement
    /// (0: it can't be cleared, and blocks improvements).
    #[serde(default)]
    pub clear_turns: i32,
    /// Production the nearest city of the owner gets when it's cleared.
    #[serde(default)]
    pub clear_production: i32,
}

macro_rules! impl_yields {
    ($($t:ty),*) => {$(
        impl $t {
            pub fn yields(&self) -> Yields {
                Yields { food: self.food, production: self.production, gold: self.gold }
            }
        }
    )*};
}

impl_yields!(TerrainDef, Modifier, FeatureDef, ImprovementDef, ResourceDef);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerrainRules {
    pub terrains: Vec<TerrainDef>,
    pub hills: Modifier,
    pub mountains: Modifier,
    pub features: Vec<FeatureDef>,
    pub river: Modifier,
    pub sea_ice: SeaIceRules,
}

/// When water freezes (see `Map::sea_ice`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeaIceRules {
    /// Below this temperature (°C) water carries thin ice: ships and embarked
    /// units cross it at `thin_move_cost` per tile.
    pub thin_below: f32,
    /// Below this it is pack ice, which nothing can cross.
    pub pack_below: f32,
    pub thin_move_cost: i32,
}

pub type UnitTypeId = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitClass {
    /// Can't fight; shares a tile with a military unit.
    Civilian,
    Military,
}

/// Where a unit moves: on land (embarking onto water with the right techs),
/// or on water and into its owner's coastal cities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Domain {
    #[default]
    Land,
    Sea,
}

/// What a technology lets all of a player's units do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TechGrant {
    /// Land units can embark onto shallow water.
    Embark,
    /// Ships and embarked units can enter deep water.
    DeepWater,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ability {
    FoundCity,
    BuildImprovements,
    /// Every land tile costs 1 movement point.
    IgnoreTerrainCost,
    /// Can't attack in a turn it has moved.
    NoAttackAfterMove,
    /// Can attack twice per turn.
    AttackTwice,
    /// Destroying an enemy unit in melee yields a Worker.
    CaptureAsWorker,
    /// Heals 10 extra hit points each turn.
    FastHeal,
    /// Builds roads (Workers build roads as part of [`Ability::BuildImprovements`]).
    BuildRoads,
    /// A horse-borne unit (spearmen fight better against it).
    Mounted,
    /// Runs trade routes between cities (see `trade.rs`).
    Trade,
    /// A General (or an Admiral): doesn't fight, but leads the units around
    /// it (see `experience.rs`).
    General,
}

/// A conditional combat bonus of a unit type.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct UnitBonus {
    pub percent: i32,
    pub when: crate::civ::CombatCond,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnitDef {
    pub id: String,
    pub name: String,
    pub class: UnitClass,
    #[serde(default)]
    pub domain: Domain,
    pub moves: i32,
    pub sight: i32,
    /// Melee strength (also used when defending).
    pub strength: i32,
    /// Ranged strength; units with it attack from a distance without taking damage.
    #[serde(default)]
    pub ranged: i32,
    /// How far a ranged unit can shoot, in tiles.
    #[serde(default)]
    pub range: i32,
    pub cost: i32,
    pub art: String,
    #[serde(default)]
    pub abilities: Vec<Ability>,
    /// The technology that unlocks this unit (`None`: available from the start).
    #[serde(default)]
    pub tech: Option<String>,
    #[serde(default)]
    pub bonuses: Vec<UnitBonus>,
    /// A strategic resource the owner needs to train it.
    #[serde(default)]
    pub resource: Option<String>,
    /// The standard unit this one replaces for its civ (unique units only).
    #[serde(default)]
    pub replaces: Option<String>,
    /// The standard unit it upgrades to (unique units upgrade like the unit
    /// they replace unless they say otherwise). Once its owner can build that,
    /// this one is obsolete.
    #[serde(default)]
    pub upgrades_to: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Abilities that need systems the game doesn't have yet.
    #[serde(default)]
    pub planned: Option<String>,
}

impl UnitDef {
    pub fn has(&self, ability: Ability) -> bool {
        self.abilities.contains(&ability)
    }
}

/// A kind of military unit a building helps (see `BuildingDef::units`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitFilter {
    /// Every military unit.
    #[default]
    Military,
    /// Military land units.
    Land,
    /// Military ships.
    Sea,
    /// Horse-borne units.
    Mounted,
}

impl UnitFilter {
    pub fn matches(self, def: &UnitDef) -> bool {
        def.class == UnitClass::Military
            && match self {
                UnitFilter::Military => true,
                UnitFilter::Land => def.domain == Domain::Land,
                UnitFilter::Sea => def.domain == Domain::Sea,
                UnitFilter::Mounted => def.has(Ability::Mounted),
            }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnitRules {
    pub units: Vec<UnitDef>,
}

pub type BuildingId = u8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildingDef {
    pub id: String,
    pub name: String,
    pub cost: i32,
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
    #[serde(default)]
    pub gold: i32,
    #[serde(default)]
    pub culture: i32,
    #[serde(default)]
    pub science: i32,
    #[serde(default)]
    pub defense: i32,
    #[serde(default)]
    pub description: String,
    /// The technology that unlocks this building (`None`: available from the start).
    #[serde(default)]
    pub tech: Option<String>,
    /// Percent of the growth threshold kept as food when the city grows.
    #[serde(default)]
    pub keeps_food: i32,
    /// The city fires at enemies this many tiles away each turn (0: it doesn't).
    #[serde(default)]
    pub city_range: i32,
    /// Extra yields on matching worked tiles.
    #[serde(default)]
    pub tile_bonus: Option<TileBonus>,
    /// Units in the city heal fully each turn.
    #[serde(default)]
    pub heals_units: bool,
    /// Percent extra production towards the units in `units`.
    #[serde(default)]
    pub military_production: i32,
    /// Experience for each unit in `units` trained or bought here (not
    /// Generals and Admirals, who would gain levels).
    #[serde(default)]
    pub unit_xp: u32,
    /// Which units `military_production` and `unit_xp` apply to.
    #[serde(default)]
    pub units: UnitFilter,
    /// Percent extra gold for trade routes from or to the city.
    #[serde(default)]
    pub trade_gold: i32,
    /// A strategic resource the owner needs to build it.
    #[serde(default)]
    pub resource: Option<String>,
    /// The standard building this one replaces for its civ (unique buildings only).
    #[serde(default)]
    pub replaces: Option<String>,
    /// Only in cities next to water.
    #[serde(default)]
    pub coastal: bool,
    /// A building the city needs first (or the civ's replacement for it).
    #[serde(default)]
    pub requires: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TileBonus {
    pub when: Vec<crate::civ::TileCond>,
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
    #[serde(default)]
    pub gold: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildingRules {
    pub buildings: Vec<BuildingDef>,
}

pub type ImprovementId = u8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImprovementDef {
    pub id: String,
    pub name: String,
    /// Turns a Worker needs to build it (plus any clearing).
    pub turns: i32,
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
    #[serde(default)]
    pub gold: i32,
    pub art: String,
    #[serde(default)]
    pub tech: Option<String>,
    /// Base terrains it can go on (empty: any land).
    #[serde(default)]
    pub terrains: Vec<String>,
    /// Elevations it can go on (empty: flat land and hills).
    #[serde(default)]
    pub elevations: Vec<crate::map::Elevation>,
    /// Only on tiles with a resource it improves.
    #[serde(default)]
    pub resource_only: bool,
    /// On water instead of land (built by an embarked Worker).
    #[serde(default)]
    pub water: bool,
    /// Only on tiles with one of these features, which it keeps (empty: it
    /// clears whatever feature is there).
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RoadDef {
    pub turns: i32,
}

/// Railways: laid over roads by Workers once their owner knows `tech`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RailDef {
    pub turns: i32,
    pub tech: String,
}

/// A tunnel, canal or bridge: what it costs a city to build and the tech that allows it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PassageDef {
    pub cost: i32,
    pub tech: String,
    /// The terrain the tile becomes (canals: shallow water).
    #[serde(default)]
    pub becomes: Option<String>,
    /// Border posts (forts and watchtowers): how far from the city that
    /// builds them they may go, their defence bonus (percent), the strength they fire with (percent of a
    /// city's) and how far they see.
    #[serde(default)]
    pub reach: i32,
    #[serde(default)]
    pub defense: i32,
    #[serde(default)]
    pub strike: i32,
    #[serde(default)]
    pub sight: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImprovementRules {
    pub improvements: Vec<ImprovementDef>,
    pub road: RoadDef,
    pub railroad: RailDef,
    pub tunnel: PassageDef,
    pub canal: PassageDef,
    pub bridge: PassageDef,
    pub fort: PassageDef,
    pub watchtower: PassageDef,
}

pub type ResourceId = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Bonus,
    Luxury,
    Strategic,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceDef {
    pub id: String,
    pub name: String,
    pub kind: ResourceKind,
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
    #[serde(default)]
    pub gold: i32,
    /// The improvement that works it (`None`: connected inside the borders
    /// with none).
    #[serde(default)]
    pub improvement: Option<String>,
    /// The technology that reveals it (`None`: everyone sees it).
    #[serde(default)]
    pub tech: Option<String>,
    /// Where it appears (see resources.ron).
    #[serde(default)]
    pub terrains: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub elevations: Vec<crate::map::Elevation>,
    #[serde(default)]
    pub temperature: Option<(f32, f32)>,
    pub abundance: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceRules {
    /// Gold in every city for each different luxury a player has.
    pub luxury_gold: i32,
    pub resources: Vec<ResourceDef>,
}

pub type TechId = u8;
pub type EraId = u8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EraDef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechDef {
    pub id: String,
    pub name: String,
    /// The era it belongs to (an id from `eras`), never earlier than its prerequisites'.
    pub era: String,
    /// Science needed to research it.
    pub cost: i32,
    /// Prerequisites (ids of techs listed earlier in the file).
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub grants: Vec<TechGrant>,
    /// Lasting bonuses for whoever knows it, like a civ's perks.
    #[serde(default)]
    pub effects: Vec<crate::civ::Effect>,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechRules {
    /// The ages of the tech tree, in order.
    pub eras: Vec<EraDef>,
    pub techs: Vec<TechDef>,
}

#[derive(Clone, Debug)]
pub struct Rules {
    pub terrain: TerrainRules,
    pub units: UnitRules,
    pub buildings: BuildingRules,
    pub techs: TechRules,
    pub civs: crate::civ::CivRules,
    pub improvements: ImprovementRules,
    pub resources: ResourceRules,
    /// Ranks and General traits (`experience.ron`).
    pub experience: crate::experience::ExperienceRules,
    /// The Earth map type's world (`earth.ron`), shared between clones.
    pub earth: std::sync::Arc<crate::earth::Earth>,
    /// Resolved references (filled in by `load`).
    tech_requires: Vec<Vec<TechId>>,
    tech_needs: Vec<Vec<TechId>>,
    tech_era: Vec<EraId>,
    unit_upgrade: Vec<Option<UnitTypeId>>,
    building_requires: Vec<Option<BuildingId>>,
    unit_tech: Vec<Option<TechId>>,
    building_tech: Vec<Option<TechId>>,
    unit_resource: Vec<Option<ResourceId>>,
    building_resource: Vec<Option<ResourceId>>,
    improvement_tech: Vec<Option<TechId>>,
    tunnel_tech: TechId,
    canal_tech: TechId,
    canal_water: TerrainId,
    bridge_tech: TechId,
    rail_tech: TechId,
    fort_tech: TechId,
    watchtower_tech: TechId,
    resource_tech: Vec<Option<TechId>>,
    resource_improvement: Vec<Option<ImprovementId>>,
    unit_replaces: Vec<Option<UnitTypeId>>,
    building_replaces: Vec<Option<BuildingId>>,
    /// Each civ's unique units and buildings.
    pub(crate) civ_units: Vec<Vec<UnitTypeId>>,
    pub(crate) civ_buildings: Vec<Vec<BuildingId>>,
    /// The civ each unique unit or building belongs to.
    pub(crate) unit_owner_civ: Vec<Option<crate::civ::CivId>>,
    pub(crate) building_owner_civ: Vec<Option<crate::civ::CivId>>,
}

#[derive(Debug)]
pub enum RulesError {
    Io(String, std::io::Error),
    Parse(String, Box<ron::error::SpannedError>),
    Invalid(String),
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RulesError::Io(file, e) => write!(f, "{file}: {e}"),
            RulesError::Parse(file, e) => write!(f, "{file}: {e}"),
            RulesError::Invalid(msg) => write!(f, "invalid rules: {msg}"),
        }
    }
}

impl std::error::Error for RulesError {}

/// Where the rules' `.ron` files are: `FOURX_DATA` if set, else a `data`
/// folder beside the executable (as in a release), else the source tree's
/// `data` (for `cargo run` and tests).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FOURX_DATA") {
        return dir.into();
    }
    let beside_exe = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join("data")));
    match beside_exe {
        Some(dir) if dir.join("terrain.ron").is_file() => dir,
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"),
    }
}

fn load<T: for<'de> Deserialize<'de>>(dir: &Path, file: &str) -> Result<T, RulesError> {
    let path = dir.join(file);
    let text = fs::read_to_string(&path).map_err(|e| RulesError::Io(path.display().to_string(), e))?;
    ron::from_str(&text).map_err(|e| RulesError::Parse(path.display().to_string(), Box::new(e)))
}

impl Rules {
    pub fn load(data_dir: impl AsRef<Path>) -> Result<Rules, RulesError> {
        let dir = data_dir.as_ref();
        let mut rules = Rules {
            terrain: load(dir, "terrain.ron")?,
            units: load(dir, "units.ron")?,
            buildings: load(dir, "buildings.ron")?,
            techs: load(dir, "techs.ron")?,
            civs: load(dir, "civs.ron")?,
            improvements: load(dir, "improvements.ron")?,
            resources: load(dir, "resources.ron")?,
            experience: load(dir, "experience.ron")?,
            earth: std::sync::Arc::new(load(dir, "earth.ron")?),
            tech_requires: Vec::new(),
            tech_needs: Vec::new(),
            tech_era: Vec::new(),
            unit_upgrade: Vec::new(),
            building_requires: Vec::new(),
            unit_tech: Vec::new(),
            building_tech: Vec::new(),
            unit_resource: Vec::new(),
            building_resource: Vec::new(),
            improvement_tech: Vec::new(),
            tunnel_tech: 0,
            canal_tech: 0,
            canal_water: 0,
            bridge_tech: 0,
            rail_tech: 0,
            fort_tech: 0,
            watchtower_tech: 0,
            resource_tech: Vec::new(),
            resource_improvement: Vec::new(),
            unit_replaces: Vec::new(),
            building_replaces: Vec::new(),
            civ_units: Vec::new(),
            civ_buildings: Vec::new(),
            unit_owner_civ: Vec::new(),
            building_owner_civ: Vec::new(),
        };
        rules.validate()?;
        rules.resolve()?;
        Ok(rules)
    }

    /// The rules shipped in `data/`, located relative to the workspace root.
    /// Loads the rules from [`data_dir`].
    pub fn load_default() -> Result<Rules, RulesError> {
        Self::load(data_dir())
    }

    fn validate(&self) -> Result<(), RulesError> {
        let t = &self.terrain;
        if t.terrains.len() > u8::MAX as usize
            || t.features.len() > u8::MAX as usize
            || self.units.units.len() > u8::MAX as usize
            || self.buildings.buildings.len() > u8::MAX as usize
            || self.techs.techs.len() > u8::MAX as usize
            || self.civs.civs.len() > u8::MAX as usize
            || self.improvements.improvements.len() > u8::MAX as usize
            || self.resources.resources.len() > u8::MAX as usize
        {
            return Err(RulesError::Invalid("too many terrain, feature, unit, building, tech, civ, improvement or resource types".into()));
        }
        let terrains = |owner: &str, ids: &[String]| match ids.iter().find(|t| self.terrain_id(t).is_none()) {
            Some(t) => Err(RulesError::Invalid(format!("'{owner}' refers to unknown terrain '{t}'"))),
            None => Ok(()),
        };
        for i in &self.improvements.improvements {
            terrains(&i.id, &i.terrains)?;
            if let Some(f) = i.features.iter().find(|f| self.feature_id(f).is_none()) {
                return Err(RulesError::Invalid(format!("improvement '{}' refers to unknown feature '{f}'", i.id)));
            }
            if i.turns <= 0 {
                return Err(RulesError::Invalid(format!("improvement '{}' must take at least a turn", i.id)));
            }
        }
        if [&self.improvements.tunnel, &self.improvements.canal, &self.improvements.bridge].iter().any(|p| p.cost <= 0) {
            return Err(RulesError::Invalid("tunnels, canals and bridges must cost some production".into()));
        }
        for r in &self.resources.resources {
            terrains(&r.id, &r.terrains)?;
            if let Some(f) = r.features.iter().find(|f| self.feature_id(f).is_none()) {
                return Err(RulesError::Invalid(format!("resource '{}' refers to unknown feature '{f}'", r.id)));
            }
            if r.improvement.as_ref().is_some_and(|i| self.improvement_id(i).is_none()) {
                return Err(RulesError::Invalid(format!("resource '{}' has an unknown improvement", r.id)));
            }
        }
        if self.civs.civs.is_empty() {
            return Err(RulesError::Invalid("no civilizations".into()));
        }
        for f in &t.features {
            for on in &f.allowed_on {
                if self.terrain_id(on).is_none() {
                    return Err(RulesError::Invalid(format!(
                        "feature '{}' allowed_on unknown terrain '{on}'",
                        f.id
                    )));
                }
            }
        }
        for (i, c) in self.civs.civs.iter().enumerate() {
            if self.civs.civs[..i].iter().any(|o| o.id == c.id) {
                return Err(RulesError::Invalid(format!("civ '{}' is listed twice", c.id)));
            }
            if c.cities.is_empty() {
                return Err(RulesError::Invalid(format!("civ '{}' has no city names", c.id)));
            }
            for e in c.effects() {
                self.validate_effect(&c.id, e)?;
            }
        }
        for t in &self.techs.techs {
            for e in &t.effects {
                self.validate_effect(&t.id, e)?;
            }
        }
        for u in &self.units.units {
            if u.domain == Domain::Sea && u.class != UnitClass::Military {
                return Err(RulesError::Invalid(format!("sea unit '{}' must be military", u.id)));
            }
        }
        for b in &self.buildings.buildings {
            if let Some(bonus) = &b.tile_bonus {
                self.validate_tile_conds(&b.id, &bonus.when)?;
            }
        }
        self.experience.validate()?;
        Ok(())
    }

    fn validate_tile_conds(&self, owner: &str, when: &[crate::civ::TileCond]) -> Result<(), RulesError> {
        use crate::civ::TileCond;
        for c in when {
            let ok = match c {
                TileCond::Terrain(t) => self.terrain_id(t).is_some(),
                TileCond::Feature(f) => self.feature_id(f).is_some(),
                TileCond::Improvement(i) => self.improvement_id(i).is_some(),
                TileCond::River | TileCond::NearMountain => true,
            };
            if !ok {
                return Err(RulesError::Invalid(format!("'{owner}' refers to unknown terrain or feature in {c:?}")));
            }
        }
        Ok(())
    }

    fn validate_effect(&self, owner: &str, e: &crate::civ::Effect) -> Result<(), RulesError> {
        use crate::civ::Effect;
        match e {
            Effect::TileYield { when, .. } => self.validate_tile_conds(owner, when),
            Effect::BuildingBonus { building, .. } | Effect::FreeBuilding(building) => match self.building_id(building) {
                Some(_) => Ok(()),
                None => Err(RulesError::Invalid(format!("'{owner}' refers to unknown building '{building}'"))),
            },
            _ => Ok(()),
        }
    }

    /// Looks up tech references. Prerequisites must come earlier in the file,
    /// which also rules out cycles.
    fn resolve(&mut self) -> Result<(), RulesError> {
        let mut requires = Vec::new();
        for (i, t) in self.techs.techs.iter().enumerate() {
            let mut ids = Vec::new();
            for r in &t.requires {
                match self.tech_id(r) {
                    Some(id) if (id as usize) < i => ids.push(id),
                    Some(_) => {
                        return Err(RulesError::Invalid(format!("tech '{}' requires '{r}', which must be listed before it", t.id)))
                    }
                    None => return Err(RulesError::Invalid(format!("tech '{}' requires unknown tech '{r}'", t.id))),
                }
            }
            requires.push(ids);
        }
        let mut eras = Vec::new();
        for (i, t) in self.techs.techs.iter().enumerate() {
            let Some(era) = self.techs.eras.iter().position(|e| e.id == t.era) else {
                return Err(RulesError::Invalid(format!("tech '{}' is in unknown era '{}'", t.id, t.era)));
            };
            if requires[i].iter().any(|&r| eras[r as usize] > era as EraId) {
                return Err(RulesError::Invalid(format!("tech '{}' comes in an earlier era than one of its prerequisites", t.id)));
            }
            // Eras come in order, so file order stays a valid research order.
            if eras.last().is_some_and(|&last| last > era as EraId) {
                return Err(RulesError::Invalid(format!("tech '{}' is listed after a tech of a later era", t.id)));
            }
            eras.push(era as EraId);
        }
        // A tech needs its prerequisites and every tech of the era before its own.
        let needs: Vec<Vec<TechId>> = (0..eras.len())
            .map(|i| {
                let mut n: Vec<TechId> = (0..i).filter(|&j| eras[j] + 1 == eras[i]).map(|j| j as TechId).collect();
                n.extend(&requires[i]);
                n.sort();
                n.dedup();
                n
            })
            .collect();
        let lookup = |owner: &str, tech: &Option<String>| -> Result<Option<TechId>, RulesError> {
            match tech {
                None => Ok(None),
                Some(t) => self
                    .tech_id(t)
                    .map(Some)
                    .ok_or_else(|| RulesError::Invalid(format!("'{owner}' needs unknown tech '{t}'"))),
            }
        };
        let unit_tech = self.units.units.iter().map(|u| lookup(&u.id, &u.tech)).collect::<Result<_, _>>()?;
        let building_tech = self.buildings.buildings.iter().map(|b| lookup(&b.id, &b.tech)).collect::<Result<_, _>>()?;
        let improvement_tech = self.improvements.improvements.iter().map(|i| lookup(&i.id, &i.tech)).collect::<Result<_, _>>()?;
        let resource_tech = self.resources.resources.iter().map(|r| lookup(&r.id, &r.tech)).collect::<Result<_, _>>()?;
        let tunnel_tech = lookup("tunnel", &Some(self.improvements.tunnel.tech.clone()))?.expect("named");
        let canal_tech = lookup("canal", &Some(self.improvements.canal.tech.clone()))?.expect("named");
        let bridge_tech = lookup("bridge", &Some(self.improvements.bridge.tech.clone()))?.expect("named");
        let rail_tech = lookup("railroad", &Some(self.improvements.railroad.tech.clone()))?.expect("named");
        if self.improvements.railroad.turns < 1 {
            return Err(RulesError::Invalid("railroad: `turns` must be at least 1".into()));
        }
        let fort_tech = lookup("fort", &Some(self.improvements.fort.tech.clone()))?.expect("named");
        let watchtower_tech = lookup("watchtower", &Some(self.improvements.watchtower.tech.clone()))?.expect("named");
        for (id, p) in [("fort", &self.improvements.fort), ("watchtower", &self.improvements.watchtower)] {
            if p.reach < 1 || p.sight < 0 {
                return Err(RulesError::Invalid(format!("{id}: `reach` must be at least 1, `sight` not negative")));
            }
        }
        let canal_water = match &self.improvements.canal.becomes {
            Some(t) => match self.terrain_id(t) {
                Some(id) if self.terrain(id).water && !self.terrain(id).deep => id,
                _ => return Err(RulesError::Invalid(format!("canals become '{t}', which isn't shallow water"))),
            },
            None => return Err(RulesError::Invalid("canals need the terrain they become (`becomes`)".into())),
        };
        let strategic = |owner: &str, r: &Option<String>| match r {
            None => Ok(None),
            Some(r) => match self.resource_id(r) {
                Some(id) if self.resource(id).kind == ResourceKind::Strategic => Ok(Some(id)),
                _ => Err(RulesError::Invalid(format!("'{owner}' needs '{r}', which isn't a strategic resource"))),
            },
        };
        let unit_resource = self.units.units.iter().map(|u| strategic(&u.id, &u.resource)).collect::<Result<Vec<_>, _>>()?;
        let building_resource = self.buildings.buildings.iter().map(|b| strategic(&b.id, &b.resource)).collect::<Result<Vec<_>, _>>()?;
        self.resource_improvement =
            self.resources.resources.iter().map(|r| r.improvement.as_ref().and_then(|i| self.improvement_id(i))).collect();
        let mut building_requires = Vec::new();
        for b in &self.buildings.buildings {
            building_requires.push(match &b.requires {
                None => None,
                Some(r) => match self.building_id(r) {
                    Some(id) if self.buildings.buildings[id as usize].replaces.is_none() => Some(id),
                    _ => return Err(RulesError::Invalid(format!("building '{}' requires '{r}', which isn't a standard building", b.id))),
                },
            });
        }
        self.building_requires = building_requires;
        self.tech_era = eras;
        self.tech_needs = needs;
        self.tech_requires = requires;
        self.unit_tech = unit_tech;
        self.building_tech = building_tech;
        self.improvement_tech = improvement_tech;
        self.tunnel_tech = tunnel_tech;
        self.canal_tech = canal_tech;
        self.canal_water = canal_water;
        self.bridge_tech = bridge_tech;
        self.rail_tech = rail_tech;
        self.fort_tech = fort_tech;
        self.watchtower_tech = watchtower_tech;
        self.resource_tech = resource_tech;
        self.unit_resource = unit_resource;
        self.building_resource = building_resource;
        self.resolve_uniques()
    }

    /// Links unique units and buildings to their civ and what they replace.
    /// Each must belong to exactly one civ and replace a standard one, and a
    /// civ replaces each standard unit or building at most once.
    fn resolve_uniques(&mut self) -> Result<(), RulesError> {
        let invalid = |m: String| Err(RulesError::Invalid(m));
        let mut unit_replaces = Vec::new();
        for u in &self.units.units {
            unit_replaces.push(match &u.replaces {
                None => None,
                Some(r) => match self.unit_type_id(r) {
                    Some(id) if self.units.units[id as usize].replaces.is_none() => Some(id),
                    _ => return invalid(format!("unit '{}' replaces '{r}', which isn't a standard unit", u.id)),
                },
            });
        }
        let mut building_replaces = Vec::new();
        for b in &self.buildings.buildings {
            building_replaces.push(match &b.replaces {
                None => None,
                Some(r) => match self.building_id(r) {
                    Some(id) if self.buildings.buildings[id as usize].replaces.is_none() => Some(id),
                    _ => return invalid(format!("building '{}' replaces '{r}', which isn't a standard building", b.id)),
                },
            });
        }
        let mut unit_owner = vec![None; self.units.units.len()];
        let mut building_owner = vec![None; self.buildings.buildings.len()];
        let (mut civ_units, mut civ_buildings) = (Vec::new(), Vec::new());
        for (ci, c) in self.civs.civs.iter().enumerate() {
            let mut units: Vec<UnitTypeId> = Vec::new();
            for id in &c.unique_units {
                let Some(u) = self.unit_type_id(id) else { return invalid(format!("civ '{}' has unknown unique unit '{id}'", c.id)) };
                if unit_replaces[u as usize].is_none() {
                    return invalid(format!("unique unit '{id}' of civ '{}' doesn't replace anything", c.id));
                }
                if unit_owner[u as usize].is_some() {
                    return invalid(format!("unique unit '{id}' belongs to two civs"));
                }
                if units.iter().any(|&o| unit_replaces[o as usize] == unit_replaces[u as usize]) {
                    return invalid(format!("civ '{}' replaces the same unit twice", c.id));
                }
                unit_owner[u as usize] = Some(ci as crate::civ::CivId);
                units.push(u);
            }
            let mut buildings: Vec<BuildingId> = Vec::new();
            for id in &c.unique_buildings {
                let Some(b) = self.building_id(id) else { return invalid(format!("civ '{}' has unknown unique building '{id}'", c.id)) };
                if building_replaces[b as usize].is_none() {
                    return invalid(format!("unique building '{id}' of civ '{}' doesn't replace anything", c.id));
                }
                if building_owner[b as usize].is_some() {
                    return invalid(format!("unique building '{id}' belongs to two civs"));
                }
                if buildings.iter().any(|&o| building_replaces[o as usize] == building_replaces[b as usize]) {
                    return invalid(format!("civ '{}' replaces the same building twice", c.id));
                }
                building_owner[b as usize] = Some(ci as crate::civ::CivId);
                buildings.push(b);
            }
            civ_units.push(units);
            civ_buildings.push(buildings);
        }
        if let Some(u) = (0..unit_owner.len()).find(|&u| unit_replaces[u].is_some() && unit_owner[u].is_none()) {
            return invalid(format!("unique unit '{}' isn't listed by any civ", self.units.units[u].id));
        }
        if let Some(b) = (0..building_owner.len()).find(|&b| building_replaces[b].is_some() && building_owner[b].is_none()) {
            return invalid(format!("unique building '{}' isn't listed by any civ", self.buildings.buildings[b].id));
        }
        // Upgrades lead to a standard unit of the same domain that's unlocked
        // later; unique units upgrade like what they replace unless they say otherwise.
        let mut unit_upgrade = Vec::new();
        for (i, u) in self.units.units.iter().enumerate() {
            let named = match &u.upgrades_to {
                None => None,
                Some(r) => match self.unit_type_id(r) {
                    Some(id) if unit_replaces[id as usize].is_none() && self.unit(id).domain == u.domain && id as usize != i => Some(id),
                    _ => return invalid(format!("unit '{}' upgrades to '{r}', which isn't a standard unit of its kind", u.id)),
                },
            };
            unit_upgrade.push(named.or_else(|| unit_replaces[i].and_then(|b| self.units.units[b as usize].upgrades_to.as_deref()).and_then(|r| self.unit_type_id(r))));
        }
        for (i, up) in unit_upgrade.iter().enumerate() {
            let (from, to) = (self.unit_tech[i], up.and_then(|u| self.unit_tech[u as usize]));
            if up.is_some() && (to.is_none() || from.is_some_and(|f| f >= to.unwrap())) {
                return invalid(format!("unit '{}' upgrades to a unit that doesn't come with a later tech", self.units.units[i].id));
            }
        }
        // No cycles: every step needs a later tech.
        self.unit_upgrade = unit_upgrade;
        self.unit_replaces = unit_replaces;
        self.building_replaces = building_replaces;
        self.civ_units = civ_units;
        self.civ_buildings = civ_buildings;
        self.unit_owner_civ = unit_owner;
        self.building_owner_civ = building_owner;
        Ok(())
    }

    /// The standard unit a unique unit replaces.
    pub fn unit_replaces(&self, id: UnitTypeId) -> Option<UnitTypeId> {
        self.unit_replaces[id as usize]
    }

    /// The standard unit a unit type upgrades to.
    pub fn unit_upgrade(&self, id: UnitTypeId) -> Option<UnitTypeId> {
        self.unit_upgrade[id as usize]
    }

    /// The standard building a building needs in the city first.
    pub fn building_requires(&self, id: BuildingId) -> Option<BuildingId> {
        self.building_requires[id as usize]
    }

    /// The era a tech belongs to.
    pub fn tech_era(&self, id: TechId) -> EraId {
        self.tech_era[id as usize]
    }

    pub fn era(&self, id: EraId) -> &EraDef {
        &self.techs.eras[id as usize]
    }

    /// The standard building a unique building replaces.
    pub fn building_replaces(&self, id: BuildingId) -> Option<BuildingId> {
        self.building_replaces[id as usize]
    }

    /// A civ's unique units.
    pub fn civ_unique_units(&self, civ: crate::civ::CivId) -> &[UnitTypeId] {
        &self.civ_units[civ as usize]
    }

    pub fn civ_unique_buildings(&self, civ: crate::civ::CivId) -> &[BuildingId] {
        &self.civ_buildings[civ as usize]
    }

    pub fn tech_id(&self, id: &str) -> Option<TechId> {
        self.techs.techs.iter().position(|t| t.id == id).map(|i| i as TechId)
    }

    pub fn tech(&self, id: TechId) -> &TechDef {
        &self.techs.techs[id as usize]
    }

    pub fn tech_count(&self) -> usize {
        self.techs.techs.len()
    }

    /// Direct prerequisites of a tech, as the tree draws them.
    pub fn tech_requires(&self, id: TechId) -> &[TechId] {
        &self.tech_requires[id as usize]
    }

    /// Everything a tech needs known first: its prerequisites and every tech
    /// of the previous era (a new era opens only once the last is complete).
    pub fn tech_needs(&self, id: TechId) -> &[TechId] {
        &self.tech_needs[id as usize]
    }

    /// The tech that unlocks a unit type, if any.
    pub fn unit_tech(&self, id: UnitTypeId) -> Option<TechId> {
        self.unit_tech[id as usize]
    }

    /// The tech that unlocks a building, if any.
    pub fn building_tech(&self, id: BuildingId) -> Option<TechId> {
        self.building_tech[id as usize]
    }

    /// The strategic resource a unit type needs, if any.
    pub fn unit_resource(&self, id: UnitTypeId) -> Option<ResourceId> {
        self.unit_resource[id as usize]
    }

    /// The strategic resource a building needs, if any.
    pub fn building_resource(&self, id: BuildingId) -> Option<ResourceId> {
        self.building_resource[id as usize]
    }

    pub fn improvement_tech(&self, id: ImprovementId) -> Option<TechId> {
        self.improvement_tech[id as usize]
    }

    /// The tech that lets cities dig tunnels.
    pub fn tunnel_tech(&self) -> TechId {
        self.tunnel_tech
    }

    /// The tech that lets cities dig canals.
    pub fn canal_tech(&self) -> TechId {
        self.canal_tech
    }

    /// The shallow water a canal leaves.
    pub fn canal_water(&self) -> TerrainId {
        self.canal_water
    }

    /// The tech that lets cities build bridges.
    pub fn bridge_tech(&self) -> TechId {
        self.bridge_tech
    }

    /// The tech that lets Workers lay railways.
    pub fn rail_tech(&self) -> TechId {
        self.rail_tech
    }

    /// The techs that let cities build forts and watchtowers.
    pub fn fort_tech(&self) -> TechId {
        self.fort_tech
    }

    pub fn watchtower_tech(&self) -> TechId {
        self.watchtower_tech
    }

    /// The tech that reveals a resource, if any.
    pub fn resource_tech(&self, id: ResourceId) -> Option<TechId> {
        self.resource_tech[id as usize]
    }

    /// The improvement that works a resource (`None`: none is needed).
    pub fn resource_improvement(&self, id: ResourceId) -> Option<ImprovementId> {
        self.resource_improvement[id as usize]
    }

    pub fn improvement_id(&self, id: &str) -> Option<ImprovementId> {
        self.improvements.improvements.iter().position(|i| i.id == id).map(|i| i as ImprovementId)
    }

    pub fn improvement(&self, id: ImprovementId) -> &ImprovementDef {
        &self.improvements.improvements[id as usize]
    }

    pub fn resource_id(&self, id: &str) -> Option<ResourceId> {
        self.resources.resources.iter().position(|r| r.id == id).map(|i| i as ResourceId)
    }

    pub fn resource(&self, id: ResourceId) -> &ResourceDef {
        &self.resources.resources[id as usize]
    }

    pub fn terrain_id(&self, id: &str) -> Option<TerrainId> {
        self.terrain.terrains.iter().position(|t| t.id == id).map(|i| i as TerrainId)
    }

    pub fn feature_id(&self, id: &str) -> Option<FeatureId> {
        self.terrain.features.iter().position(|f| f.id == id).map(|i| i as FeatureId)
    }

    pub fn unit_type_id(&self, id: &str) -> Option<UnitTypeId> {
        self.units.units.iter().position(|u| u.id == id).map(|i| i as UnitTypeId)
    }

    pub fn unit(&self, id: UnitTypeId) -> &UnitDef {
        &self.units.units[id as usize]
    }

    pub fn building_id(&self, id: &str) -> Option<BuildingId> {
        self.buildings.buildings.iter().position(|b| b.id == id).map(|i| i as BuildingId)
    }

    pub fn building(&self, id: BuildingId) -> &BuildingDef {
        &self.buildings.buildings[id as usize]
    }

    pub fn terrain(&self, id: TerrainId) -> &TerrainDef {
        &self.terrain.terrains[id as usize]
    }

    pub fn feature(&self, id: FeatureId) -> &FeatureDef {
        &self.terrain.features[id as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rules_load() {
        let r = Rules::load_default().unwrap();
        assert!(r.terrain_id("grassland").is_some());
        assert!(r.feature_id("forest").is_some());
        let writing = r.tech_id("writing").unwrap();
        assert_eq!(r.tech_requires(writing), &[r.tech_id("pottery").unwrap()]);
        assert_eq!(r.building_tech(r.building_id("library").unwrap()), Some(writing));
        assert_eq!(r.unit_tech(r.unit_type_id("warrior").unwrap()), None);
    }

    #[test]
    fn civs_and_their_uniques_resolve() {
        let r = Rules::load_default().unwrap();
        assert_eq!(r.civ_count(), 10);
        let dutch = r.civ_id("netherlands").unwrap();
        let rome = r.civ_id("rome").unwrap();
        let spearman = r.unit_type_id("spearman").unwrap();
        let schutter = r.unit_type_id("schutter").unwrap();
        assert_eq!(r.civ_unit(dutch, "spearman"), Some(schutter));
        assert_eq!(r.civ_unit(dutch, "warrior"), r.unit_type_id("warrior"));
        assert!(!r.civ_can_build_unit(dutch, spearman), "replaced");
        assert!(!r.civ_can_build_unit(rome, schutter), "someone else's");
        assert!(r.civ_can_build_unit(dutch, schutter));
        assert_eq!(r.unit_civ(schutter), Some(dutch));
        let waag = r.building_id("waag").unwrap();
        assert_eq!(r.civ_building(dutch, "market"), Some(waag));
        assert!(r.building_is(waag, "market"));
        // Unique units and buildings are unlocked by the same tech as what they replace.
        for u in 0..r.units.units.len() as UnitTypeId {
            if let Some(base) = r.unit_replaces(u) {
                assert_eq!(r.unit_tech(u), r.unit_tech(base), "{}", r.unit(u).id);
            }
        }
        for b in 0..r.buildings.buildings.len() as BuildingId {
            if let Some(base) = r.building_replaces(b) {
                assert_eq!(r.building_tech(b), r.building_tech(base), "{}", r.building(b).id);
            }
        }
        // Every civ has its own colour.
        for (i, a) in r.civs.civs.iter().enumerate() {
            for b in &r.civs.civs[i + 1..] {
                let (p, q) = (a.colors.primary, b.colors.primary);
                let d = (p.0 as i32 - q.0 as i32).abs() + (p.1 as i32 - q.1 as i32).abs() + (p.2 as i32 - q.2 as i32).abs();
                assert!(d > 60, "{} and {} look alike", a.id, b.id);
            }
        }
    }
}
