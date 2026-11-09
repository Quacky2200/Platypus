// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Steam`: the Steamworks features a game can use: the app id (so Steam shows the game as running),
//! rich presence, achievements and stats, and matchmaking lobbies. The namespace talks to a
//! [`Backend`] the host installs per thread (`platypus_steam` gives the real one over the
//! `steamworks` crate); with none installed every call is a harmless no-op (`Steam.isAvailable()`
//! is false), so a game runs the same without Steam. [`MemoryBackend`] is an in-process stand-in for
//! tests. Lobby and search calls only start the work; the script reads the state later
//! (`lobbyStatus`, `searchStatus`), and the host calls [`pump`] each frame.
//!
//! Steam ids (users, lobbies) are 64-bit, which a script number can't hold, so scripts get them as
//! decimal strings. Lists (`lobby`, `lobbies`) come as JSON text.

use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use ts_api::ts_api;

/// Who can find and join a lobby.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyKind {
    Public,
    FriendsOnly,
    Private,
    Invisible,
}

impl LobbyKind {
    /// `"public"`, `"friends"`, `"private"` or `"invisible"` (anything else: public).
    pub fn parse(text: &str) -> LobbyKind {
        match text {
            "friends" => LobbyKind::FriendsOnly,
            "private" => LobbyKind::Private,
            "invisible" => LobbyKind::Invisible,
            _ => LobbyKind::Public,
        }
    }
}

/// Where the lobby the player is in (or is entering) stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LobbyStatus {
    #[default]
    None,
    Creating,
    Joining,
    In,
    Failed,
}

impl LobbyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            LobbyStatus::None => "none",
            LobbyStatus::Creating => "creating",
            LobbyStatus::Joining => "joining",
            LobbyStatus::In => "in",
            LobbyStatus::Failed => "failed",
        }
    }
}

/// Where a lobby search stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchStatus {
    #[default]
    Idle,
    Searching,
    Done,
    Failed,
}

impl SearchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchStatus::Idle => "idle",
            SearchStatus::Searching => "searching",
            SearchStatus::Done => "done",
            SearchStatus::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Member {
    pub id: u64,
    pub name: String,
}

/// The lobby the player is in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LobbyState {
    pub status: LobbyStatus,
    pub id: u64,
    pub owner: u64,
    pub max_members: u32,
    pub members: Vec<Member>,
    pub data: BTreeMap<String, String>,
    /// Why the status is `Failed` (Steam's reason), else empty.
    pub error: String,
}

/// One search result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LobbyInfo {
    pub id: u64,
    pub owner: u64,
    pub members: u32,
    pub max_members: u32,
    pub data: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchState {
    pub status: SearchStatus,
    pub results: Vec<LobbyInfo>,
}

/// What the host provides: Steam itself, or a stand-in.
pub trait Backend {
    fn available(&self) -> bool;
    fn app_id(&self) -> u32;
    fn user_name(&self) -> String;
    fn user_id(&self) -> u64;

    /// A rich presence key (`status`, `steam_display`, `connect`, ...); `None` clears the key.
    fn set_rich_presence(&mut self, key: &str, value: Option<&str>) -> bool;
    fn clear_rich_presence(&mut self);

    fn achievement(&mut self, name: &str) -> Option<bool>;
    fn set_achievement(&mut self, name: &str, unlocked: bool) -> bool;
    fn stat(&mut self, name: &str) -> Option<f64>;
    fn set_stat(&mut self, name: &str, value: f64) -> bool;
    /// Sends changed stats and achievements to Steam (unlocking pops Steam's own toast).
    fn store_stats(&mut self) -> bool;

    fn create_lobby(&mut self, kind: LobbyKind, max_members: u32);
    /// Starts a search; every filter is `(key, value)` and must match the lobby's data.
    fn find_lobbies(&mut self, filters: &[(String, String)]);
    fn join_lobby(&mut self, id: u64);
    fn leave_lobby(&mut self);
    fn set_lobby_data(&mut self, key: &str, value: &str) -> bool;
    fn set_lobby_joinable(&mut self, joinable: bool) -> bool;
    fn lobby(&self) -> LobbyState;
    fn search(&self) -> SearchState;

