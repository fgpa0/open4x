//! Game state: players, units, cities and turns. State changes only through
//! [`Game::apply`] with a [`Command`], which returns the [`Event`]s describing
//! what happened (for the renderer, replays and AI).

use crate::city::{Build, City, CityId, Focus};
use crate::civ::{CivId, StartBias};
use crate::hex::TileId;
use crate::map::{Elevation, Map};
use crate::path::{self, Path};
use crate::improvement::Passage;
use crate::rules::{Ability, Domain, Rules, TechGrant, TechId, UnitClass, UnitTypeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type PlayerId = u8;
pub type UnitId = u32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub civ: CivId,
    /// The civ's name.
    pub name: String,
    /// sRGB colour used for this player's units and borders.
    pub color: [u8; 3],
    /// sRGB trim colour (emblems, banner edges).
    pub secondary: [u8; 3],
    pub human: bool,
    /// Tiles this player has ever seen.
    pub explored: Vec<bool>,
    /// How many of this player's units and cities currently see each tile.
    pub visible: Vec<u16>,
    pub gold: i32,
    /// Cities founded so far (picks the next city name).
    pub cities_founded: u32,
    /// Known technologies, by tech id.
    pub techs: Vec<bool>,
    /// Science put into each technology so far.
    pub research: Vec<i32>,
    /// What to research, in order (the front is being researched now).
    pub research_path: Vec<TechId>,
    /// Science not yet spent (overflow waiting for the next choice).
    pub science_stored: i32,
    /// What this player remembers about each player, by player id.
    #[serde(default)]
    pub relations: Vec<Relation>,
    /// Players this one has met, by player id.
    #[serde(default)]
    pub met: Vec<bool>,
    /// Out of the game: lost every city and every unit that could found one.
    #[serde(default)]
    pub eliminated: bool,
    /// Generals named so far (from the civ's list, see `experience.rs`).
    #[serde(default)]
    pub generals_named: u32,
    /// Admirals named so far.
    #[serde(default)]
    pub admirals_named: u32,
}

impl Player {
    pub fn sees(&self, tile: TileId) -> bool {
        self.visible[tile as usize] > 0
    }
}

/// How one player stands with another: at war or not, and who attacked whom
/// (read by the AI for grudges and wars; diplomacy will build on it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    /// Attacks they made on our units and cities.
    pub attacked_us: u32,
    /// Attacks we made on theirs.
    pub we_attacked: u32,
    /// The turn of their latest attack on us.
    pub last_attacked_us: Option<u32>,
    /// The turn of our latest attack on them.
    pub last_we_attacked: Option<u32>,
    /// Cities they took from us.
    pub cities_lost: u32,
    /// At war (always the same both ways). Players start at peace.
    #[serde(default)]
    pub at_war: bool,
    /// The turn the current war began.
    #[serde(default)]
    pub war_since: Option<u32>,
    /// The turn we last proposed them a deal (see `diplomacy.rs`).
    #[serde(default)]
    pub last_proposal: Option<u32>,
    /// After a peace treaty, neither side may declare war before this turn.
    #[serde(default)]
    pub truce_until: Option<u32>,
    /// Our borders are open to them until this turn (a deal; see `diplomacy.rs`).
    #[serde(default)]
    pub borders_open_until: Option<u32>,
    /// Allies: borders open both ways, and no war between them.
    #[serde(default)]
    pub allied: bool,
}

/// A standing order: what the unit keeps doing until told otherwise.
/// Which of a tile's places a unit takes (see [`Game::slot`]): one unit of
/// each per tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Civilian,
    /// Land soldiers.
    Land,
    /// Ships.
    Sea,
    /// Generals and Admirals.
    Leader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Order {
    /// Waiting for orders.
    None,
    /// Moving to a tile, over several turns if needed.
    Goto(TileId),
    /// Automatically exploring (driven by the automation in the `ai` crate).
    Explore,
    /// Dug in: stays put and doesn't ask for orders.
    Fortify,
    /// Stays put and doesn't ask for orders until woken.
    Sleep,
    /// Done for this turn; asks again next turn.
    Skip,
    /// Building an improvement or road where it stands (see `improvement.rs`).
    Build(crate::improvement::Work),
    /// A Worker improving its owner's land on its own (driven by the
    /// automation in the `ai` crate).
    Automate,
    /// An automated Worker building something; it goes back to
    /// [`Order::Automate`] when done.
    AutoBuild(crate::improvement::Work),
    /// Chasing an enemy unit: closes in each turn and attacks when it can,
    /// until the target is gone (see `combat.rs`). `last` is where it was
    /// last seen.
    Hunt { target: UnitId, last: TileId },
    /// A Merchant running a trade route (see `trade.rs`).
    Trade(crate::trade::Route),
    /// One of a group sent to take an enemy city: they gather near it out
    /// of its fire, then strike together, until it falls (driven by the
    /// automation in the `ai` crate, see `ai::orders`).
    Take { city: CityId },
    /// Guarding one of its owner's cities: it stays in or next to it and
    /// strikes enemies that come close (see `ai::orders`).
    Defend { city: CityId },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub owner: PlayerId,
    pub kind: UnitTypeId,
    pub tile: TileId,
    /// Movement points left this turn.
    pub moves: i32,
    pub order: Order,
    /// Hit points, out of [`crate::combat::MAX_HP`].
    pub hp: i32,
    /// Attacks made this turn.
    #[serde(default)]
    pub attacks: u8,
    /// Whether it has moved this turn.
    #[serde(default)]
    pub moved: bool,
    /// The turn it asks for orders although asleep or fortified: an enemy
    /// came within striking distance or attacked it (see `combat.rs`).
    /// Cleared by any order its owner gives it.
    #[serde(default)]
    pub alerted: Option<u32>,
    /// A General's or Admiral's name.
    #[serde(default)]
    pub name: Option<String>,
    /// Experience from battle: its rank, or a General's level (see `experience.rs`).
    #[serde(default)]
    pub xp: u32,
    /// A General's chosen traits.
    #[serde(default)]
    pub traits: Vec<crate::experience::TraitId>,
}

/// Who plays and with which rules options. Player 0 is the human.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSetup {
    /// Each player's civ, the human first. No civ may appear twice.
    pub civs: Vec<CivId>,
    pub options: GameOptions,
    /// Start tiles the setup chose, by player (`None`, or missing: the game
    /// picks one). A tile where no one may start (see [`Game::may_start_at`])
    /// or too close to an earlier player's is ignored.
    #[serde(default)]
    pub starts: Vec<Option<TileId>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameOptions {
    /// Most military units a player may have (`None`: no limit).
    pub unit_limit: Option<u32>,
    /// The game ends after this turn and the highest score wins (`None`: no limit).
    #[serde(default)]
    pub turn_limit: Option<u32>,
    /// On the Earth map, every civ with a `home` starts there (True Start
    /// Locations). Starts the setup chose still come first.
    #[serde(default)]
    pub true_starts: bool,
}

/// How close two chosen starts may be (see [`GameSetup::starts`]).
pub const MIN_FIXED_SPACING: i32 = 3;

impl GameSetup {
    /// The first `players` civs in the rules.
    pub fn first(rules: &Rules, players: usize) -> GameSetup {
        let n = players.clamp(1, rules.civ_count());
        GameSetup { civs: (0..n as CivId).collect(), options: GameOptions::default(), starts: Vec::new() }
    }

