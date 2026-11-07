//! Saved games. A save file is a small header with what a save list shows
//! (civ, turn, date), readable on its own, followed by the gzipped game state.
//! The rules aren't saved: they are loaded from `data/` as usual, and the ids
//! they contained when saving are checked so a save from different rules is
//! refused instead of misread.
//!
//! Layout: `MAGIC`, the header's length (u32, little endian), the header as
//! RON, then the game as gzipped RON.

use crate::city::{City, CityId};
use crate::game::{Game, GameOptions, Player, PlayerId, Unit, UnitId};
use crate::map::Map;
use crate::rules::Rules;
use crate::diplomacy::{Payment, Proposal};
use crate::improvement::TileWorks;
use crate::resource::ResourceDeal;
use crate::victory::Victory;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};

const MAGIC: &[u8; 8] = b"FOURXSV1";
/// Bumped when the saved game state changes shape.
pub const SAVE_VERSION: u32 = 2;

/// What a save list shows about a save.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveMeta {
    pub version: u32,
    /// The saving player's civ id, name and leader.
    pub civ: String,
    pub civ_name: String,
    pub leader: String,
    pub turn: u32,
    pub players: u8,
    pub map_width: i32,
    pub map_height: i32,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
    pub autosave: bool,
    /// The player who saved (the one on screen).
    pub me: PlayerId,
}

#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    NotASave,
    /// Saved by a version of the game whose saves this one can't read.
    Version(u32),
    /// The rules in `data/` changed since the game was saved.
    RulesChanged(String),
    Format(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "{e}"),
            SaveError::NotASave => f.write_str("not a saved game"),
            SaveError::Version(v) => write!(f, "saved by an incompatible version (format {v})"),
            SaveError::RulesChanged(what) => write!(f, "the game rules changed since this was saved ({what})"),
            SaveError::Format(e) => write!(f, "damaged save: {e}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<std::io::Error> for SaveError {
    fn from(e: std::io::Error) -> Self {
        SaveError::Io(e)
    }
}

/// The ids of everything in the rules, in order (saves store indices).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RulesIds {
    terrains: Vec<String>,
    features: Vec<String>,
    units: Vec<String>,
    buildings: Vec<String>,
    techs: Vec<String>,
    civs: Vec<String>,
    #[serde(default)]
    improvements: Vec<String>,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(default)]
    traits: Vec<String>,
}

impl RulesIds {
    fn of(rules: &Rules) -> RulesIds {
        let ids = |v: &mut dyn Iterator<Item = &String>| v.cloned().collect();
        RulesIds {
            terrains: ids(&mut rules.terrain.terrains.iter().map(|t| &t.id)),
            features: ids(&mut rules.terrain.features.iter().map(|t| &t.id)),
            units: ids(&mut rules.units.units.iter().map(|t| &t.id)),
            buildings: ids(&mut rules.buildings.buildings.iter().map(|t| &t.id)),
            techs: ids(&mut rules.techs.techs.iter().map(|t| &t.id)),
            civs: ids(&mut rules.civs.civs.iter().map(|t| &t.id)),
            improvements: ids(&mut rules.improvements.improvements.iter().map(|t| &t.id)),
            resources: ids(&mut rules.resources.resources.iter().map(|t| &t.id)),
            traits: ids(&mut rules.experience.traits.iter().map(|t| &t.id)),
        }
    }

    /// What differs, if anything.
    fn difference(&self, other: &RulesIds) -> Option<&'static str> {
        [
            (self.terrains != other.terrains, "terrain"),
            (self.features != other.features, "features"),
            // New units and traits go at the end, so older saves' ids still hold.
            (!other.units.iter().zip(&self.units).all(|(a, b)| a == b) || other.units.len() > self.units.len(), "units"),
            (self.buildings != other.buildings, "buildings"),
            (self.techs != other.techs, "technologies"),
            (self.civs != other.civs, "civilizations"),
            (self.improvements != other.improvements, "improvements"),
            (self.resources != other.resources, "resources"),
            // Saves from before traits had none to refer to.
            (!other.traits.iter().zip(&self.traits).all(|(a, b)| a == b) || other.traits.len() > self.traits.len(), "general traits"),
        ]
        .into_iter()
        .find_map(|(differs, what)| differs.then_some(what))
    }
}