    /// Runs the backend's callbacks and moves finished requests into the state (each frame).
    fn pump(&mut self);
}

thread_local! {
    static BACKEND: RefCell<Option<Box<dyn Backend>>> = const { RefCell::new(None) };
    static FILTERS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// Installs the backend scripts talk to (the host, at start); `None` removes it.
pub fn install(backend: Option<Box<dyn Backend>>) {
    BACKEND.with(|b| *b.borrow_mut() = backend);
    FILTERS.with(|f| f.borrow_mut().clear());
}

/// Runs the backend's callbacks; the host calls this once per frame.
pub fn pump() {
    with(|b| b.pump());
}

fn with<R>(f: impl FnOnce(&mut dyn Backend) -> R) -> Option<R> {
    BACKEND.with(|b| b.borrow_mut().as_mut().map(|b| f(b.as_mut())))
}

fn id_text(id: u64) -> String {
    if id == 0 {
        String::new()
    } else {
        id.to_string()
    }
}

fn data_json(data: &BTreeMap<String, String>) -> Value {
    Value::Object(data.iter().map(|(k, v)| (k.clone(), json!(v))).collect())
}

/// The lobby as the JSON a script reads (ids as strings).
pub fn lobby_json(lobby: &LobbyState) -> String {
    json!({
        "status": lobby.status.as_str(),
        "id": id_text(lobby.id),
        "owner": id_text(lobby.owner),
        "maxMembers": lobby.max_members,
        "members": lobby.members.iter().map(|m| json!({"id": id_text(m.id), "name": m.name})).collect::<Vec<_>>(),
        "data": data_json(&lobby.data),
        "error": lobby.error,
    })
    .to_string()
}

pub fn lobbies_json(results: &[LobbyInfo]) -> String {
    Value::Array(
        results
            .iter()
            .map(|l| {
                json!({
                    "id": id_text(l.id),
                    "owner": id_text(l.owner),
                    "members": l.members,
                    "maxMembers": l.max_members,
                    "data": data_json(&l.data),
                })
            })
            .collect(),
    )
    .to_string()
}

#[ts_api(namespace = "Steam")]
pub mod steam_api {
    use super::{FILTERS, LobbyKind, id_text, lobbies_json, lobby_json, with};

    /// True when Steam is running and the game is connected to it.
    pub fn is_available() -> bool {
        with(|b| b.available()).unwrap_or(false)
    }

    /// The Steam app id the game runs as.
    pub fn app_id() -> f64 {
        with(|b| b.app_id()).unwrap_or(0) as f64
    }

    /// The player's Steam name ("" without Steam).
    pub fn user_name() -> String {
        with(|b| b.user_name()).unwrap_or_default()
    }

    /// The player's 64-bit Steam id as a decimal string ("" without Steam).
    pub fn user_id() -> String {
        id_text(with(|b| b.user_id()).unwrap_or(0))
    }

    /// Sets a rich presence key (`status`, `steam_display`, `connect`, ...), shown in friends lists.
    pub fn set_rich_presence(key: &str, value: &str) -> bool {
        with(|b| b.set_rich_presence(key, Some(value))).unwrap_or(false)
    }

    pub fn clear_rich_presence() {
        with(|b| b.clear_rich_presence());
    }

    /// Unlocks an achievement and stores it, so Steam shows its popup.
    pub fn unlock_achievement(name: &str) -> bool {
        with(|b| b.set_achievement(name, true) && b.store_stats()).unwrap_or(false)
    }

    /// Locks an achievement again (for testing).
    pub fn clear_achievement(name: &str) -> bool {
        with(|b| b.set_achievement(name, false) && b.store_stats()).unwrap_or(false)
    }

    pub fn is_achievement_unlocked(name: &str) -> bool {
        with(|b| b.achievement(name)).flatten().unwrap_or(false)
    }

    /// Sets a stat (not stored until `storeStats`, or an unlock).
    pub fn set_stat(name: &str, value: f64) -> bool {
        with(|b| b.set_stat(name, value)).unwrap_or(false)
    }

    /// A stat's value (0 when it isn't known).
    pub fn get_stat(name: &str) -> f64 {
        with(|b| b.stat(name)).flatten().unwrap_or(0.0)
    }

    pub fn store_stats() -> bool {
        with(|b| b.store_stats()).unwrap_or(false)
    }

    /// Starts creating a lobby: `visibility` is `"public"`, `"friends"`, `"private"` or `"invisible"`.
    /// Read `lobbyStatus()` for the result.
    pub fn create_lobby(visibility: &str, max_members: f64) {
        let max = if max_members.is_nan() {
            4.0
        } else {
            max_members.clamp(1.0, 250.0)
        };
        with(|b| b.create_lobby(LobbyKind::parse(visibility), max as u32));
    }

    /// Adds a filter for the next `findLobbies`: the lobby's data `key` must equal `value`.
    pub fn add_lobby_filter(key: &str, value: &str) {
        FILTERS.with(|f| f.borrow_mut().push((key.to_string(), value.to_string())));
    }

    /// Starts a search with the filters added since the last one; read `searchStatus()` and `lobbies()`.
    pub fn find_lobbies() {
        let filters = FILTERS.with(|f| std::mem::take(&mut *f.borrow_mut()));
        with(|b| b.find_lobbies(&filters));
    }

    /// Starts joining a lobby by the id string `lobbies()` gave.
    pub fn join_lobby(id: &str) {
        if let Ok(id) = id.parse::<u64>() {
            with(|b| b.join_lobby(id));
        }
    }

    pub fn leave_lobby() {
        with(|b| b.leave_lobby());
    }

    /// `"none"`, `"creating"`, `"joining"`, `"in"` or `"failed"`.
    pub fn lobby_status() -> String {
        with(|b| b.lobby().status.as_str().to_string()).unwrap_or_else(|| "none".to_string())
    }

    /// The current lobby as JSON: `{status, id, owner, maxMembers, members: [{id, name}], data, error}`
    /// (`error` is Steam's reason when the status is `"failed"`).
    pub fn lobby() -> String {
        with(|b| lobby_json(&b.lobby())).unwrap_or_else(|| lobby_json(&Default::default()))
    }

    /// Sets a lobby data key (the owner only); searches filter on these. Steam takes a few seconds
    /// to publish a change to searches (seen: not found after 0.3 s, found after 3 s).
    pub fn set_lobby_data(key: &str, value: &str) -> bool {
        with(|b| b.set_lobby_data(key, value)).unwrap_or(false)
    }

    /// Opens or closes the lobby to new players (the owner only).
    pub fn set_lobby_joinable(joinable: bool) -> bool {
        with(|b| b.set_lobby_joinable(joinable)).unwrap_or(false)
    }

    /// `"idle"`, `"searching"`, `"done"` or `"failed"`.
    pub fn search_status() -> String {
        with(|b| b.search().status.as_str().to_string()).unwrap_or_else(|| "idle".to_string())
    }

    /// The last search's results as JSON: `[{id, owner, members, maxMembers, data}]`.
    pub fn lobbies() -> String {
        with(|b| lobbies_json(&b.search().results)).unwrap_or_else(|| "[]".to_string())
    }
}

/// A stand-in that keeps everything in the process (tests, and running without Steam on
/// purpose). Requests finish at once, a search sees the lobbies this backend made.
#[derive(Default)]
pub struct MemoryBackend {
    pub presence: BTreeMap<String, String>,
    achievements: BTreeMap<String, bool>,
    stats: BTreeMap<String, f64>,
    /// Achievements and stats the last `store_stats` sent (what "Steam" would show).
    pub stored: Vec<String>,
    lobby: LobbyState,
    lobby_joinable: bool,
    known: Vec<LobbyInfo>,
    search: SearchState,
}

const MEMORY_USER: u64 = 76561190000000001;

impl MemoryBackend {
    pub fn new() -> Self {
        MemoryBackend {
            lobby_joinable: true,
            ..Default::default()
        }
    }

    fn member() -> Member {
        Member {
            id: MEMORY_USER,
            name: "Player".to_string(),
        }
    }

    fn sync_known(&mut self) {
        let lobby = &self.lobby;
        if let Some(info) = self.known.iter_mut().find(|l| l.id == lobby.id) {
            info.members = lobby.members.len() as u32;
            info.data = lobby.data.clone();
        }
    }
}

impl Backend for MemoryBackend {
    fn available(&self) -> bool {
        true
    }
    fn app_id(&self) -> u32 {
        1
    }
    fn user_name(&self) -> String {
        "Player".to_string()
    }
    fn user_id(&self) -> u64 {
        MEMORY_USER
    }
    fn set_rich_presence(&mut self, key: &str, value: Option<&str>) -> bool {
        match value {
            Some(v) => self.presence.insert(key.to_string(), v.to_string()),
            None => self.presence.remove(key),
        };
        true
    }
    fn clear_rich_presence(&mut self) {
        self.presence.clear();
    }
    fn achievement(&mut self, name: &str) -> Option<bool> {
        self.achievements.get(name).copied()
    }
    fn set_achievement(&mut self, name: &str, unlocked: bool) -> bool {
        self.achievements.insert(name.to_string(), unlocked);
        true
    }
    fn stat(&mut self, name: &str) -> Option<f64> {
        self.stats.get(name).copied()
    }
    fn set_stat(&mut self, name: &str, value: f64) -> bool {
        self.stats.insert(name.to_string(), value);
        true
    }
    fn store_stats(&mut self) -> bool {
        self.stored = self
            .achievements
            .iter()
            .filter(|(_, on)| **on)
            .map(|(n, _)| n.clone())
            .collect();
        true
    }
    fn create_lobby(&mut self, kind: LobbyKind, max_members: u32) {
        let id = 109775240000000000 + self.known.len() as u64 + 1;
        self.lobby = LobbyState {
            status: LobbyStatus::In,
            id,
            owner: MEMORY_USER,
            max_members,
            members: vec![Self::member()],
            data: BTreeMap::new(),
            error: String::new(),
        };
        self.lobby_joinable = true;
        if matches!(kind, LobbyKind::Public | LobbyKind::FriendsOnly) {
            self.known.push(LobbyInfo {
                id,
                owner: MEMORY_USER,
                members: 1,
                max_members,
                data: BTreeMap::new(),
            });
        }
    }
    fn find_lobbies(&mut self, filters: &[(String, String)]) {
        let results = self
            .known
            .iter()
            .filter(|l| filters.iter().all(|(k, v)| l.data.get(k) == Some(v)))
            .cloned()
            .collect();
        self.search = SearchState {
            status: SearchStatus::Done,
            results,
        };
    }
    fn join_lobby(&mut self, id: u64) {
        match self.known.iter().find(|l| l.id == id) {
            Some(info) if self.lobby_joinable && info.members < info.max_members => {
                self.lobby = LobbyState {
                    status: LobbyStatus::In,
                    id,
                    owner: info.owner,
                    max_members: info.max_members,
                    members: vec![Self::member()],
                    data: info.data.clone(),
                    error: String::new(),
                };
            }
            _ => {
                self.lobby = LobbyState {
                    status: LobbyStatus::Failed,
                    error: "the lobby is closed, full or gone".to_string(),
                    ..Default::default()
                }
            }
        }
    }
    fn leave_lobby(&mut self) {
        self.lobby = LobbyState::default();
    }
    fn set_lobby_data(&mut self, key: &str, value: &str) -> bool {
        if self.lobby.status != LobbyStatus::In || self.lobby.owner != MEMORY_USER {
            return false;
        }
        self.lobby.data.insert(key.to_string(), value.to_string());
        self.sync_known();
        true
    }
    fn set_lobby_joinable(&mut self, joinable: bool) -> bool {
        if self.lobby.status != LobbyStatus::In {
            return false;
        }
        self.lobby_joinable = joinable;
        true
    }
    fn lobby(&self) -> LobbyState {
        self.lobby.clone()
    }
    fn search(&self) -> SearchState {
        self.search.clone()
    }
    fn pump(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::steam_api as steam;
    use super::*;

    fn with_memory() {
        install(Some(Box::new(MemoryBackend::new())));
    }

    // M8-STM-01
    #[test]
    fn without_a_backend_every_call_is_a_harmless_no_op() {
        install(None);
        assert!(!steam::is_available());
        assert_eq!(steam::app_id(), 0.0);
        assert!(!steam::unlock_achievement("ACH_WIN"));
        assert!(!steam::is_achievement_unlocked("ACH_WIN"));
        assert_eq!(steam::lobby_status(), "none");
        assert_eq!(steam::search_status(), "idle");
        assert_eq!(steam::lobbies(), "[]");
    }

    // M8-STM-02
    #[test]
    fn app_id_user_and_rich_presence_reach_the_backend() {
        with_memory();
        assert!(steam::is_available());
        assert_eq!(steam::app_id(), 1.0);
        assert_eq!(steam::user_id(), MEMORY_USER.to_string());
        assert!(steam::set_rich_presence("status", "In the menu"));
        steam::clear_rich_presence();
    }

    // M8-STM-03
    #[test]
    fn unlocking_an_achievement_stores_it_and_clearing_locks_it() {
        with_memory();
        assert!(!steam::is_achievement_unlocked("ACH_WIN"));
        assert!(steam::unlock_achievement("ACH_WIN"));
        assert!(steam::is_achievement_unlocked("ACH_WIN"));
        assert!(steam::clear_achievement("ACH_WIN"));
        assert!(!steam::is_achievement_unlocked("ACH_WIN"));
        assert!(steam::set_stat("kills", 7.0));
        assert_eq!(steam::get_stat("kills"), 7.0);
        assert_eq!(steam::get_stat("unknown"), 0.0);
    }

    // M8-STM-04
    #[test]
    fn a_created_lobby_can_be_found_by_data_and_joined() {
        with_memory();
        steam::create_lobby("public", 4.0);
        assert_eq!(steam::lobby_status(), "in");
        assert!(steam::set_lobby_data("mode", "ctf"));

        steam::add_lobby_filter("mode", "deathmatch");
        steam::find_lobbies();
        assert_eq!(steam::search_status(), "done");
        assert_eq!(steam::lobbies(), "[]");

        steam::add_lobby_filter("mode", "ctf");
        steam::find_lobbies();
        let found: Value = serde_json::from_str(&steam::lobbies()).unwrap();
        let id = found[0]["id"].as_str().unwrap().to_string();
        assert_eq!(found[0]["data"]["mode"], "ctf");

        steam::leave_lobby();
        assert_eq!(steam::lobby_status(), "none");
        steam::join_lobby(&id);
        let lobby: Value = serde_json::from_str(&steam::lobby()).unwrap();
        assert_eq!(lobby["status"], "in");
        assert_eq!(lobby["id"], id.as_str());
        assert_eq!(lobby["data"]["mode"], "ctf");
    }

    // M8-STM-05
    #[test]
    fn joining_a_closed_or_unknown_lobby_fails() {
        with_memory();
        steam::create_lobby("public", 2.0);
        let id = serde_json::from_str::<Value>(&steam::lobby()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(steam::set_lobby_joinable(false));
        steam::join_lobby(&id);
        assert_eq!(steam::lobby_status(), "failed");
        steam::join_lobby("12345");
        assert_eq!(steam::lobby_status(), "failed");
    }
}
