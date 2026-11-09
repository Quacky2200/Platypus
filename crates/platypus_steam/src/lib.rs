// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The real Steam backend of the script `Steam` namespace
//! (`platypus_engine_api::steam`), over the `steamworks` crate: initialising with an app id (so Steam
//! shows the game as running and overlay, achievements and lobbies work), rich presence,
//! achievements and stats, and matchmaking lobbies.
//!
//! The host calls [`install`] at start with the game's app id and `platypus_engine_api::steam::pump`
//! each frame. Steam must be running on the same user; without it [`install`] fails and the script
//! API stays the harmless no-op.
//!
//! A game opts in with `"steam": { "appId": N }` in `project.json`. There is no default id: using
//! another game's would put lobbies and searches among that game's real players.

use platypus_engine_api::steam::{
    Backend, LobbyInfo, LobbyKind, LobbyState, LobbyStatus, Member, SearchState, SearchStatus,
    install as install_backend,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use steamworks::{Client, LobbyId, LobbyKey, LobbyType, StringFilter, StringFilterKind};

/// Finished requests, set by Steam's callbacks (which may run on any thread that pumps) and read
/// by [`SteamBackend::pump`] on the script thread.
#[derive(Default)]
struct Shared {
    lobby: Option<Result<u64, String>>,
    search: Option<Result<Vec<u64>, ()>>,
}

pub struct SteamBackend {
    client: Client,
    app_id: u32,
    shared: Arc<Mutex<Shared>>,
    lobby: LobbyState,
    search: SearchState,
}

/// Connects to the running Steam client as `app_id`.
pub fn connect(app_id: u32) -> Result<SteamBackend, String> {
    let client = Client::init_app(app_id).map_err(|e| e.to_string())?;
    // The stats and achievements of the player load before they are read or set.
    client
        .user_stats()
        .request_user_stats(client.user().steam_id().raw());
    Ok(SteamBackend {
        client,
        app_id,
        shared: Arc::default(),
        lobby: LobbyState::default(),
        search: SearchState::default(),
    })
}

/// Connects as `app_id` and installs it as the script API's backend.
/// On failure (Steam not running, no licence for the id) nothing is installed and the reason returns.
pub fn install(app_id: u32) -> Result<u32, String> {
    let backend = connect(app_id)?;
    install_backend(Some(Box::new(backend)));
    Ok(app_id)
}

fn lobby_type(kind: LobbyKind) -> LobbyType {
    match kind {
        LobbyKind::Public => LobbyType::Public,
        LobbyKind::FriendsOnly => LobbyType::FriendsOnly,
        LobbyKind::Private => LobbyType::Private,
        LobbyKind::Invisible => LobbyType::Invisible,
    }
}

impl SteamBackend {
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lobby_data(&self, id: LobbyId) -> BTreeMap<String, String> {
        let mm = self.client.matchmaking();
        (0..mm.lobby_data_count(id))
            .filter_map(|i| mm.lobby_data_by_index(id, i))
            .collect()
    }

    /// Reads the lobby the player is in from Steam (members, owner, data).
    fn refresh_lobby(&mut self) {
        if self.lobby.status != LobbyStatus::In {
            return;
        }
        let id = LobbyId::from_raw(self.lobby.id);
        let mm = self.client.matchmaking();
        let friends = self.client.friends();
        self.lobby.owner = mm.lobby_owner(id).raw();
        self.lobby.max_members = mm.lobby_member_limit(id).unwrap_or(0) as u32;
        self.lobby.members = mm
            .lobby_members(id)
            .into_iter()
            .map(|m| Member {
                id: m.raw(),
                name: friends.get_friend(m).name(),
            })
            .collect();
        self.lobby.data = self.lobby_data(id);
    }

    fn entered(&mut self, id: u64) {
        self.lobby = LobbyState {
            status: LobbyStatus::In,
            id,
            ..Default::default()
        };
        self.refresh_lobby();
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
        self.client.friends().name()
    }

    fn user_id(&self) -> u64 {
        self.client.user().steam_id().raw()
    }

    fn set_rich_presence(&mut self, key: &str, value: Option<&str>) -> bool {
        self.client.friends().set_rich_presence(key, value)
    }

    fn clear_rich_presence(&mut self) {
        self.client.friends().clear_rich_presence();
    }

    fn achievement(&mut self, name: &str) -> Option<bool> {
        self.client.user_stats().achievement(name).get().ok()
    }

    fn set_achievement(&mut self, name: &str, unlocked: bool) -> bool {
        let stats = self.client.user_stats();
        let a = stats.achievement(name);
        if unlocked { a.set() } else { a.clear() }.is_ok()
    }

    fn stat(&mut self, name: &str) -> Option<f64> {
        let stats = self.client.user_stats();
        // A stat is an int or a float on Steam; try the int first, then the float.
        stats
            .get_stat_i32(name)
            .map(f64::from)
            .or_else(|_| stats.get_stat_f32(name).map(f64::from))
            .ok()
    }

    fn set_stat(&mut self, name: &str, value: f64) -> bool {
        let stats = self.client.user_stats();
        // A whole value goes to an int stat when there is one, else to a float stat.
        if value.fract() == 0.0 && stats.set_stat_i32(name, value as i32).is_ok() {
            return true;
        }
        stats.set_stat_f32(name, value as f32).is_ok()
    }

    fn store_stats(&mut self) -> bool {
        self.client.user_stats().store_stats().is_ok()
    }

    fn create_lobby(&mut self, kind: LobbyKind, max_members: u32) {
        self.lobby = LobbyState {
            status: LobbyStatus::Creating,
            ..Default::default()
        };
        let shared = self.shared.clone();
        self.client
            .matchmaking()
            .create_lobby(lobby_type(kind), max_members, move |r| {
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                s.lobby = Some(r.map(|id| id.raw()).map_err(|e| e.to_string()));
            });
    }

    fn find_lobbies(&mut self, filters: &[(String, String)]) {
        let mm = self.client.matchmaking();
        for (key, value) in filters {
            // Steam limits a key to 255 bytes; a longer one can't match anything.
            if let Ok(key) = LobbyKey::try_new(key) {
                mm.add_request_lobby_list_string_filter(StringFilter(
                    key,
                    value,
                    StringFilterKind::Equal,
                ));
            }
        }
        self.search = SearchState {
            status: SearchStatus::Searching,
            results: Vec::new(),
        };
        let shared = self.shared.clone();
        mm.request_lobby_list(move |r| {
            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
            s.search = Some(
                r.map(|ids| ids.iter().map(|i| i.raw()).collect())
                    .map_err(|_| ()),
            );
        });
    }

    fn join_lobby(&mut self, id: u64) {
        self.lobby = LobbyState {
            status: LobbyStatus::Joining,
            id,
            ..Default::default()
        };
        let shared = self.shared.clone();
        self.client
            .matchmaking()
            .join_lobby(LobbyId::from_raw(id), move |r| {
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                s.lobby = Some(
                    r.map(|id| id.raw())
                        .map_err(|()| "Steam refused the join (full, closed or gone)".to_string()),
                );
            });
    }

    fn leave_lobby(&mut self) {
        if self.lobby.status == LobbyStatus::In {
            self.client
                .matchmaking()
                .leave_lobby(LobbyId::from_raw(self.lobby.id));
        }
        self.lobby = LobbyState::default();
    }

    fn set_lobby_data(&mut self, key: &str, value: &str) -> bool {
        if self.lobby.status != LobbyStatus::In {
            return false;
        }
        let ok =
            self.client
                .matchmaking()
                .set_lobby_data(LobbyId::from_raw(self.lobby.id), key, value);
        if ok {
            self.lobby.data.insert(key.to_string(), value.to_string());
        }
        ok
    }

    fn set_lobby_joinable(&mut self, joinable: bool) -> bool {
        self.lobby.status == LobbyStatus::In
            && self
                .client
                .matchmaking()
                .set_lobby_joinable(LobbyId::from_raw(self.lobby.id), joinable)
    }

    fn lobby(&self) -> LobbyState {
        self.lobby.clone()
    }

    fn search(&self) -> SearchState {
        self.search.clone()
    }

    fn pump(&mut self) {
        self.client.run_callbacks();
        let (lobby, search) = {
            let mut s = self.lock();
            (s.lobby.take(), s.search.take())
        };
        match lobby {
            Some(Ok(id)) => self.entered(id),
            Some(Err(error)) => {
                self.lobby = LobbyState {
                    status: LobbyStatus::Failed,
                    error,
                    ..Default::default()
                }
            }
            None => self.refresh_lobby(),
        }
        match search {
            Some(Ok(ids)) => {
                let mm = self.client.matchmaking();
                let results = ids
                    .into_iter()
                    .map(|raw| {
                        let id = LobbyId::from_raw(raw);
                        LobbyInfo {
                            id: raw,
                            owner: mm.lobby_owner(id).raw(),
                            members: mm.lobby_member_count(id) as u32,
                            max_members: mm.lobby_member_limit(id).unwrap_or(0) as u32,
                            data: self.lobby_data(id),
                        }
                    })
                    .collect();
                self.search = SearchState {
                    status: SearchStatus::Done,
                    results,
                };
            }
            Some(Err(())) => {
                self.search = SearchState {
                    status: SearchStatus::Failed,
                    results: Vec::new(),
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M8-STM-07
    #[test]
    fn lobby_kinds_map_to_steams_visibility() {
        assert_eq!(lobby_type(LobbyKind::Public), LobbyType::Public);
        assert_eq!(lobby_type(LobbyKind::FriendsOnly), LobbyType::FriendsOnly);
        assert_eq!(lobby_type(LobbyKind::Private), LobbyType::Private);
        assert_eq!(lobby_type(LobbyKind::Invisible), LobbyType::Invisible);
    }

    // M8-STM-08: runs only with the Steam client up and your own app id in `PLATYPUS_STEAM_APP_ID`
    // (`cargo test -p platypus_steam -- --ignored`); without the variable it does nothing.
    #[test]
    #[ignore = "needs the Steam client running and PLATYPUS_STEAM_APP_ID"]
    fn connecting_reports_the_app_and_user() {
        let Some(id) = std::env::var("PLATYPUS_STEAM_APP_ID")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
        else {
            return;
        };
        let b = connect(id).expect("Steam is running");
        assert_eq!(b.app_id(), id);
        assert_ne!(b.user_id(), 0);
    }
}
