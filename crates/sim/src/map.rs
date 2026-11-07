//! The world map: terrain per tile plus derived queries (yields, movement).

use crate::hex::{MapGrid, TileId};
use crate::rules::{FeatureId, ResourceId, Rules, TerrainId, Yields};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Elevation {
    Flat,
    Hills,
    Mountains,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Tile {
    pub terrain: TerrainId,
    pub elevation: Elevation,
    pub feature: Option<FeatureId>,
    /// Bitmask of hex directions (see `hex::DIRECTIONS`) a river flows through.
    pub rivers: u8,
    /// Normalised height (0 = deep ocean, 1 = highest peak). Used for rendering.
    pub height: f32,
    /// Mean annual temperature in °C.
    pub temperature: f32,
    /// Annual precipitation, normalised 0 (arid) ..1 (rainforest).
    pub moisture: f32,
    #[serde(default)]
    pub resource: Option<ResourceId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Map {
    pub grid: MapGrid,
    pub tiles: Vec<Tile>,
    pub seed: u64,
    /// Where civs may start (empty: anywhere), set by the map type. Only used
    /// when a game is set up, so it isn't saved.
    #[serde(skip)]
    pub start_land: Vec<bool>,
    /// The map type it was generated as.
    #[serde(default)]
    pub kind: crate::shapes::MapType,
}

impl Map {
    pub fn tile(&self, id: TileId) -> &Tile {
        &self.tiles[id as usize]
    }

    pub fn is_water(&self, rules: &Rules, id: TileId) -> bool {
        rules.terrain(self.tile(id).terrain).water
    }

    pub fn has_river(&self, id: TileId) -> bool {
        self.tile(id).rivers != 0
    }

    pub fn yields(&self, rules: &Rules, id: TileId) -> Yields {
        Self::yields_of(rules, self.tile(id))
    }

    /// What a tile like `t` yields from its land alone.
    pub fn yields_of(rules: &Rules, t: &Tile) -> Yields {
        let tr = &rules.terrain;
        let mut y = rules.terrain(t.terrain).yields();
        match t.elevation {
            Elevation::Flat => {}
            Elevation::Hills => y += tr.hills.yields(),
            Elevation::Mountains => y += tr.mountains.yields(),
        }
        if let Some(f) = t.feature {
            y += rules.feature(f).yields();
        }
        if t.rivers != 0 {
            y += tr.river.yields();
        }
        Yields {
            food: y.food.max(0),
            production: y.production.max(0),
            gold: y.gold.max(0),
        }
    }

    /// The ice on a tile at a point in the seasons (`season`, see
    /// `climate::season`): from its temperature then, with a fixed per-tile
    /// offset so the ice edge is ragged. Lakes freeze like the land around
    /// them, the sea less.
    pub fn sea_ice(&self, rules: &Rules, id: TileId, season: f32) -> crate::climate::SeaIce {
        use crate::climate::{latitude, swing, SeaIce};
        let t = self.tile(id);
        let def = rules.terrain(t.terrain);
        if !def.water {
            return SeaIce::Open;
        }
        let row = id as i32 / self.grid.width;
        let land = if def.id == "lake" { 1.0 } else { 0.0 };
        let jitter = (id.wrapping_mul(2_654_435_761) >> 16 & 0xffff) as f32 / 65535.0 * 4.0 - 2.0;
        let temperature = t.temperature + swing(latitude(row, self.grid.height), land) * season + jitter;
        let ice = &rules.terrain.sea_ice;
        if temperature < ice.pack_below {
            SeaIce::Pack
        } else if temperature < ice.thin_below {
            SeaIce::Thin
        } else {
            SeaIce::Open
        }
    }

    /// Movement points needed to enter the tile, or `None` if impassable for land units.
    pub fn land_move_cost(&self, rules: &Rules, id: TileId) -> Option<i32> {
        let t = self.tile(id);
        let base = rules.terrain(t.terrain);
        if base.water {
            return None;
        }
        let tr = &rules.terrain;
        let mut cost = base.move_cost;
        match t.elevation {
            Elevation::Flat => {}
            Elevation::Hills => cost += tr.hills.move_cost,
            Elevation::Mountains => {
                if tr.mountains.impassable {
                    return None;
                }
                cost += tr.mountains.move_cost;
            }
        }
        if let Some(f) = t.feature {
            let feature = rules.feature(f);
            if feature.impassable {
                return None;
            }
            cost += feature.move_cost;
        }
        Some(cost.max(1))
    }

    /// Defense bonus in percent for a unit standing on the tile.
    pub fn defense_bonus(&self, rules: &Rules, id: TileId) -> i32 {
        let t = self.tile(id);
        let tr = &rules.terrain;
        let mut d = rules.terrain(t.terrain).defense;
        if t.elevation == Elevation::Hills {
            d += tr.hills.defense;
        }
        if let Some(f) = t.feature {
            d += rules.feature(f).defense;
        }
        d
    }
}