/// The game state as written (borrowed, so saving doesn't copy the game).
#[derive(Serialize)]
struct GameDataRef<'a> {
    ids: RulesIds,
    map: &'a Map,
    players: &'a [Player],
    units: &'a BTreeMap<UnitId, Unit>,
    cities: &'a BTreeMap<CityId, City>,
    land: &'a [Option<PlayerId>],
    turn: u32,
    options: GameOptions,
    victory: Option<Victory>,
    proposals: &'a [Proposal],
    payments: &'a [Payment],
    resource_deals: &'a [ResourceDeal],
    works: &'a [TileWorks],
    next_unit: UnitId,
    next_city: CityId,
}

#[derive(Deserialize)]
struct GameData {
    ids: RulesIds,
    map: Map,
    players: Vec<Player>,
    units: BTreeMap<UnitId, Unit>,
    cities: BTreeMap<CityId, City>,
    /// Who owns each tile.
    #[serde(default)]
    land: Vec<Option<PlayerId>>,
    /// Saves from before land belonged to players: each tile's city.
    #[serde(default)]
    territory: Vec<Option<CityId>>,
    turn: u32,
    options: GameOptions,
    #[serde(default)]
    victory: Option<Victory>,
    #[serde(default)]
    proposals: Vec<Proposal>,
    #[serde(default)]
    payments: Vec<Payment>,
    #[serde(default)]
    resource_deals: Vec<ResourceDeal>,
    #[serde(default)]
    works: Vec<TileWorks>,
    next_unit: UnitId,
    next_city: CityId,
}

impl Game {
    /// The header for a save of this game made by player `me`.
    pub fn save_meta(&self, me: PlayerId, autosave: bool, saved_at: u64) -> SaveMeta {
        let civ = self.civ(me);
        SaveMeta {
            version: SAVE_VERSION,
            civ: civ.id.clone(),
            civ_name: civ.name.clone(),
            leader: civ.leader.clone(),
            turn: self.turn,
            players: self.players.len() as u8,
            map_width: self.map.grid.width,
            map_height: self.map.grid.height,
            saved_at,
            autosave,
            me,
        }
    }

    /// Writes the game as a save file.
    pub fn write_save(&self, meta: &SaveMeta, out: &mut impl Write) -> Result<(), SaveError> {
        let header = ron::to_string(meta).map_err(|e| SaveError::Format(e.to_string()))?;
        out.write_all(MAGIC)?;
        out.write_all(&(header.len() as u32).to_le_bytes())?;
        out.write_all(header.as_bytes())?;
        let data = GameDataRef {
            ids: RulesIds::of(&self.rules),
            map: &self.map,
            players: &self.players,
            units: &self.units,
            cities: &self.cities,
            land: &self.territory,
            turn: self.turn,
            options: self.options,
            victory: self.victory,
            proposals: &self.proposals,
            payments: &self.payments,
            resource_deals: &self.resource_deals,
            works: &self.works,
            next_unit: self.next_unit,
            next_city: self.next_city,
        };
        let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
        let body = ron::to_string(&data).map_err(|e| SaveError::Format(e.to_string()))?;
        gz.write_all(body.as_bytes())?;
        gz.finish()?;
        Ok(())
    }

    /// Reads a save file written by [`Game::write_save`], with the current rules.
    pub fn read_save(rules: Rules, input: &mut impl Read) -> Result<(SaveMeta, Game), SaveError> {
        let meta = read_meta(input)?;
        let mut body = String::new();
        flate2::read::GzDecoder::new(input).read_to_string(&mut body).map_err(|e| SaveError::Format(e.to_string()))?;
        let data: GameData = ron::from_str(&body).map_err(|e| SaveError::Format(e.to_string()))?;
        if let Some(what) = RulesIds::of(&rules).difference(&data.ids) {
            return Err(SaveError::RulesChanged(what.to_string()));
        }
        let tiles = data.map.tiles.len();
        let mut works = data.works;
        works.resize(tiles, TileWorks::default());
        let land = if data.land.is_empty() {
            data.territory.iter().map(|c| c.and_then(|c| data.cities.get(&c)).map(|c| c.owner)).collect()
        } else {
            data.land
        };
        let sizes_ok = land.len() == tiles
            && data.players.iter().all(|p| p.explored.len() == tiles && p.visible.len() == tiles && p.techs.len() == rules.tech_count());
        if !sizes_ok || data.players.is_empty() || meta.me as usize >= data.players.len() {
            return Err(SaveError::Format("inconsistent sizes".into()));
        }
        let mut game = Game {
            rules,
            map: data.map,
            players: data.players,
            units: data.units,
            cities: data.cities,
            territory: land,
            districts: Vec::new(),
            turn: data.turn,
            options: data.options,
            victory: data.victory,
            proposals: data.proposals,
            payments: data.payments,
            resource_deals: data.resource_deals,
            passages: Game::find_passages(&works),
            trade_paths: BTreeMap::new(),
            works,
            next_unit: data.next_unit,
            next_city: data.next_city,
        };
        game.refresh_districts();
        Ok((meta, game))
    }
}

