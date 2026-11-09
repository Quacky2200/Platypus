// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The real Steam backend of the script `Steam` namespace
//! (`platypus_engine_api::steam`): initialising with an app id (so Steam shows the game as running
//! and achievements and lobbies work), rich presence, achievements and stats, and matchmaking
//! lobbies. The Steam library is loaded at run time (`api.rs`), on Windows, Linux and macOS alike:
//! a program without it starts, and [`install`] just returns why Steam is off.
//!
//! The host calls [`install`] at start with the game's app id and
//! `platypus_engine_api::steam::pump` each frame. Steam must be running on the same user.
//!
//! A game opts in with `"steam": { "appId": N }` in `project.json`. There is no default id: using
//! another game's would put lobbies and searches among that game's real players.

mod api;

pub use api::{LIBRARY_NAME, library_candidates};

use api::{Api, LOBBY_CREATED, LOBBY_ENTER, LOBBY_MATCH_LIST};
use platypus_engine_api::steam::{
    Backend, LobbyInfo, LobbyKind, LobbyState, LobbyStatus, Member, SearchState, SearchStatus,
    install as install_backend,
};
use std::path::PathBuf;

/// What an asynchronous Steam call was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Create,
    Join,
    Search,
}

impl Pending {
    fn callback(self) -> i32 {
        match self {
            Pending::Create => LOBBY_CREATED,
            Pending::Join => LOBBY_ENTER,
            Pending::Search => LOBBY_MATCH_LIST,
        }
    }
}

pub struct SteamBackend {
    api: Api,
    app_id: u32,
    pending: Vec<(u64, Pending)>,
    lobby: LobbyState,
    search: SearchState,
}

/// Loads the Steam library and connects to the running Steam client as `app_id`.
///
/// `search` are folders to look for the library in after the program's own (the game's `steam/`
/// folder and the game folder).
pub fn connect(app_id: u32, search: &[PathBuf]) -> Result<SteamBackend, String> {
    // Steam reads the app id from the environment. Nothing else reads or writes the environment
    // concurrently here: this runs once at start-up, before the game's threads.
    unsafe {
        std::env::set_var("SteamAppId", app_id.to_string());
        std::env::set_var("SteamGameId", app_id.to_string());
    }
    let api = Api::load(search)?;
    Ok(SteamBackend {
        api,
        app_id,
        pending: Vec::new(),
        lobby: LobbyState::default(),
        search: SearchState::default(),
    })
}

/// Connects as `app_id` and installs it as the script API's backend.
/// On failure (no Steam library, Steam not running, no licence for the id) nothing is installed
/// and the reason returns.
pub fn install(app_id: u32, search: &[PathBuf]) -> Result<u32, String> {
    let backend = connect(app_id, search)?;
    install_backend(Some(Box::new(backend)));
    Ok(app_id)
}

/// Steam's lobby type number.
fn lobby_type(kind: LobbyKind) -> i32 {
    match kind {
        LobbyKind::Private => 0,
        LobbyKind::FriendsOnly => 1,
        LobbyKind::Public => 2,
        LobbyKind::Invisible => 3,
    }
}

impl SteamBackend {
    /// Reads the lobby the player is in from Steam (members, owner, data).
    fn refresh_lobby(&mut self) {
        if self.lobby.status != LobbyStatus::In {
            return;
        }
        let id = self.lobby.id;
        self.lobby.owner = self.api.lobby_owner(id);
        self.lobby.max_members = self.api.member_limit(id);
        self.lobby.members = self
            .api
            .members(id)
            .into_iter()
            .map(|m| Member {
                id: m,
                name: self.api.friend_name(m),
            })
            .collect();
        self.lobby.data = self.api.lobby_data(id);
    }

    fn entered(&mut self, id: u64) {
        self.lobby = LobbyState {
            status: LobbyStatus::In,
            id,
            ..Default::default()
        };
        self.refresh_lobby();
    }

    fn failed(&mut self, error: String) {
        self.lobby = LobbyState {
            status: LobbyStatus::Failed,
            error,
            ..Default::default()
        };
    }

    fn finish(&mut self, what: Pending, result: Option<Vec<u8>>) {
        match what {
            Pending::Create | Pending::Join => {
                let decoded = match (what, result) {
                    (_, None) => Err("the request to Steam failed".to_string()),
                    (Pending::Create, Some(b)) => api::lobby_created(&b),
                    (_, Some(b)) => api::lobby_enter(&b),
                };
                match decoded {
                    Ok(id) => self.entered(id),
                    Err(e) => self.failed(e),
                }
            }
            Pending::Search => {
                let count = result.as_deref().and_then(api::lobby_match_count);
                self.search = match count {
                    Some(count) => SearchState {
                        status: SearchStatus::Done,
                        results: self
                            .api
                            .found_lobbies(count)
                            .into_iter()
                            .map(|id| LobbyInfo {
                                id,
                                owner: self.api.lobby_owner(id),
                                members: self.api.member_count(id),
                                max_members: self.api.member_limit(id),
                                data: self.api.lobby_data(id),
                            })
                            .collect(),
                    },
                    None => SearchState {
                        status: SearchStatus::Failed,
                        results: Vec::new(),
                    },
                };
            }
        }
    }
}