    /// `players` different civs picked at random from the seed; `fixed` are
    /// chosen civs (`None`: random) for the first slots.
    pub fn random(rules: &Rules, players: usize, fixed: &[Option<CivId>], seed: u64) -> GameSetup {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;
        let n = players.clamp(1, rules.civ_count());
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed ^ 0x5eed_c1f5);
        let mut pool: Vec<CivId> =
            (0..rules.civ_count() as CivId).filter(|c| !fixed.contains(&Some(*c))).collect();
        pool.shuffle(&mut rng);
        let civs = (0..n).map(|i| fixed.get(i).copied().flatten().unwrap_or_else(|| pool.pop().expect("enough civs"))).collect();
        GameSetup { civs, options: GameOptions::default(), starts: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Move towards a tile (over several turns if needed); sets a Goto order.
    MoveUnit { unit: UnitId, to: TileId },
    SetOrder { unit: UnitId, order: Order },
    /// Attack whatever is on a tile (adjacent for melee, within range for
    /// ranged).
    Attack { unit: UnitId, tile: TileId },
    /// Chase an enemy unit and attack it, over several turns if needed; sets a
    /// Hunt order.
    Hunt { unit: UnitId, target: UnitId },
    /// A settler founds a city where it stands.
    FoundCity { unit: UnitId },
    /// A Worker starts building an improvement or road where it stands.
    Build { unit: UnitId, work: crate::improvement::Work },
    /// Replaces a city's build queue.
    SetQueue { city: CityId, queue: Vec<Build> },
    SetFocus { city: CityId, focus: Focus },
    /// Whether the governor refills the queue when it runs empty.
    SetAutoQueue { city: CityId, on: bool },
    /// Buys a unit or building in a city with gold (see `purchase.rs`).
    Buy { city: CityId, build: Build },
    /// Buys an unclaimed tile next to a city's borders with gold.
    BuyTile { city: CityId, tile: TileId },
    /// Upgrades a unit to the best unit its line leads to, for gold (see `purchase.rs`).
    Upgrade { unit: UnitId },
    /// A General or Admiral with a level to spend takes a trait (see `experience.rs`).
    ChooseTrait { unit: UnitId, trait_id: crate::experience::TraitId },
    /// A Merchant takes up a trade route between its home (its owner's
    /// nearest city) and `to` (see `trade.rs`).
    Trade { unit: UnitId, to: CityId },
    /// A Merchant moves to another of its owner's cities at once, to trade
    /// from there (it ends its route and takes its turn).
    Transfer { unit: UnitId, city: CityId },
    /// Whether a Merchant keeps its route (`repeat`) or takes the best paying
    /// one again each time it's home from a trip.
    RepeatRoute { unit: UnitId, repeat: bool },
    /// Burns a city taken this turn to the ground (see `combat.rs`); keeping
    /// it needs no command.
    RazeCity { city: CityId },
    /// Research towards a technology (its missing prerequisites first).
    SetResearch { tech: TechId },
    /// Go to war with a player you've met.
    DeclareWar { on: PlayerId },
    /// Proposes a trade (which may include peace); they accept or refuse.
    ProposeDeal { to: PlayerId, deal: crate::diplomacy::Deal },
    /// Accepts the deal `from` proposed.
    AcceptDeal { from: PlayerId },
    /// Turns down the deal `from` proposed.
    RefuseDeal { from: PlayerId },
    /// Ends the turn for everyone: moves are restored and Goto and Hunt orders
    /// continue.
    EndTurn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    UnitSpawned { unit: UnitId },
    UnitRemoved { unit: UnitId },
    CityFounded { city: CityId },
    CityGrew { city: CityId, population: i32 },
    CityBuilt { city: CityId, build: Build },
    /// Anything about the city changed (queue, focus, worked tiles...).
    CityChanged { city: CityId },
    BordersChanged { player: PlayerId },
    /// `by`'s unit pulled down `owner`'s fort (or watchtower) on `tile`.
    PostDestroyed { tile: TileId, owner: PlayerId, by: PlayerId, fort: bool },
    /// The unit stepped through these tiles, in order.
    UnitMoved { unit: UnitId, path: Vec<TileId> },
    OrderChanged { unit: UnitId, order: Order },
    /// An improvement or road was finished on a tile (its feature may be gone).
    TileImproved { tile: TileId },
    /// A tile's land changed for good: a canal turned it into water.
    TerrainChanged { tile: TileId },
    /// Tiles a player saw for the first time.
    Explored { player: PlayerId, tiles: Vec<TileId> },
    VisibilityChanged { player: PlayerId },
    TurnStarted { turn: u32 },
    TechResearched { player: PlayerId, tech: TechId },
    /// A unit attacked a tile (hit points are already updated; deaths, captures
    /// and the attacker advancing follow as their own events).
    Combat(crate::combat::CombatReport),
    /// A new unit that was captured from `from` (the old one was removed).
    UnitCaptured { unit: UnitId, from: PlayerId },
    CityCaptured { city: CityId, from: PlayerId },
    /// `by` razed a city it took from `from`: it's gone, with its land.
    CityRazed { city: CityId, name: String, tile: TileId, by: PlayerId, from: PlayerId },
    /// A unit of `owner`'s in a city that fell, with nowhere to go (the
    /// others are pushed out, see `combat.rs`).
    LostWithCity { kind: UnitTypeId, owner: PlayerId, city: CityId },
    ResearchChanged { player: PlayerId },
    /// Two players met for the first time (sent once, with the one who saw
    /// the other as `player`).
    Met { player: PlayerId, other: PlayerId },
    WarDeclared { by: PlayerId, on: PlayerId },
    DealProposed { from: PlayerId, to: PlayerId },
    DealRefused { by: PlayerId, from: PlayerId },
    /// `from` proposed the deal and `to` accepted it.
    DealMade { from: PlayerId, to: PlayerId, deal: crate::diplomacy::Deal },
    /// A city bought a unit or building for `gold`.
    Bought { city: CityId, build: Build, gold: i32 },
    /// A unit was upgraded from type `from` for `gold`.
    UnitUpgraded { unit: UnitId, from: crate::rules::UnitTypeId, gold: i32 },
    /// A unit rose to a new rank (1 = the first, see `experience.rs`).
    Promoted { unit: UnitId, rank: u8 },
    /// A General reached a new level and may choose a trait.
    GeneralPromoted { unit: UnitId, level: u8 },
    TraitChosen { unit: UnitId, trait_id: crate::experience::TraitId },
    /// A city bought a tile for `gold`.
    TileBought { city: CityId, tile: TileId, gold: i32 },
    /// A city changed hands in a deal.
    CityTraded { city: CityId, from: PlayerId, to: PlayerId },
    /// A merchant sold its goods at the far end of its route.
    Traded(crate::trade::TradeReport),
    /// A merchant moved to another of its owner's cities (see `trade.rs`).
    UnitTransferred { unit: UnitId, city: CityId },
    /// A merchant's route can't go on (a city lost, or war); it waits for orders.
    RouteEnded { unit: UnitId, city: CityId },
    /// A merchant home from a trip took a better paying route (from trading
    /// with `from` to `to`).
    RouteChanged { unit: UnitId, from: CityId, to: CityId },
    /// `by` captured a merchant of `from`'s and took its cargo.
    Plundered { by: PlayerId, from: PlayerId, gold: i32 },
    /// A player is out of the game (their remaining units are removed first).
    PlayerEliminated { player: PlayerId },
    /// Someone won. Play can go on afterwards, but there's no second winner.
    GameOver(crate::victory::Victory),
    /// A unit of `kind` was disbanded: its owner couldn't pay its upkeep.
    UnitDisbanded { player: PlayerId, kind: UnitTypeId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandError {
    NoSuchUnit,
    NotYourUnit,
    Unreachable,
    NoMovesLeft,
    NoSuchCity,
    NotYourCity,
    CantFoundCity(&'static str),
    CantAttack(&'static str),
    NoSuchTech,
    AlreadyKnown,
    /// A build that needs a technology (or a building the city already has).
    NotAvailable,
    NoSuchPlayer,
    /// Diplomacy with a player you haven't met.
    NotMet,
    AlreadyAtWar,
    /// Attacking a player you're at peace with.
    NotAtWar,
    /// Declaring war during the truce after a peace treaty.
    Truce,
    /// Declaring war on an ally.
    Allied,
    /// Accepting or refusing a deal nobody proposed.
    NoDeal,
    /// A deal that can't be carried out (see `diplomacy::Deal::problem`).
    BadDeal(&'static str),
    /// A purchase that can't be made (see `purchase.rs`).
    CantBuy(&'static str),
    /// A Worker can't build that here (see `improvement.rs`).
    CantBuild(&'static str),
    /// A Merchant can't take up that route (see `trade.rs`).
    CantTrade(&'static str),
    /// A city that can't be razed (see `Game::raze_problem`).
    CantRaze(&'static str),
    /// An order a unit can't take (see `Order::Take` and `Order::Defend`).
    CantOrder(&'static str),
    /// A trait a General can't take (see `experience.rs`).
    CantPromote(&'static str),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            CommandError::NoSuchUnit => "no such unit",
            CommandError::NotYourUnit => "not your unit",
            CommandError::Unreachable => "can't get there",
            CommandError::NoMovesLeft => "no moves left",
            CommandError::NoSuchCity => "no such city",
            CommandError::NotYourCity => "not your city",
            CommandError::CantFoundCity(why) | CommandError::CantAttack(why) => why,
            CommandError::NoSuchTech => "no such technology",
            CommandError::AlreadyKnown => "already researched",
            CommandError::NotAvailable => "not available yet",
            CommandError::NoSuchPlayer => "no such player",
            CommandError::NotMet => "you haven't met them",
            CommandError::AlreadyAtWar => "already at war",
            CommandError::NotAtWar => "you're at peace with them",
            CommandError::Truce => "a peace treaty forbids war for now",
            CommandError::Allied => "you're allied with them",
            CommandError::NoDeal => "there's no such deal",
            CommandError::BadDeal(why) | CommandError::CantBuy(why) | CommandError::CantBuild(why) | CommandError::CantTrade(why) | CommandError::CantRaze(why) | CommandError::CantOrder(why) | CommandError::CantPromote(why) => why,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Game {
    pub rules: Rules,
    pub map: Map,
    pub players: Vec<Player>,
    pub units: BTreeMap<UnitId, Unit>,
    pub cities: BTreeMap<CityId, City>,
    /// The player who owns each tile (land belongs to players; cities work
    /// the owned land in their reach, see [`Game::district`]).
    pub territory: Vec<Option<PlayerId>>,
    /// The city that works each owned tile: the owner's nearest city within
    /// [`crate::city::MAX_CITY_RADIUS`] (the older one on a tie). Follows
    /// `territory` and the cities (`refresh_districts`); not saved.
    pub(crate) districts: Vec<Option<CityId>>,
    pub turn: u32,
    pub options: GameOptions,
    /// Set once someone has won.
    pub victory: Option<crate::victory::Victory>,
    /// Deals proposed and not yet answered.
    pub proposals: Vec<crate::diplomacy::Proposal>,
    /// Gold paid every turn under deals.
    pub payments: Vec<crate::diplomacy::Payment>,
    /// Resources handed over under deals.
    pub resource_deals: Vec<crate::resource::ResourceDeal>,
    /// Improvements, roads and work in progress on each tile.
    pub works: Vec<crate::improvement::TileWorks>,
    /// Tiles with a tunnel or bridge, in order (follows `works`).
    pub(crate) passages: Vec<TileId>,
    /// Each merchant's way to the goal of its route, kept between turns so
    /// it isn't searched again every turn (not saved; see `trade.rs`).
    pub(crate) trade_paths: BTreeMap<UnitId, crate::trade::TradeWay>,
    pub(crate) next_unit: UnitId,
    pub(crate) next_city: CityId,
}

impl Game {
    /// A new game on `map` with one human and `ai_players` computer players,
    /// playing the first civs in the rules.
    pub fn new(rules: Rules, map: Map, ai_players: usize) -> Game {
        let setup = GameSetup::first(&rules, ai_players + 1);
        Game::with_setup(rules, map, &setup)
    }

    /// A new game for `setup`. Every player starts with a settler, a warrior
    /// and a scout.
    pub fn with_setup(rules: Rules, map: Map, setup: &GameSetup) -> Game {
        let count = setup.civs.len();
        let tiles = map.tiles.len();
        let rgb = |c: (u8, u8, u8)| [c.0, c.1, c.2];
        let players = setup
            .civs
            .iter()
            .enumerate()
            .map(|(i, &civ)| Player {
                civ,
                name: rules.civ(civ).name.clone(),
                color: rgb(rules.civ(civ).colors.primary),
                secondary: rgb(rules.civ(civ).colors.secondary),
                human: i == 0,
                explored: vec![false; tiles],
                visible: vec![0; tiles],
                gold: 0,
                cities_founded: 0,
                techs: vec![false; rules.tech_count()],
                research: vec![0; rules.tech_count()],
                research_path: Vec::new(),
                science_stored: 0,
                relations: vec![Relation::default(); count],
                met: vec![false; count],
                eliminated: false,
                generals_named: 0,
                admirals_named: 0,
            })
            .collect();
        let mut game = Game {
            rules,
            map,
            players,
            units: BTreeMap::new(),
            cities: BTreeMap::new(),
            territory: vec![None; tiles],
            districts: vec![None; tiles],
            turn: 1,
            options: setup.options,
            victory: None,
            proposals: Vec::new(),
            payments: Vec::new(),
            resource_deals: Vec::new(),
            works: vec![Default::default(); tiles],
            passages: Vec::new(),
            trade_paths: BTreeMap::new(),
            next_unit: 1,
            next_city: 1,
        };
        let starts = game.choose_starts(&setup.starts);
        for (p, start) in starts.into_iter().enumerate() {
            let Some(start) = start else { continue };
            let p = p as PlayerId;
            game.balance_start(p, start);
            for kind in ["settler", "warrior"] {
                if let Some(k) = game.rules.unit_type_id(kind) {
                    game.spawn_unit(p, k, start);
                }
            }
            // The scout starts next door.
            let scout_tile = game.map.grid.neighbors(start).find(|&t| game.map.land_move_cost(&game.rules, t).is_some());
            if let (Some(k), Some(t)) = (game.rules.unit_type_id("scout"), scout_tile) {
                game.spawn_unit(p, k, t);
            }
        }
        for p in 0..count as PlayerId {
            game.update_visibility(p);
        }
        game
    }

    pub fn spawn_unit(&mut self, owner: PlayerId, kind: UnitTypeId, tile: TileId) -> UnitId {
        let id = self.next_unit;
        self.next_unit += 1;
        let moves = path::points(self.rules.unit(kind).moves);
        self.units.insert(id, Unit { id, owner, kind, tile, moves, order: Order::None, hp: crate::combat::MAX_HP, attacks: 0, moved: false, alerted: None, name: None, xp: 0, traits: Vec::new() });
        self.name_general(id);
        id
    }

    /// Whether `player` has met `other` (everyone knows themselves).
    pub fn has_met(&self, player: PlayerId, other: PlayerId) -> bool {
        player == other || self.players[player as usize].met.get(other as usize).copied().unwrap_or(false)
    }

    /// Players meet the first time either sees the other's units, cities or
    /// borders. Returns a [`Event::Met`] for each new meeting.
    fn meet_visible(&mut self, player: PlayerId) -> Vec<Event> {
        let p = &self.players[player as usize];
        let mut seen: Vec<PlayerId> = self.units.values().filter(|u| u.owner != player && p.sees(u.tile)).map(|u| u.owner).collect();
        seen.extend(self.cities.values().filter(|c| c.owner != player && p.sees(c.tile)).map(|c| c.owner));
        for (t, owner) in self.territory.iter().enumerate() {
            if let Some(o) = owner.filter(|_| p.visible[t] > 0) {
                seen.push(o);
            }
        }
        seen.retain(|&q| q != player);
        seen.sort();
        seen.dedup();
        let mut events = Vec::new();
        let n = self.players.len();
        for other in seen {
            if self.has_met(player, other) {
                continue;
            }
            for (a, b) in [(player, other), (other, player)] {
                let met = &mut self.players[a as usize].met;
                met.resize(n, false);
                met[b as usize] = true;
            }
            events.push(Event::Met { player, other });
        }
        events
    }

    /// Whether two players are at war.
    pub fn at_war(&self, a: PlayerId, b: PlayerId) -> bool {
        a != b && self.relation(a, b).at_war
    }

    pub(crate) fn declare_war(&mut self, by: PlayerId, on: PlayerId) -> Result<Vec<Event>, CommandError> {
        if on as usize >= self.players.len() || on == by {
            return Err(CommandError::NoSuchPlayer);
        }
        if !self.has_met(by, on) {
            return Err(CommandError::NotMet);
        }
        if self.at_war(by, on) {
            return Err(CommandError::AlreadyAtWar);
        }
        if self.truce_left(by, on) > 0 {
            return Err(CommandError::Truce);
        }
        if self.allied(by, on) {
            return Err(CommandError::Allied);
        }
        let turn = self.turn;
        for (a, b) in [(by, on), (on, by)] {
            let r = self.relation_mut(a, b);
            r.at_war = true;
            r.war_since = Some(turn);
            r.borders_open_until = None;
        }
        // War cancels what they pay and give each other.
        self.payments.retain(|p| !((p.from == by && p.to == on) || (p.from == on && p.to == by)));
        self.resource_deals.retain(|d| !((d.from == by && d.to == on) || (d.from == on && d.to == by)));
        let mut events = vec![Event::WarDeclared { by, on }];
        events.extend(self.expel_at_war(by, on));
        Ok(events)
    }

    /// What `player` remembers about `other`.
    pub fn relation(&self, player: PlayerId, other: PlayerId) -> Relation {
        self.players[player as usize].relations.get(other as usize).copied().unwrap_or_default()
    }

    pub(crate) fn relation_mut(&mut self, player: PlayerId, other: PlayerId) -> &mut Relation {
        let n = self.players.len();
        let relations = &mut self.players[player as usize].relations;
        relations.resize(n, Relation::default());
        &mut relations[other as usize]
    }

    /// Remembers that `attacker` attacked `defender` this turn.
    pub(crate) fn record_attack(&mut self, attacker: PlayerId, defender: PlayerId) {
        if attacker == defender {
            return;
        }
        let turn = self.turn;
        let theirs = self.relation_mut(defender, attacker);
        theirs.attacked_us += 1;
        theirs.last_attacked_us = Some(turn);
        let ours = self.relation_mut(attacker, defender);
        ours.we_attacked += 1;
        ours.last_we_attacked = Some(turn);
    }

    /// Military units a player has.
    pub fn military_units(&self, player: PlayerId) -> usize {
        self.units.values().filter(|u| u.owner == player && self.unit_def(u).class == crate::rules::UnitClass::Military).count()
    }

    /// Whether the unit limit stops `player` from getting another military unit.
    pub fn at_unit_limit(&self, player: PlayerId) -> bool {
        self.options.unit_limit.is_some_and(|limit| self.military_units(player) >= limit as usize)
    }

    /// How well a start tile suits a civ's start bias.
    fn start_bias_score(&self, civ: CivId, tile: TileId) -> i32 {
        let map = &self.map;
        let rules = &self.rules;
        let area = map.grid.within(tile, 2);
        let count = |f: &dyn Fn(TileId) -> bool| area.iter().filter(|&&t| f(t)).count() as i32;
        let terrain = |t: TileId, id: &str| rules.terrain(map.tile(t).terrain).id == id;
        let feature = |t: TileId, id: &str| map.tile(t).feature.is_some_and(|f| rules.feature(f).id == id);
        let boreal = |t: TileId| (crate::climate::TUNDRA..crate::climate::BOREAL).contains(&map.tile(t).temperature);
        let biases = &rules.civ(civ).start_bias;
        biases
            .iter()
            .enumerate()
            .map(|(rank, b)| {
                // Earlier biases matter more.
                let weight = 3 - rank.min(2) as i32;
                let hits = match b {
                    StartBias::Coast => {
                        if map.grid.neighbors(tile).any(|n| terrain(n, "coast")) { 6 } else { count(&|t| terrain(t, "coast")).min(3) }
                    }
                    StartBias::River => {
                        if map.has_river(tile) { 6 } else { count(&|t| map.has_river(t)).min(4) }
                    }
                    StartBias::Lake => count(&|t| terrain(t, "lake")).min(3) * 2,
                    StartBias::Desert => count(&|t| terrain(t, "desert")).min(6),
                    StartBias::Plains => count(&|t| terrain(t, "plains")).min(8) / 2,
                    StartBias::Grassland => count(&|t| terrain(t, "grassland")).min(8) / 2,
                    StartBias::Tundra => count(&|t| terrain(t, "tundra")).min(6),
                    StartBias::Taiga => count(&|t| boreal(t) && feature(t, "forest")).min(6),
                    StartBias::Hills => count(&|t| map.tile(t).elevation == Elevation::Hills).min(6),
                    StartBias::Mountains => count(&|t| map.tile(t).elevation == Elevation::Mountains).min(4) * 2,
                    StartBias::Forest => count(&|t| feature(t, "forest")).min(6),
                    StartBias::Jungle => count(&|t| feature(t, "jungle")).min(6),
                    StartBias::Marsh => count(&|t| feature(t, "marsh")).min(4) * 2,
                };
                hits * weight
            })
            .sum()
    }

    /// How good a start tile is for a player, ignoring their bias: a mix of
    /// food and production (with their civ's tile perks), fresh water or coast,
    /// and for civs without a climate of their own, a temperate climate.
    fn start_quality(&self, player: PlayerId, tile: TileId) -> i32 {
        let map = &self.map;
        let rules = &self.rules;
        let area = map.grid.within(tile, 2);
        // Jungle and marsh are poor until cleared, and an area without enough
        // food can't grow. Short of food in the desert, the start gets oases.
        let oases = self.start_oases(player, tile);
        let oasis = rules.feature_id("oasis");
        let mut food = 0;
        let mut value = 0;
        for &o in &area {
            let mut y = map.yields(rules, o) + self.civ_tile_bonus(player, o);
            if let (Some(f), true) = (oasis, oases.contains(&o)) {
                y += rules.feature(f).yields();
            }
            let poor = map.tile(o).feature.is_some_and(|f| matches!(rules.feature(f).id.as_str(), "jungle" | "marsh"));
            food += y.food;
            value += y.food * 2 + y.production * 2 + y.gold - if poor { 4 } else { 0 };
        }
        let value = value - 4 * (2 * area.len() as i32 - food).max(0);
        let water = map.has_river(tile) || area.iter().any(|&o| map.is_water(rules, o));
        let climate = rules
            .civ(self.players[player as usize].civ)
            .start_bias
            .iter()
            .any(|b| matches!(b, StartBias::Desert | StartBias::Tundra | StartBias::Taiga | StartBias::Jungle));
        let mild = !climate && (8.0..22.0).contains(&map.tile(tile).temperature);
        value + if water { 12 } else { 0 } + if mild { 10 } else { 0 }
    }

    /// The start nearest a civ's homeland: the homeland's own tile or a
    /// neighbour (whichever suits the civ best, nearer counting for more),
    /// looking further out only if none of them will do.
    fn home_start(&self, player: PlayerId, home: TileId, taken: &[Option<TileId>]) -> Option<TileId> {
        let map = &self.map;
        let civ = self.players[player as usize].civ;
        let free = |t: TileId| {
            Game::may_start_at(map, &self.rules, t) && taken.iter().flatten().all(|&o| map.grid.distance(o, t) >= MIN_FIXED_SPACING)
        };
        [1, 2, 3, 5, 8].into_iter().find_map(|radius| {
            map.grid.within(home, radius).into_iter().filter(|&t| free(t)).max_by_key(|&t| {
                let score = self.start_quality(player, t) + 4 * self.start_bias_score(civ, t) - 40 * map.grid.distance(home, t);
                (score, std::cmp::Reverse(t))
            })
        })
    }

    /// Whether a player may be set to start on a tile: land a settler can
    /// walk on, not a mountain.
    pub fn may_start_at(map: &Map, rules: &Rules, tile: TileId) -> bool {
        (tile as usize) < map.tiles.len() && map.tile(tile).elevation != Elevation::Mountains && map.land_move_cost(rules, tile).is_some()
    }

    /// Each player's starting tile (`None` if the map has no room): open,
    /// flat or hilly land on landmasses big enough to expand, well spaced.
    /// Players choose in turn, the one that cares most about its start bias
    /// first; a tile's score is its quality plus the bias, and only tiles
    /// close to the player's best quality count, so no start is crippled.
    /// Players with a `fixed` start keep it, and the others keep their
    /// distance from them.
    fn choose_starts(&self, fixed: &[Option<TileId>]) -> Vec<Option<TileId>> {
        /// How many quality points one point of start bias is worth.
        const BIAS_WEIGHT: i32 = 4;
        /// How far below its best tile's quality a player may start.
        const QUALITY_MARGIN: i32 = 40;
        let map = &self.map;
        let rules = &self.rules;
        let mut chosen: Vec<Option<TileId>> = vec![None; self.players.len()];
        for (p, &tile) in fixed.iter().enumerate().take(self.players.len()) {
            let Some(tile) = tile.filter(|&t| Game::may_start_at(map, rules, t)) else { continue };
            if chosen.iter().flatten().all(|&o| map.grid.distance(o, tile) >= MIN_FIXED_SPACING) {
                chosen[p] = Some(tile);
            }
        }
        if self.options.true_starts && map.kind == crate::shapes::MapType::Earth {
            for p in 0..self.players.len() {
                if chosen[p].is_some() {
                    continue;
                }
                let Some((lon, lat)) = rules.civ(self.players[p].civ).home else { continue };
                chosen[p] = self.home_start(p as PlayerId, crate::earth::tile_at(&map.grid, lon, lat), &chosen);
            }
        }
        let ok_terrain = ["grassland", "plains", "desert", "tundra"];
        let candidates: Vec<TileId> = (0..map.tiles.len() as TileId)
            .filter(|&id| {
                let t = map.tile(id);
                t.elevation != Elevation::Mountains
                    && map.start_land.get(id as usize).copied().unwrap_or(true)
                    // Open ground: no forest, jungle or marsh on the start tile itself.
                    && t.feature.is_none()
                    && ok_terrain.contains(&rules.terrain(t.terrain).id.as_str())
                    && map.grid.within(id, 3).iter().filter(|&&o| !map.is_water(rules, o)).count() >= 25
            })
            .collect();
        // Each player's acceptable tiles with their scores, best first.
        let options: Vec<Vec<(i32, TileId)>> = (0..self.players.len() as PlayerId)
            .map(|p| {
                let civ = self.players[p as usize].civ;
                let quality: Vec<i32> = candidates.iter().map(|&t| self.start_quality(p, t)).collect();
                let floor = quality.iter().max().copied().unwrap_or(0) - QUALITY_MARGIN;
                let mut scored: Vec<(i32, TileId)> = candidates
                    .iter()
                    .zip(&quality)
                    .filter(|&(_, &q)| q >= floor)
                    .map(|(&t, &q)| (q + BIAS_WEIGHT * self.start_bias_score(civ, t), t))
                    .collect();
                scored.sort_by(|a, b| b.cmp(a));
                scored
            })
            .collect();
        // Spread players out, relaxing the spacing if the map is crowded.
        let mut spacing = (map.grid.width.min(map.grid.height * 2) / 3).max(6);
        loop {
            let mut out = chosen.clone();
            let mut picked: Vec<TileId> = chosen.iter().flatten().copied().collect();
            let mut left: Vec<usize> = (0..self.players.len()).filter(|&p| chosen[p].is_none()).collect();
            while !left.is_empty() {
                // How much each player cares: its best open tile over its average.
                let mut best: Option<(i32, usize, TileId)> = None;
                let mut stuck = false;
                for (li, &p) in left.iter().enumerate() {
                    let open: Vec<&(i32, TileId)> =
                        options[p].iter().filter(|(_, t)| picked.iter().all(|&o| map.grid.distance(o, *t) >= spacing)).collect();
                    let Some(&&(top, tile)) = open.first() else {
                        stuck = true;
                        break;
                    };
                    let urgency = top - open.iter().map(|(s, _)| s).sum::<i32>() / open.len() as i32;
                    if best.is_none_or(|(b, _, _)| urgency > b) {
                        best = Some((urgency, li, tile));
                    }
                }
                if stuck {
                    break;
                }
                let (_, li, tile) = best.expect("players left");
                out[left.remove(li)] = Some(tile);
                picked.push(tile);
            }
            if left.is_empty() || spacing <= 2 {
                return out;
            }
            spacing -= 2;
        }
    }

    pub fn unit_def(&self, unit: &Unit) -> &crate::rules::UnitDef {
        self.rules.unit(unit.kind)
    }

    /// Units on a tile.
    pub fn units_at(&self, tile: TileId) -> impl Iterator<Item = &Unit> {
        self.units.values().filter(move |u| u.tile == tile)
    }

    /// Which of a tile's places a unit takes: civilians, land military,
    /// ships and leaders (Generals and Admirals) each have their own, so a
    /// ship can carry (escort) an embarked unit and a General can stand
    /// with the unit it leads.
    pub fn slot(&self, unit: &Unit) -> Slot {
        let def = self.unit_def(unit);
        match def.class {
            UnitClass::Civilian => Slot::Civilian,
            UnitClass::Military if def.has(Ability::General) => Slot::Leader,
            UnitClass::Military if def.domain == Domain::Sea => Slot::Sea,
            UnitClass::Military => Slot::Land,
        }
    }

    /// Whether `unit` may end a move on `tile`: one unit per place (see
    /// [`Self::slot`]), never with another player's units or in their city.
    pub fn can_stop_at(&self, unit: &Unit, tile: TileId) -> bool {
        let slot = self.slot(unit);
        self.city_at(tile).is_none_or(|c| c.owner == unit.owner)
            && self
                .units_at(tile)
                .filter(|o| o.id != unit.id && !self.passes(unit, o))
                .all(|o| o.owner == unit.owner && self.slot(o) != slot)
    }

    /// Whether `a` and `b` ignore each other when moving and stopping:
    /// merchants get past anyone their owner isn't at war with, and anyone
    /// gets past them, so a busy harbour never shuts a city in.
    pub fn passes(&self, a: &Unit, b: &Unit) -> bool {
        a.id != b.id && (self.is_merchant(a) || self.is_merchant(b)) && !self.at_war(a.owner, b.owner)
    }

    /// Whether `player` knows a technology that grants `grant`.
    pub fn grants(&self, player: PlayerId, grant: TechGrant) -> bool {
        let known = &self.players[player as usize].techs;
        self.rules.techs.techs.iter().zip(known).any(|(t, &k)| k && t.grants.contains(&grant))
    }

    /// How `unit` moves, with what its owner's techs and cities allow.
    pub fn mover(&self, unit: &Unit) -> path::Mover {
        let def = self.unit_def(unit);
        let owner = unit.owner;
        let harbors = match def.domain {
            Domain::Sea => self.cities.values().filter(|c| c.owner == owner && self.is_coastal(c.tile)).map(|c| c.tile).collect(),
            Domain::Land => Vec::new(),
        };
        path::Mover {
            embark: self.grants(owner, TechGrant::Embark),
            deep_water: self.grants(owner, TechGrant::DeepWater),
            harbors,
            // Merchants go wherever there's peace.
            closed: if def.has(Ability::Trade) { self.trade_closed_tiles(owner) } else { self.closed_tiles(owner) },
            season: self.season(),
            tunnels: if def.domain == Domain::Land { self.passage_tiles(Passage::Tunnel) } else { Vec::new() },
            bridges: if def.domain == Domain::Land { self.passage_tiles(Passage::Bridge) } else { Vec::new() },
            moves: path::points(def.moves + self.aura_moves(unit)),
            min_step: if def.domain == Domain::Land { self.min_step() } else { path::MOVE },
            ..path::Mover::new(def)
        }
    }

    /// How far the seasons have swung this turn (see `climate::season`).
    pub fn season(&self) -> f32 {
        crate::climate::season(crate::climate::year_phase(self.turn))
    }

    /// The ice on a tile this turn.
    pub fn sea_ice(&self, tile: TileId) -> crate::climate::SeaIce {
        self.map.sea_ice(&self.rules, tile, self.season())
    }

    /// A land unit at sea: it can't attack and defends poorly.
    pub fn is_embarked(&self, unit: &Unit) -> bool {
        self.unit_def(unit).domain == Domain::Land && self.at_sea(unit.tile)
    }

    /// Tiles `unit` can't path through: those with other players' units
    /// (except those it passes, see [`Self::passes`]) or cities.
    pub(crate) fn blocked_for(&self, unit: &Unit) -> std::collections::HashSet<TileId> {
        let owner = unit.owner;
        let units = self.units.values().filter(|o| o.owner != owner && !self.passes(unit, o)).map(|o| o.tile);
        units.chain(self.cities.values().filter(|c| c.owner != owner).map(|c| c.tile)).collect()
    }

    /// Whether `other` exerts a zone of control over `unit`: an enemy
    /// military unit of the same domain (ships hold back ships, land units land units).
    fn controls(&self, unit: &Unit, other: &Unit) -> bool {
        let def = self.unit_def(other);
        self.at_war(unit.owner, other.owner) && def.class == UnitClass::Military && def.domain == self.unit_def(unit).domain
    }

    /// Whether `tile` is in an enemy's zone of control for `unit`: next to
    /// a city of a player it's at war with, or to one of their military units
    /// (see [`Self::controls`]). Military units entering such a tile stop for
    /// the turn, so enemies can't simply walk past each other.
    pub fn in_enemy_zoc(&self, unit: &Unit, tile: TileId) -> bool {
        if self.unit_def(unit).class != UnitClass::Military {
            return false;
        }
        let owner = unit.owner;
        self.map.grid.neighbors(tile).any(|n| {
            self.city_at(n).is_some_and(|c| self.at_war(owner, c.owner)) || self.units_at(n).any(|o| self.controls(unit, o))
        })
    }

    /// Every tile in an enemy's zone of control for `unit` (see [`Self::in_enemy_zoc`]).
    fn enemy_zoc_tiles(&self, unit: &Unit) -> std::collections::HashSet<TileId> {
        let mut out = std::collections::HashSet::new();
        if self.unit_def(unit).class != UnitClass::Military {
            return out;
        }
        let owner = unit.owner;
        let units = self.units.values().filter(|o| self.controls(unit, o)).map(|o| o.tile);
        let cities = self.cities.values().filter(|c| self.at_war(owner, c.owner)).map(|c| c.tile);
        for t in units.chain(cities) {
            out.extend(self.map.grid.neighbors(t));
        }
        out
    }

    /// The path `unit` would take to `to`, if any.
    pub fn path_for(&self, unit: UnitId, to: TileId) -> Option<Path> {
        let u = self.units.get(&unit)?;
        self.path_with(u, to, &self.mover(u))
    }

    /// The path `u` would take to `to` moving as `mover`.
    pub(crate) fn path_with(&self, u: &Unit, to: TileId, mover: &path::Mover) -> Option<Path> {
        if !self.can_stop_at(u, to) {
            return None;
        }
        self.path_through(u, to, mover)
    }

    /// [`Self::path_with`] to a tile `u` may not be able to stop on, like a
    /// foreign city (merchants stop at its gates).
    pub(crate) fn path_through(&self, u: &Unit, to: TileId, mover: &path::Mover) -> Option<Path> {
        let unit = u.id;
        let zoc = self.enemy_zoc_tiles(u);
        let blocked = self.blocked_for(u);
        let owner = u.owner;
        let perk = self.civ_has(owner, |e| matches!(e, crate::civ::Effect::IgnoreTerrainInTerritory));
        let land = mover.domain == Domain::Land;
        let cheap = |from: TileId, to: TileId| if land { self.cheap_step(owner, perk, from, to) } else { None };
        // Our own units of the same kind can be passed but not shared.
        let slot = self.slot(u);
        let crowded: std::collections::HashSet<TileId> =
            self.units.values().filter(|o| o.owner == owner && o.id != unit && self.slot(o) == slot && !self.passes(u, o)).map(|o| o.tile).collect();
        path::find_path_zoc(
            &self.map,
            &self.rules,
            mover,
            u.tile,
            to,
            u.moves,
            |t| t != to && blocked.contains(&t),
            |t| zoc.contains(&t),
            cheap,
            |t| crowded.contains(&t),
        )
    }

    /// For every tile `unit` could get to within `max_turns` extra turns,
    /// the turn it would arrive (as [`Self::path_for`]'s `arrival_turn`),
    /// found in one search: cheaper than a path to each of many tiles.
    pub fn reach_for(&self, unit: UnitId, max_turns: u32) -> std::collections::HashMap<TileId, u32> {
        let Some(u) = self.units.get(&unit) else { return Default::default() };
        let mover = self.mover(u);
        let zoc = self.enemy_zoc_tiles(u);
        let blocked = self.blocked_for(u);
        let owner = u.owner;
        let perk = self.civ_has(owner, |e| matches!(e, crate::civ::Effect::IgnoreTerrainInTerritory));
        let land = mover.domain == Domain::Land;
        let cheap = |from: TileId, to: TileId| if land { self.cheap_step(owner, perk, from, to) } else { None };
        let slot = self.slot(u);
        let crowded: std::collections::HashSet<TileId> =
            self.units.values().filter(|o| o.owner == owner && o.id != unit && self.slot(o) == slot && !self.passes(u, o)).map(|o| o.tile).collect();
        let mut out = path::reachable_zoc(
            &self.map,
            &self.rules,
            &mover,
            u.tile,
            u.moves,
            max_turns,
            |t| blocked.contains(&t),
            |t| zoc.contains(&t),
            cheap,
            |t| crowded.contains(&t),
        );
        out.retain(|&t, _| self.can_stop_at(u, t));
        out
    }

    /// Movement points `unit` (moving as `mover`) needs to step from `from`
    /// onto the neighbouring `to`, and whether that ends its turn (`None`:
    /// impassable).
    pub(crate) fn step_cost(&self, unit: &Unit, mover: &path::Mover, from: TileId, to: TileId) -> Option<(i32, bool)> {
        let (cost, ends) = mover.step(&self.map, &self.rules, from, to)?;
        let perk = self.civ_has(unit.owner, |e| matches!(e, crate::civ::Effect::IgnoreTerrainInTerritory));
        let cheap = if mover.domain == Domain::Land { self.cheap_step(unit.owner, perk, from, to) } else { None };
        Some((cheap.map_or(cost, |c| c.min(cost)), ends))
    }

    /// What a land unit of `owner`'s pays to step from `from` to `to` if a
    /// road (or `perk`, ignoring terrain in their land) makes it cheap.
    fn cheap_step(&self, owner: PlayerId, perk: bool, from: TileId, to: TileId) -> Option<i32> {
        if self.has_rail(from) && self.has_rail(to) {
            return Some(path::RAIL_COST);
        }
        ((perk && self.territory_owner(to) == Some(owner)) || self.road_between(from, to)).then_some(path::MOVE)
    }

    /// Whether a road (cities have one) links two neighbouring tiles.
    fn road_between(&self, from: TileId, to: TileId) -> bool {
        self.works[from as usize].road && self.works[to as usize].road
    }

    /// The least a land unit's step can cost: a railway's once anyone can
    /// lay them, else a whole point.
    pub fn min_step(&self) -> i32 {
        let rail = self.rules.rail_tech() as usize;
        if self.players.iter().any(|p| p.techs[rail]) { path::RAIL_COST } else { path::MOVE }
    }

    /// Movement a unit gets at the start of a turn (in [`path::MOVE`]s).
    pub fn turn_moves(&self, unit: &Unit) -> i32 {
        let bonus: i32 = if self.territory_owner(unit.tile) == Some(unit.owner) {
            self.civ_effects(unit.owner).map(|e| if let crate::civ::Effect::MovesInTerritory(n) = e { *n } else { 0 }).sum()
        } else {
            0
        };
        path::points(self.unit_def(unit).moves + bonus + self.aura_moves(unit))
    }

    /// Units of `player` waiting for orders this turn, including sleeping and
    /// fortified ones under threat. Units with a standing order (Goto, Take,
    /// Defend, Hunt...) don't ask until it's done.
    pub fn units_needing_orders(&self, player: PlayerId) -> impl Iterator<Item = &Unit> {
        self.units.values().filter(move |u| u.owner == player && (u.order == Order::None || self.is_alerted(u)) && u.moves > 0)
    }

    /// Whether `unit` sleeps or is fortified but asks for orders this turn
    /// because an enemy threatens it.
    pub fn is_alerted(&self, unit: &Unit) -> bool {
        unit.alerted == Some(self.turn) && matches!(unit.order, Order::Sleep | Order::Fortify)
    }

    pub fn apply(&mut self, player: PlayerId, cmd: Command) -> Result<Vec<Event>, CommandError> {
        let unit = match cmd {
            Command::MoveUnit { unit, .. }
            | Command::Attack { unit, .. }
            | Command::Hunt { unit, .. }
            | Command::SetOrder { unit, .. }
            | Command::Build { unit, .. }
            | Command::Upgrade { unit }
            | Command::Trade { unit, .. }
            | Command::Transfer { unit, .. }
            | Command::RepeatRoute { unit, .. } => Some(unit),
            _ => None,
        };
        let events = self.run_command(player, cmd)?;
        // Any order the owner gives answers an alert.
        if let Some(u) = unit.and_then(|id| self.units.get_mut(&id)) {
            u.alerted = None;
        }
        Ok(events)
    }

    fn run_command(&mut self, player: PlayerId, cmd: Command) -> Result<Vec<Event>, CommandError> {
        match cmd {
            Command::MoveUnit { unit, to } => {
                self.check_owner(player, unit)?;
                self.check_not_merchant(unit)?;
                if self.path_for(unit, to).is_none() {
                    return Err(CommandError::Unreachable);
                }
                let mut events = self.set_order(unit, Order::Goto(to));
                match self.advance(unit, to) {
                    Ok(e) => events.extend(e),
                    // Out of moves: it sets off next turn.
                    Err(CommandError::NoMovesLeft) => {}
                    Err(e) => return Err(e),
                }
                Ok(events)
            }
            Command::Attack { unit, tile } => {
                self.check_owner(player, unit)?;
                let mut events = self.attack(unit, tile)?;
                events.extend(self.check_game_over());
                Ok(events)
            }
            Command::Hunt { unit, target } => {
                self.check_owner(player, unit)?;
                self.check_not_merchant(unit)?;
                let mut events = self.hunt(unit, target)?;
                events.extend(self.check_game_over());
                Ok(events)
            }
            Command::SetOrder { unit, order } => {
                self.check_owner(player, unit)?;
                if !matches!(order, Order::None | Order::Sleep | Order::Skip | Order::Trade(_)) {
                    self.check_not_merchant(unit)?;
                }
                match order {
                    Order::Build(work) | Order::AutoBuild(work) => self.start_work(unit, work),
                    Order::Trade(route) => self.set_route(unit, route.to),
                    Order::Take { city } => self.take_order(unit, city),
                    Order::Defend { city } => self.defend_order(unit, city),
                    Order::Automate if !self.unit_def(&self.units[&unit]).has(Ability::BuildImprovements) => {
                        Err(CommandError::CantBuild("only Workers improve land on their own"))
                    }
                    _ => {
                        // An alerted unit told to skip, sleep or fortify again
                        // goes back to what it was doing; the event tells the
                        // client it was dealt with.
                        let u = &self.units[&unit];
                        let alerted = self.is_alerted(u);
                        let order = if alerted && order == Order::Skip { u.order } else { order };
                        let mut events = self.set_order(unit, order);
                        if alerted && events.is_empty() {
                            events.push(Event::OrderChanged { unit, order });
                        }
                        Ok(events)
                    }
                }
            }
            Command::FoundCity { unit } => {
                self.check_owner(player, unit)?;
                let u = &self.units[&unit];
                if !self.unit_def(u).has(Ability::FoundCity) {
                    return Err(CommandError::CantFoundCity("this unit can't found cities"));
                }
                if u.moves <= 0 {
                    return Err(CommandError::NoMovesLeft);
                }
                if let Some(why) = self.found_city_problem(player, u.tile) {
                    return Err(CommandError::CantFoundCity(why));
                }
                Ok(self.found_city(unit))
            }
            Command::Build { unit, work } => {
                self.check_owner(player, unit)?;
                self.start_work(unit, work)
            }
            Command::SetQueue { city, queue } => {
                self.check_city(player, city)?;
                // Checked with the new queue in place, so a canal can follow
                // the one before it towards the water.
                let old = std::mem::replace(&mut self.cities.get_mut(&city).expect("checked").queue, queue.clone());
                let c = &self.cities[&city];
                let bad = queue.iter().enumerate().any(|(i, &b)| {
                    let built = match b {
                        Build::Building(id) => c.has(id) || queue[..i].contains(&b),
                        Build::Passage(..) => queue[..i].contains(&b),
                        _ => false,
                    };
                    // A project never finishes, so it can only come last.
                    let stuck = matches!(b, Build::Project(_)) && i + 1 < queue.len();
                    built
                        || stuck
                        || !self.has_tech_for(player, b)
                        || !self.civ_builds(player, b)
                        || !self.builds_here(city, b)
                        || !self.has_resources_for(player, b)
                });
                if bad {
                    self.cities.get_mut(&city).expect("checked").queue = old;
                    return Err(CommandError::NotAvailable);
                }
                Ok(vec![Event::CityChanged { city }])
            }
            Command::SetFocus { city, focus } => {
                self.check_city(player, city)?;
                self.cities.get_mut(&city).expect("checked").focus = focus;
                self.assign_worked_tiles(city);
                Ok(vec![Event::CityChanged { city }])
            }
            Command::SetAutoQueue { city, on } => {
                self.check_city(player, city)?;
                self.cities.get_mut(&city).expect("checked").auto_queue = on;
                Ok(vec![Event::CityChanged { city }])
            }
            Command::Buy { city, build } => {
                self.check_city(player, city)?;
                if let Some(why) = self.buy_problem(city, build) {
                    return Err(CommandError::CantBuy(why));
                }
                Ok(self.buy(city, build))
            }
            Command::ChooseTrait { unit, trait_id } => {
                self.check_owner(player, unit)?;
                self.choose_trait(unit, trait_id)
            }
            Command::Upgrade { unit } => {
                self.check_owner(player, unit)?;
                if let Some(why) = self.upgrade_problem(unit) {
                    return Err(CommandError::CantBuy(why));
                }
                Ok(self.upgrade(unit))
            }
            Command::BuyTile { city, tile } => {
                self.check_city(player, city)?;
                if let Some(why) = self.tile_problem(city, tile) {
                    return Err(CommandError::CantBuy(why));
                }
                Ok(self.buy_tile(city, tile))
            }
            Command::Trade { unit, to } => {
                self.check_owner(player, unit)?;
                self.set_route(unit, to)
            }
            Command::RepeatRoute { unit, repeat } => {
                self.check_owner(player, unit)?;
                self.set_repeat(unit, repeat)
            }
            Command::Transfer { unit, city } => {
                self.check_owner(player, unit)?;
                self.transfer(unit, city)
            }
            Command::RazeCity { city } => {
                self.check_city(player, city)?;
                if let Some(why) = self.raze_problem(city) {
                    return Err(CommandError::CantRaze(why));
                }
                Ok(self.raze(city))
            }
            Command::SetResearch { tech } => self.set_research(player, tech),
            Command::DeclareWar { on } => self.declare_war(player, on),
            Command::ProposeDeal { to, deal } => self.propose_deal(player, to, deal),
            Command::AcceptDeal { from } => self.accept_deal(player, from),
            Command::RefuseDeal { from } => self.refuse_deal(player, from),
            Command::EndTurn => {
                let mut events = self.end_turn();
                events.extend(self.check_game_over());
                Ok(events)
            }
        }
    }

    fn check_owner(&self, player: PlayerId, unit: UnitId) -> Result<(), CommandError> {
        match self.units.get(&unit) {
            None => Err(CommandError::NoSuchUnit),
            Some(u) if u.owner != player => Err(CommandError::NotYourUnit),
            Some(_) => Ok(()),
        }
    }

    /// Merchants go only where their trade route takes them: they can be
    /// given a route (`Command::Trade`), told to wait or skip a turn, but
    /// not moved.
    fn check_not_merchant(&self, unit: UnitId) -> Result<(), CommandError> {
        match self.units.get(&unit) {
            Some(u) if self.is_merchant(u) => Err(CommandError::CantTrade("merchants travel on their own: give it a trade route")),
            _ => Ok(()),
        }
    }

    fn check_city(&self, player: PlayerId, city: CityId) -> Result<(), CommandError> {
        match self.cities.get(&city) {
            None => Err(CommandError::NoSuchCity),
            Some(c) if c.owner != player => Err(CommandError::NotYourCity),
            Some(_) => Ok(()),
        }
    }

    /// Sets the order of `unit`.
    pub(crate) fn set_order(&mut self, unit: UnitId, order: Order) -> Vec<Event> {
        let u = self.units.get_mut(&unit).expect("checked by caller");
        if u.order == order {
            return Vec::new();
        }
        u.order = order;
        vec![Event::OrderChanged { unit, order }]
    }

    /// Why `unit` can't be sent to take `city`, if it can't: only soldiers
    /// and leaders go, against a city of a player we've met.
    pub fn take_problem(&self, unit: UnitId, city: CityId) -> Option<&'static str> {
        let u = self.units.get(&unit)?;
        let Some(c) = self.cities.get(&city) else { return Some("there's no such city") };
        if self.unit_def(u).class != UnitClass::Military {
            return Some("only military units take cities");
        }
        if c.owner == u.owner {
            return Some("it's your own city");
        }
        if !self.players[u.owner as usize].explored[c.tile as usize] {
            return Some("you haven't found that city");
        }
        if !self.has_met(u.owner, c.owner) {
            return Some("you haven't met them");
        }
        None
    }

    /// `Order::Take`: checked, then carried out by the automation in the `ai` crate.
    fn take_order(&mut self, unit: UnitId, city: CityId) -> Result<Vec<Event>, CommandError> {
        if let Some(why) = self.take_problem(unit, city) {
            return Err(CommandError::CantOrder(why));
        }
        Ok(self.set_order(unit, Order::Take { city }))
    }

    /// Why `unit` can't be sent to defend `city`, if it can't.
    pub fn defend_problem(&self, unit: UnitId, city: CityId) -> Option<&'static str> {
        let u = self.units.get(&unit)?;
        let Some(c) = self.cities.get(&city) else { return Some("there's no such city") };
        if self.unit_def(u).class != UnitClass::Military {
            return Some("only military units defend cities");
        }
        if c.owner != u.owner {
            return Some("only your own cities");
        }
        None
    }

    /// `Order::Defend`: checked, then carried out by the automation in the `ai` crate.
    fn defend_order(&mut self, unit: UnitId, city: CityId) -> Result<Vec<Event>, CommandError> {
        if let Some(why) = self.defend_problem(unit, city) {
            return Err(CommandError::CantOrder(why));
        }
        Ok(self.set_order(unit, Order::Defend { city }))
    }

    /// Moves `unit` along its best path towards `to` as far as this turn's
    /// movement allows, keeping its current order. Used for Goto orders and by
    /// automation (which keeps the Explore order).
    pub fn advance(&mut self, unit: UnitId, to: TileId) -> Result<Vec<Event>, CommandError> {
        let u = self.units.get(&unit).ok_or(CommandError::NoSuchUnit)?;
        if u.moves <= 0 {
            return Err(CommandError::NoMovesLeft);
        }
        let path = self.path_for(unit, to).ok_or(CommandError::Unreachable)?;
        let mut events = self.walk(unit, &path, true);
        // Arrived: a Goto order is done.
        if self.units[&unit].tile == to && self.units[&unit].order == Order::Goto(to) {
            events.extend(self.set_order(unit, Order::None));
        }
        Ok(events)
    }

    /// Moves `unit` along `path` as far as this turn's movement allows. With
    /// `see` false, the caller updates its owner's sight afterwards.
    pub(crate) fn walk(&mut self, unit: UnitId, path: &Path, see: bool) -> Vec<Event> {
        let u = &self.units[&unit];
        let owner = u.owner;
        let mut moves = u.moves;
        let mut walked = Vec::new();
        let mut at = u.tile;
        let mover = self.mover(u);
        for (i, &tile) in path.tiles.iter().enumerate() {
            if path.turns[i] > 0 || moves <= 0 {
                break;
            }
            let Some((cost, switch)) = self.step_cost(&self.units[&unit], &mover, at, tile) else { break };
            // Embarking and landing take the rest of the turn.
            let cost = if switch { moves } else { cost };
            // Don't end the turn on a tile we can't stop on (e.g. our own unit of
            // the same kind); stop just before it instead.
            let ends_here = moves - cost <= 0 || i + 1 == path.tiles.len() || path.turns[i + 1] > 0;
            let probe = Unit { tile: at, ..self.units[&unit].clone() };
            if ends_here && !self.can_stop_at(&probe, tile) {
                break;
            }
            moves -= cost;
            if self.in_enemy_zoc(&probe, tile) {
                moves = 0;
            }
            walked.push(tile);
            at = tile;
        }
        let mut events = Vec::new();
        let u = self.units.get_mut(&unit).expect("exists");
        u.moves = moves.max(0);
        u.tile = at;
        u.moved |= !walked.is_empty();
        if !walked.is_empty() {
            events.push(Event::UnitMoved { unit, path: walked });
            if see {
                events.extend(self.update_visibility(owner));
            }
        }
        events
    }

    fn end_turn(&mut self) -> Vec<Event> {
        self.turn += 1;
        let mut events = vec![Event::TurnStarted { turn: self.turn }];
        events.extend(self.auto_traits());
        let cities: Vec<CityId> = self.cities.keys().copied().collect();
        for city in cities {
            events.extend(self.process_city(city));
        }
        events.extend(self.pull_down_posts());
        for player in 0..self.players.len() as PlayerId {
            events.extend(self.process_research(player));
        }
        events.extend(self.process_upkeep());
        self.process_payments();
        self.process_resource_deals();
        events.extend(self.process_works());
        self.expire_proposals();
        events.extend(self.expel_trespassers());
        events.extend(self.city_strikes());
        // Others may have walked into view without anyone recomputing sight.
        for player in 0..self.players.len() as PlayerId {
            events.extend(self.meet_visible(player));
        }
        self.heal();
        let ids: Vec<UnitId> = self.units.keys().copied().collect();
        for &id in &ids {
            let moves = self.turn_moves(&self.units[&id]);
            let u = self.units.get_mut(&id).expect("exists");
            u.moves = moves;
            u.attacks = 0;
            u.moved = false;
            if u.order == Order::Skip {
                u.order = Order::None;
            }
        }
        // Units with Goto orders continue on their way and hunters chase their
        // prey (new units have no orders). A hunt may kill units on the list.
        for &id in &ids {
            match self.units.get(&id).map(|u| u.order) {
                Some(Order::Goto(to)) => match self.advance(id, to) {
                    Ok(e) => events.extend(e),
                    // Path blocked or gone: ask the player.
                    Err(_) => events.extend(self.set_order(id, Order::None)),
                },
                Some(Order::Hunt { .. }) => events.extend(self.pursue(id)),
                _ => {}
            }
        }
        self.alert_threatened();
        // Merchants travel on, and their owners look around once at the end.
        let mut traveled = vec![false; self.players.len()];
        for &id in &ids {
            if let Some(&Unit { order: Order::Trade(_), owner, .. }) = self.units.get(&id) {
                events.extend(self.run_trade(id));
                traveled[owner as usize] |= self.units[&id].moved;
            }
        }
        for (p, _) in traveled.iter().enumerate().filter(|(_, &t)| t) {
            events.extend(self.update_visibility(p as PlayerId));
        }
        events
    }

    /// Recomputes what `player` can see; returns events for newly explored tiles.
    pub fn update_visibility(&mut self, player: PlayerId) -> Vec<Event> {
        let mut visible = vec![0u16; self.map.tiles.len()];
        for u in self.units.values().filter(|u| u.owner == player) {
            let mut sight = self.unit_sight(u);
            // High ground sees further.
            if self.map.tile(u.tile).elevation != Elevation::Flat {
                sight += 1;
            }
            for t in self.map.grid.within(u.tile, sight) {
                visible[t as usize] += 1;
            }
        }
        // Forts and watchtowers see around them.
        for (t, w) in self.works.iter().enumerate() {
            let sight = if w.watchtower { self.rules.improvements.watchtower.sight } else if w.fort { self.rules.improvements.fort.sight } else { continue };
            if self.territory[t] == Some(player) {
                let high = self.map.tile(t as TileId).elevation != Elevation::Flat;
                for x in self.map.grid.within(t as TileId, sight + high as i32) {
                    visible[x as usize] += 1;
                }
            }
        }
        // Cities see their territory and a little beyond.
        for c in self.cities.values().filter(|c| c.owner == player) {
            for t in self.map.grid.within(c.tile, 2) {
                visible[t as usize] += 1;
            }
        }
        for (v, owner) in visible.iter_mut().zip(&self.territory) {
            if *owner == Some(player) {
                *v += 1;
            }
        }
        let p = &mut self.players[player as usize];
        let mut newly = Vec::new();
        for (i, &v) in visible.iter().enumerate() {
            if v > 0 && !p.explored[i] {
                p.explored[i] = true;
                newly.push(i as TileId);
            }
        }
        let changed = p.visible != visible;
        p.visible = visible;
        let mut events = Vec::new();
        if !newly.is_empty() {
            events.push(Event::Explored { player, tiles: newly });
        }
        if changed {
            events.push(Event::VisibilityChanged { player });
        }
        events.extend(self.meet_visible(player));
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapgen::{generate, MapSettings};

    fn new_game() -> Game {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        Game::new(rules, map, 3)
    }

    #[test]
    fn chosen_starts_are_kept_and_others_picked() {
        let rules = Rules::load_default().unwrap();
        let map = generate(&rules, &MapSettings::default());
        let settler_at = |g: &Game, p: PlayerId| {
            g.units.values().find(|u| u.owner == p && g.rules.unit(u.kind).id == "settler").map(|u| u.tile)
        };
        // Any walkable land, even where the game wouldn't start anyone.
        let land: Vec<TileId> = (0..map.tiles.len() as TileId).filter(|&t| Game::may_start_at(&map, &rules, t)).collect();
        let (mine, theirs) = (land[land.len() / 3], land[land.len() * 2 / 3]);
        assert!(map.grid.distance(mine, theirs) >= MIN_FIXED_SPACING);
        let water = (0..map.tiles.len() as TileId).find(|&t| map.is_water(&rules, t)).unwrap();
        let mut setup = GameSetup::first(&rules, 4);
        setup.starts = vec![Some(mine), None, Some(theirs), Some(water)];
        let g = Game::with_setup(rules, map, &setup);
        assert_eq!(settler_at(&g, 0), Some(mine));
        assert_eq!(settler_at(&g, 2), Some(theirs));
        // The others (and the one set on water) start elsewhere, apart from them.
        for p in [1, 3] {
            let t = settler_at(&g, p).expect("a start");
            assert!(!g.map.is_water(&g.rules, t));
            assert!(g.map.grid.distance(t, mine) >= MIN_FIXED_SPACING && g.map.grid.distance(t, theirs) >= MIN_FIXED_SPACING);
        }
    }

    #[test]
    fn players_start_with_units_on_good_land() {
        let game = new_game();
        assert_eq!(game.players.len(), 4);
        for p in 0..4 {
            let units: Vec<_> = game.units.values().filter(|u| u.owner == p).collect();
            assert_eq!(units.len(), 3, "player {p}");
            for u in &units {
                assert!(game.map.land_move_cost(&game.rules, u.tile).is_some());
            }
            assert!(game.players[p as usize].explored.iter().any(|&e| e));
        }
        // Start positions are spread out.
        let settlers: Vec<TileId> = game
            .units
            .values()
            .filter(|u| game.rules.unit(u.kind).id == "settler")
            .map(|u| u.tile)
            .collect();
        for (i, &a) in settlers.iter().enumerate() {
            for &b in &settlers[i + 1..] {
                assert!(game.map.grid.distance(a, b) >= 6, "starts too close");
            }
        }
    }

    fn unit_of(game: &Game, player: PlayerId, kind: &str) -> UnitId {
        game.units.values().find(|u| u.owner == player && game.rules.unit(u.kind).id == kind).unwrap().id
    }

    /// A reachable land tile `dist` steps from the unit.
    fn target(game: &Game, unit: UnitId, dist: i32) -> TileId {
        let u = &game.units[&unit];
        (0..game.map.tiles.len() as TileId)
            .filter(|&t| game.map.grid.distance(u.tile, t) == dist)
            .find(|&t| game.path_for(unit, t).is_some())
            .expect("some reachable tile")
    }

    #[test]
    fn reach_agrees_with_paths() {
        let mut game = new_game();
        let warrior = unit_of(&game, 0, "warrior");
        // Some roads and railways about, so cheap steps count too.
        let home = game.units[&warrior].tile;
        for (i, t) in game.map.grid.within(home, 3).into_iter().enumerate() {
            game.works[t as usize].road = i % 2 == 0;
            game.works[t as usize].rail = i % 4 == 0;
        }
        let rail = game.rules.rail_tech() as usize;
        game.players[0].techs[rail] = true;
        let reach = game.reach_for(warrior, 3);
        assert!(reach.len() > 10);
        for t in game.map.grid.within(home, 6) {
            let path = game.path_for(warrior, t).map(|p| p.arrival_turn());
            match reach.get(&t) {
                Some(&turns) => assert_eq!(Some(turns), path, "tile {t}"),
                None => assert!(t == home || path.is_none_or(|p| p > 3), "tile {t}: {path:?}"),
            }
        }
    }

    #[test]
    fn goto_continues_over_turns_and_completes() {
        let mut game = new_game();
        let warrior = unit_of(&game, 0, "warrior");
        let dest = target(&game, warrior, 6);
        let turns = game.path_for(warrior, dest).unwrap().arrival_turn();
        game.apply(0, Command::MoveUnit { unit: warrior, to: dest }).unwrap();
        for _ in 0..turns {
            assert_eq!(game.units[&warrior].order, Order::Goto(dest));
            game.apply(0, Command::EndTurn).unwrap();
        }
        assert_eq!(game.units[&warrior].tile, dest);
        assert_eq!(game.units[&warrior].order, Order::None);
    }

    #[test]
    fn units_out_of_moves_can_still_be_sent_off() {
        let mut game = new_game();
        let warrior = unit_of(&game, 0, "warrior");
        let dest = target(&game, warrior, 4);
        game.units.get_mut(&warrior).unwrap().moves = 0;
        game.apply(0, Command::MoveUnit { unit: warrior, to: dest }).unwrap();
        assert_eq!(game.units[&warrior].order, Order::Goto(dest));
        game.apply(0, Command::EndTurn).unwrap();
        assert!(game.map.grid.distance(game.units[&warrior].tile, dest) < 4, "set off next turn");
    }

    #[test]
    fn moving_reveals_tiles_and_spends_moves() {
        let mut game = new_game();
        let scout = unit_of(&game, 0, "scout");
        let before = game.players[0].explored.iter().filter(|&&e| e).count();
        let dest = target(&game, scout, 3);
        let events = game.apply(0, Command::MoveUnit { unit: scout, to: dest }).unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::UnitMoved { .. })));
        let after = game.players[0].explored.iter().filter(|&&e| e).count();
        assert!(after > before);
        assert!(game.units[&scout].moves < path::points(game.rules.unit(game.units[&scout].kind).moves));
    }

    #[test]
    fn players_meet_when_they_see_each_other() {
        let mut game = new_game();
        assert!(!game.has_met(0, 1));
        let theirs = unit_of(&game, 1, "scout");
        let ours = game.units[&unit_of(&game, 0, "warrior")].tile;
        game.units.get_mut(&theirs).unwrap().tile = game.map.grid.neighbors(ours).next().unwrap();
        let events = game.update_visibility(1);
        assert!(events.contains(&Event::Met { player: 1, other: 0 }));
        assert!(game.has_met(0, 1) && game.has_met(1, 0), "meeting goes both ways");
        assert!(!game.has_met(0, 2));
        assert!(!game.update_visibility(0).iter().any(|e| matches!(e, Event::Met { .. })), "only once");
    }

    #[test]
    fn cannot_command_other_players_units() {
        let mut game = new_game();
        let theirs = unit_of(&game, 1, "warrior");
        let err = game.apply(0, Command::SetOrder { unit: theirs, order: Order::Fortify }).unwrap_err();
        assert_eq!(err, CommandError::NotYourUnit);
    }

    #[test]
    fn stacking_one_military_and_one_civilian() {
        let game = new_game();
        let warrior = &game.units[&unit_of(&game, 0, "warrior")];
        let settler = &game.units[&unit_of(&game, 0, "settler")];
        let scout = &game.units[&unit_of(&game, 0, "scout")];
        assert_eq!(warrior.tile, settler.tile);
        // The scout (military) can't stop where the warrior is.
        assert!(!game.can_stop_at(scout, warrior.tile));
    }
}