/// Reads just the header of a save file (for save lists).
pub fn read_meta(input: &mut impl Read) -> Result<SaveMeta, SaveError> {
    let mut magic = [0u8; 8];
    input.read_exact(&mut magic).map_err(|_| SaveError::NotASave)?;
    if &magic != MAGIC {
        return Err(SaveError::NotASave);
    }
    let mut len = [0u8; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > 64 * 1024 {
        return Err(SaveError::NotASave);
    }
    let mut header = vec![0u8; len];
    input.read_exact(&mut header)?;
    let text = String::from_utf8(header).map_err(|_| SaveError::NotASave)?;
    // Check the version before the rest of the header, whose shape may differ.
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    let v: Version = ron::from_str(&text).map_err(|_| SaveError::NotASave)?;
    if v.version != SAVE_VERSION {
        return Err(SaveError::Version(v.version));
    }
    ron::from_str(&text).map_err(|e| SaveError::Format(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Command;
    use crate::mapgen::{generate, MapSettings};
    use crate::rules::Ability;

    fn played_game() -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let mut g = Game::new(rules, map, 2);
        for p in 0..3 {
            let settler = g.units.values().find(|u| u.owner == p && g.unit_def(u).has(Ability::FoundCity)).unwrap().id;
            g.apply(p, Command::FoundCity { unit: settler }).unwrap();
        }
        for _ in 0..25 {
            g.apply(0, Command::EndTurn).unwrap();
        }
        g
    }

    fn bytes(g: &Game) -> Vec<u8> {
        let mut out = Vec::new();
        g.write_save(&g.save_meta(0, false, 1000), &mut out).unwrap();
        out
    }

    #[test]
    fn a_loaded_game_plays_on_exactly_like_the_original() {
        let mut original = played_game();
        let saved = bytes(&original);
        let (meta, mut loaded) = Game::read_save(Rules::load_default().unwrap(), &mut saved.as_slice()).unwrap();
        assert_eq!(meta.turn, original.turn);
        assert_eq!(meta.civ, "netherlands");
        assert_eq!(bytes(&loaded), saved, "round trip changed the state");
        for _ in 0..20 {
            original.apply(0, Command::EndTurn).unwrap();
            loaded.apply(0, Command::EndTurn).unwrap();
        }
        assert_eq!(bytes(&loaded), bytes(&original), "the games diverged after loading");
    }

    #[test]
    fn the_header_reads_on_its_own() {
        let g = played_game();
        let saved = bytes(&g);
        let meta = read_meta(&mut saved.as_slice()).unwrap();
        assert_eq!(meta.players, 3);
        assert_eq!(meta.saved_at, 1000);
        // Compressed: well under the raw state's size.
        assert!(saved.len() < 400_000, "save is {} bytes", saved.len());
    }

    #[test]
    fn bad_files_are_refused() {
        assert!(matches!(read_meta(&mut &b"hello world, not a save"[..]), Err(SaveError::NotASave)));
        let mut saved = bytes(&played_game());
        let n = saved.len();
        saved.truncate(n / 2);
        assert!(Game::read_save(Rules::load_default().unwrap(), &mut saved.as_slice()).is_err());
    }

    #[test]
    fn saves_from_other_rules_are_refused() {
        let saved = bytes(&played_game());
        let mut rules = Rules::load_default().unwrap();
        rules.techs.techs[0].id = "renamed".into();
        let err = Game::read_save(rules, &mut saved.as_slice()).err().unwrap();
        assert!(matches!(err, SaveError::RulesChanged(_)), "{err}");
    }
}