impl Backend for SteamBackend {
    fn available(&self) -> bool {
        true
    }

    fn app_id(&self) -> u32 {
        self.app_id
    }

    fn user_name(&self) -> String {
        self.api.user_name()
    }

    fn user_id(&self) -> u64 {
        self.api.user_id()
    }

    fn set_rich_presence(&mut self, key: &str, value: Option<&str>) -> bool {
        self.api.set_rich_presence(key, value)
    }

    fn clear_rich_presence(&mut self) {
        self.api.clear_rich_presence();
    }

    fn achievement(&mut self, name: &str) -> Option<bool> {
        self.api.achievement(name)
    }

    fn set_achievement(&mut self, name: &str, unlocked: bool) -> bool {
        self.api.set_achievement(name, unlocked)
    }

    fn stat(&mut self, name: &str) -> Option<f64> {
        self.api.stat(name)
    }

    fn set_stat(&mut self, name: &str, value: f64) -> bool {
        self.api.set_stat(name, value)
    }

    fn store_stats(&mut self) -> bool {
        self.api.store_stats()
    }

    fn create_lobby(&mut self, kind: LobbyKind, max_members: u32) {
        self.lobby = LobbyState {
            status: LobbyStatus::Creating,
            ..Default::default()
        };
        let call = self
            .api
            .create_lobby(lobby_type(kind), max_members.min(250) as i32);
        self.pending.push((call, Pending::Create));
    }

    fn find_lobbies(&mut self, filters: &[(String, String)]) {
        self.search = SearchState {
            status: SearchStatus::Searching,
            results: Vec::new(),
        };
        let call = self.api.request_lobby_list(filters);
        self.pending.push((call, Pending::Search));
    }

    fn join_lobby(&mut self, id: u64) {
        self.lobby = LobbyState {
            status: LobbyStatus::Joining,
            id,
            ..Default::default()
        };
        let call = self.api.join_lobby(id);
        self.pending.push((call, Pending::Join));
    }

    fn leave_lobby(&mut self) {
        if self.lobby.status == LobbyStatus::In {
            self.api.leave_lobby(self.lobby.id);
        }
        self.lobby = LobbyState::default();
    }

    fn set_lobby_data(&mut self, key: &str, value: &str) -> bool {
        if self.lobby.status != LobbyStatus::In {
            return false;
        }
        let ok = self.api.set_lobby_data(self.lobby.id, key, value);
        if ok {
            self.lobby.data.insert(key.to_string(), value.to_string());
        }
        ok
    }

    fn set_lobby_joinable(&mut self, joinable: bool) -> bool {
        self.lobby.status == LobbyStatus::In && self.api.set_joinable(self.lobby.id, joinable)
    }

    fn lobby(&self) -> LobbyState {
        self.lobby.clone()
    }

    fn search(&self) -> SearchState {
        self.search.clone()
    }

    fn pump(&mut self) {
        let mut done = Vec::new();
        self.api
            .poll(|call, callback, result| done.push((call, callback, result)));
        for (call, callback, result) in done {
            // A call counts only for the kind of result it was started for.
            if let Some(at) = self
                .pending
                .iter()
                .position(|(c, what)| *c == call && what.callback() == callback)
            {
                let (_, what) = self.pending.remove(at);
                self.finish(what, result);
            }
        }
        self.refresh_lobby();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M8-STM-07
    #[test]
    fn lobby_kinds_map_to_steams_lobby_type_numbers() {
        assert_eq!(lobby_type(LobbyKind::Private), 0);
        assert_eq!(lobby_type(LobbyKind::FriendsOnly), 1);
        assert_eq!(lobby_type(LobbyKind::Public), 2);
        assert_eq!(lobby_type(LobbyKind::Invisible), 3);
    }

    // M8-STM-12
    #[test]
    fn each_pending_call_expects_its_own_result_type() {
        assert_eq!(Pending::Create.callback(), 513);
        assert_eq!(Pending::Join.callback(), 504);
        assert_eq!(Pending::Search.callback(), 510);
    }

    // M8-STM-08: runs only with the Steam client up, its library beside the test program or on the
    // loader's path, and your own app id in `PLATYPUS_STEAM_APP_ID`
    // (`cargo test -p platypus_steam -- --ignored`); without the variable it does nothing.
    #[test]
    #[ignore = "needs the Steam client, its library and PLATYPUS_STEAM_APP_ID"]
    fn connecting_reports_the_app_and_user() {
        let Some(id) = std::env::var("PLATYPUS_STEAM_APP_ID")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
        else {
            return;
        };
        let b = connect(id, &[]).expect("Steam is running and its library is found");
        assert_eq!(b.app_id(), id);
        assert_ne!(b.user_id(), 0);
    }
}
