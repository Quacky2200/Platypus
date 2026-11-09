// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The Steam client library (`steam_api64.dll`, `libsteam_api.so`, `libsteam_api.dylib`) loaded at
//! run time, and the few calls of its flat C API Platypus uses. Nothing links to Steam at build
//! time, so a program without the library starts and Steam is simply off; the library comes from
//! the Steamworks SDK and ships beside the game's exe.
//!
//! Asynchronous calls (lobbies) return a call handle; their results arrive through the manual
//! dispatch loop in [`Api::poll`]. Layouts follow the SDK's `steam_api_flat.h` and callback
//! structs (packed to 4 bytes off Windows).

use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;

/// `SteamAPICallCompleted_t`'s callback id.
pub const CALL_COMPLETED: i32 = 703;
pub const LOBBY_ENTER: i32 = 504;
pub const LOBBY_MATCH_LIST: i32 = 510;
pub const LOBBY_CREATED: i32 = 513;

/// The library's file name on this platform.
pub const LIBRARY_NAME: &str = if cfg!(windows) {
    "steam_api64.dll"
} else if cfg!(target_os = "macos") {
    "libsteam_api.dylib"
} else {
    "libsteam_api.so"
};

/// Where to look: beside the running program first, then wherever the system's loader looks.
pub fn library_candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        found.push(dir.join(LIBRARY_NAME));
    }
    found.push(PathBuf::from(LIBRARY_NAME));
    found
}

type Ptr = *mut c_void;
type Id = u64;

/// `CallbackMsg_t` (packed to 4 bytes; the offsets are the same on Windows).
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
struct CallbackMsg {
    _user: i32,
    callback: i32,
    param: *mut u8,
    size: i32,
}

/// The loaded library and the interfaces taken from it.
pub struct Api {
    shutdown: unsafe extern "C" fn(),
    run_frame: unsafe extern "C" fn(i32),
    next_callback: unsafe extern "C" fn(i32, *mut CallbackMsg) -> bool,
    free_callback: unsafe extern "C" fn(i32),
    call_result: unsafe extern "C" fn(i32, u64, *mut c_void, c_int, c_int, *mut bool) -> bool,
    pipe: i32,
    user: Ptr,
    friends: Ptr,
    matchmaking: Ptr,
    stats: Ptr,
    get_steam_id: unsafe extern "C" fn(Ptr) -> Id,
    persona_name: unsafe extern "C" fn(Ptr) -> *const c_char,
    friend_name: unsafe extern "C" fn(Ptr, Id) -> *const c_char,
    set_rich_presence: unsafe extern "C" fn(Ptr, *const c_char, *const c_char) -> bool,
    clear_rich_presence: unsafe extern "C" fn(Ptr),
    get_achievement: unsafe extern "C" fn(Ptr, *const c_char, *mut bool) -> bool,
    set_achievement: unsafe extern "C" fn(Ptr, *const c_char) -> bool,
    clear_achievement: unsafe extern "C" fn(Ptr, *const c_char) -> bool,
    get_stat_i32: unsafe extern "C" fn(Ptr, *const c_char, *mut i32) -> bool,
    get_stat_f32: unsafe extern "C" fn(Ptr, *const c_char, *mut f32) -> bool,
    set_stat_i32: unsafe extern "C" fn(Ptr, *const c_char, i32) -> bool,
    set_stat_f32: unsafe extern "C" fn(Ptr, *const c_char, f32) -> bool,
    store_stats: unsafe extern "C" fn(Ptr) -> bool,
    request_lobby_list: unsafe extern "C" fn(Ptr) -> u64,
    add_string_filter: unsafe extern "C" fn(Ptr, *const c_char, *const c_char, c_int),
    create_lobby: unsafe extern "C" fn(Ptr, c_int, c_int) -> u64,
    join_lobby: unsafe extern "C" fn(Ptr, Id) -> u64,
    leave_lobby: unsafe extern "C" fn(Ptr, Id),
    lobby_by_index: unsafe extern "C" fn(Ptr, c_int) -> Id,
    lobby_data_count: unsafe extern "C" fn(Ptr, Id) -> c_int,
    lobby_data_by_index:
        unsafe extern "C" fn(Ptr, Id, c_int, *mut c_char, c_int, *mut c_char, c_int) -> bool,
    set_lobby_data: unsafe extern "C" fn(Ptr, Id, *const c_char, *const c_char) -> bool,
    lobby_owner: unsafe extern "C" fn(Ptr, Id) -> Id,
    member_count: unsafe extern "C" fn(Ptr, Id) -> c_int,
    member_by_index: unsafe extern "C" fn(Ptr, Id, c_int) -> Id,
    member_limit: unsafe extern "C" fn(Ptr, Id) -> c_int,
    set_joinable: unsafe extern "C" fn(Ptr, Id, bool) -> bool,
    // Last, so the library outlives every pointer above while the struct is dropped in order.
    _lib: Library,
}

/// Takes a symbol of the library as the field's function type.
macro_rules! sym {
    ($lib:expr, $name:literal) => {
        *unsafe { $lib.get($name.as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?
    };
}

/// The first of several versioned accessors (`SteamAPI_SteamUser_v023`, ...) the library has.
fn interface(lib: &Library, names: &[&str]) -> Result<Ptr, String> {
    for name in names {
        if let Ok(get) = unsafe { lib.get::<unsafe extern "C" fn() -> Ptr>(name.as_bytes()) } {
            let p = unsafe { get() };
            if !p.is_null() {
                return Ok(p);
            }
        }
    }
    Err(format!("none of {} is available", names.join(", ")))
}

fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn c(s: &str) -> Option<CString> {
    CString::new(s).ok()
}

impl Api {
    /// Loads the library at `path` and initialises Steam as the app set in the `SteamAppId`
    /// environment variable (see [`crate::connect`]).
    pub fn load_from(path: &Path) -> Result<Api, String> {
        let lib = unsafe { Library::new(path) }
            .map_err(|e| format!("could not load {}: {e}", path.display()))?;
        let init: unsafe extern "C" fn(*mut c_char) -> i32 = sym!(lib, "SteamAPI_InitFlat");
        let mut message = [0 as c_char; 1024];
        let result = unsafe { init(message.as_mut_ptr()) };
        if result != 0 {
            let why = text(message.as_ptr());
            return Err(match result {
                2 => format!("the Steam client is not running ({why})"),
                3 => format!("the Steam library and client versions differ ({why})"),
                _ => format!("Steam init failed with code {result}: {why}"),
            });
        }
        let dispatch_init: unsafe extern "C" fn() = sym!(lib, "SteamAPI_ManualDispatch_Init");
        unsafe { dispatch_init() };
        let get_pipe: unsafe extern "C" fn() -> i32 = sym!(lib, "SteamAPI_GetHSteamPipe");
        let api = Api {
            shutdown: sym!(lib, "SteamAPI_Shutdown"),
            run_frame: sym!(lib, "SteamAPI_ManualDispatch_RunFrame"),
            next_callback: sym!(lib, "SteamAPI_ManualDispatch_GetNextCallback"),
            free_callback: sym!(lib, "SteamAPI_ManualDispatch_FreeLastCallback"),
            call_result: sym!(lib, "SteamAPI_ManualDispatch_GetAPICallResult"),
            pipe: unsafe { get_pipe() },
            user: interface(
                &lib,
                &[
                    "SteamAPI_SteamUser_v023",
                    "SteamAPI_SteamUser_v022",
                    "SteamAPI_SteamUser_v021",
                ],
            )?,
            friends: interface(
                &lib,
                &[
                    "SteamAPI_SteamFriends_v018",
                    "SteamAPI_SteamFriends_v017",
                    "SteamAPI_SteamFriends_v016",
                ],
            )?,
            matchmaking: interface(&lib, &["SteamAPI_SteamMatchmaking_v009"])?,
            stats: interface(
                &lib,
                &[
                    "SteamAPI_SteamUserStats_v013",
                    "SteamAPI_SteamUserStats_v012",
                    "SteamAPI_SteamUserStats_v011",
                ],
            )?,
            get_steam_id: sym!(lib, "SteamAPI_ISteamUser_GetSteamID"),
            persona_name: sym!(lib, "SteamAPI_ISteamFriends_GetPersonaName"),
            friend_name: sym!(lib, "SteamAPI_ISteamFriends_GetFriendPersonaName"),
            set_rich_presence: sym!(lib, "SteamAPI_ISteamFriends_SetRichPresence"),
            clear_rich_presence: sym!(lib, "SteamAPI_ISteamFriends_ClearRichPresence"),
            get_achievement: sym!(lib, "SteamAPI_ISteamUserStats_GetAchievement"),
            set_achievement: sym!(lib, "SteamAPI_ISteamUserStats_SetAchievement"),
            clear_achievement: sym!(lib, "SteamAPI_ISteamUserStats_ClearAchievement"),
            get_stat_i32: sym!(lib, "SteamAPI_ISteamUserStats_GetStatInt32"),
            get_stat_f32: sym!(lib, "SteamAPI_ISteamUserStats_GetStatFloat"),
            set_stat_i32: sym!(lib, "SteamAPI_ISteamUserStats_SetStatInt32"),
            set_stat_f32: sym!(lib, "SteamAPI_ISteamUserStats_SetStatFloat"),
            store_stats: sym!(lib, "SteamAPI_ISteamUserStats_StoreStats"),
            request_lobby_list: sym!(lib, "SteamAPI_ISteamMatchmaking_RequestLobbyList"),
            add_string_filter: sym!(
                lib,
                "SteamAPI_ISteamMatchmaking_AddRequestLobbyListStringFilter"
            ),
            create_lobby: sym!(lib, "SteamAPI_ISteamMatchmaking_CreateLobby"),
            join_lobby: sym!(lib, "SteamAPI_ISteamMatchmaking_JoinLobby"),
            leave_lobby: sym!(lib, "SteamAPI_ISteamMatchmaking_LeaveLobby"),
            lobby_by_index: sym!(lib, "SteamAPI_ISteamMatchmaking_GetLobbyByIndex"),
            lobby_data_count: sym!(lib, "SteamAPI_ISteamMatchmaking_GetLobbyDataCount"),
            lobby_data_by_index: sym!(lib, "SteamAPI_ISteamMatchmaking_GetLobbyDataByIndex"),
            set_lobby_data: sym!(lib, "SteamAPI_ISteamMatchmaking_SetLobbyData"),
            lobby_owner: sym!(lib, "SteamAPI_ISteamMatchmaking_GetLobbyOwner"),
            member_count: sym!(lib, "SteamAPI_ISteamMatchmaking_GetNumLobbyMembers"),
            member_by_index: sym!(lib, "SteamAPI_ISteamMatchmaking_GetLobbyMemberByIndex"),
            member_limit: sym!(lib, "SteamAPI_ISteamMatchmaking_GetLobbyMemberLimit"),
            set_joinable: sym!(lib, "SteamAPI_ISteamMatchmaking_SetLobbyJoinable"),
            _lib: lib,
        };
        Ok(api)
    }

    /// Loads the library from the first place [`library_candidates`] finds it.
    pub fn load() -> Result<Api, String> {
        let mut last = String::new();
        for path in library_candidates() {
            match Api::load_from(&path) {
                Ok(api) => return Ok(api),
                // A library that loaded but failed to initialise says why: don't hide it behind
                // the next candidate's "not found".
                Err(e) if !e.starts_with("could not load") => return Err(e),
                Err(e) => last = e,
            }
        }
        Err(format!(
            "{LIBRARY_NAME} not found beside the program ({last})"
        ))
    }

    /// Runs Steam's callbacks and hands every finished asynchronous call to `done` as
    /// `(call handle, callback id, result bytes)`; the bytes are `None` when the call failed.
    pub fn poll(&self, mut done: impl FnMut(u64, i32, Option<Vec<u8>>)) {
        unsafe { (self.run_frame)(self.pipe) };
        loop {
            let mut msg = CallbackMsg {
                _user: 0,
                callback: 0,
                param: ptr::null_mut(),
                size: 0,
            };
            if !unsafe { (self.next_callback)(self.pipe, &mut msg) } {
                break;
            }
            let (callback, param) = (msg.callback, msg.param);
            if callback == CALL_COMPLETED && !param.is_null() {
                // SteamAPICallCompleted_t: handle u64 @0, callback id i32 @8, result size u32 @12.
                let (handle, id, size) = unsafe {
                    (
                        ptr::read_unaligned(param as *const u64),
                        ptr::read_unaligned(param.add(8) as *const i32),
                        ptr::read_unaligned(param.add(12) as *const u32),
                    )
                };
                let mut buf = vec![0u8; size as usize];
                let mut failed = false;
                let ok = unsafe {
                    (self.call_result)(
                        self.pipe,
                        handle,
                        buf.as_mut_ptr().cast(),
                        size as c_int,
                        id,
                        &mut failed,
                    )
                };
                done(handle, id, (ok && !failed).then_some(buf));
            }
            unsafe { (self.free_callback)(self.pipe) };
        }
    }

    pub fn user_id(&self) -> u64 {
        unsafe { (self.get_steam_id)(self.user) }
    }

    pub fn user_name(&self) -> String {
        text(unsafe { (self.persona_name)(self.friends) })
    }

    pub fn friend_name(&self, id: u64) -> String {
        text(unsafe { (self.friend_name)(self.friends, id) })
    }

    /// `value` of `None` clears the key.
    pub fn set_rich_presence(&self, key: &str, value: Option<&str>) -> bool {
        let Some(key) = c(key) else { return false };
        let value = match value {
            Some(v) => match c(v) {
                Some(v) => Some(v),
                None => return false,
            },
            None => None,
        };
        let value_ptr = value.as_ref().map_or(ptr::null(), |v| v.as_ptr());
        unsafe { (self.set_rich_presence)(self.friends, key.as_ptr(), value_ptr) }
    }

    pub fn clear_rich_presence(&self) {
        unsafe { (self.clear_rich_presence)(self.friends) }
    }

    pub fn achievement(&self, name: &str) -> Option<bool> {
        let name = c(name)?;
        let mut on = false;
        unsafe { (self.get_achievement)(self.stats, name.as_ptr(), &mut on) }.then_some(on)
    }

    pub fn set_achievement(&self, name: &str, unlocked: bool) -> bool {
        let Some(name) = c(name) else { return false };
        unsafe {
            if unlocked {
                (self.set_achievement)(self.stats, name.as_ptr())
            } else {
                (self.clear_achievement)(self.stats, name.as_ptr())
            }
        }
    }

    /// An int stat, else a float stat.
    pub fn stat(&self, name: &str) -> Option<f64> {
        let name = c(name)?;
        let mut i = 0i32;
        if unsafe { (self.get_stat_i32)(self.stats, name.as_ptr(), &mut i) } {
            return Some(f64::from(i));
        }
        let mut f = 0f32;
        unsafe { (self.get_stat_f32)(self.stats, name.as_ptr(), &mut f) }.then_some(f64::from(f))
    }

    /// A whole value goes to an int stat when there is one, else to a float stat.
    pub fn set_stat(&self, name: &str, value: f64) -> bool {
        let Some(name) = c(name) else { return false };
        if value.fract() == 0.0
            && unsafe { (self.set_stat_i32)(self.stats, name.as_ptr(), value as i32) }
        {
            return true;
        }
        unsafe { (self.set_stat_f32)(self.stats, name.as_ptr(), value as f32) }
    }

    pub fn store_stats(&self) -> bool {
        unsafe { (self.store_stats)(self.stats) }
    }

    /// Starts a lobby list request with equality filters on lobby data; returns its call handle.
    pub fn request_lobby_list(&self, filters: &[(String, String)]) -> u64 {
        for (key, value) in filters {
            // Steam limits a key to 255 bytes; a longer one can't match anything.
            if let (Some(k), Some(v)) = (c(key), c(value))
                && key.len() <= 255
            {
                // k_ELobbyComparisonEqual
                unsafe { (self.add_string_filter)(self.matchmaking, k.as_ptr(), v.as_ptr(), 0) };
            }
        }
        unsafe { (self.request_lobby_list)(self.matchmaking) }
    }

    /// `kind`: 0 private, 1 friends only, 2 public, 3 invisible.
    pub fn create_lobby(&self, kind: i32, max_members: i32) -> u64 {
        unsafe { (self.create_lobby)(self.matchmaking, kind, max_members) }
    }

    pub fn join_lobby(&self, id: u64) -> u64 {
        unsafe { (self.join_lobby)(self.matchmaking, id) }
    }

    pub fn leave_lobby(&self, id: u64) {
        unsafe { (self.leave_lobby)(self.matchmaking, id) }
    }

    /// The lobby ids of the last list result.
    pub fn found_lobbies(&self, count: u32) -> Vec<u64> {
        (0..count as c_int)
            .map(|i| unsafe { (self.lobby_by_index)(self.matchmaking, i) })
            .collect()
    }

    pub fn lobby_data(&self, id: u64) -> std::collections::BTreeMap<String, String> {
        let count = unsafe { (self.lobby_data_count)(self.matchmaking, id) };
        let mut out = std::collections::BTreeMap::new();
        for i in 0..count {
            let (mut key, mut value) = ([0 as c_char; 256], [0 as c_char; 8192]);
            let ok = unsafe {
                (self.lobby_data_by_index)(
                    self.matchmaking,
                    id,
                    i,
                    key.as_mut_ptr(),
                    key.len() as c_int,
                    value.as_mut_ptr(),
                    value.len() as c_int,
                )
            };
            if ok {
                out.insert(text(key.as_ptr()), text(value.as_ptr()));
            }
        }
        out
    }

    pub fn set_lobby_data(&self, id: u64, key: &str, value: &str) -> bool {
        let (Some(k), Some(v)) = (c(key), c(value)) else {
            return false;
        };
        unsafe { (self.set_lobby_data)(self.matchmaking, id, k.as_ptr(), v.as_ptr()) }
    }

    pub fn lobby_owner(&self, id: u64) -> u64 {
        unsafe { (self.lobby_owner)(self.matchmaking, id) }
    }

    pub fn member_count(&self, id: u64) -> u32 {
        unsafe { (self.member_count)(self.matchmaking, id) }.max(0) as u32
    }

    pub fn members(&self, id: u64) -> Vec<u64> {
        (0..self.member_count(id) as c_int)
            .map(|i| unsafe { (self.member_by_index)(self.matchmaking, id, i) })
            .collect()
    }

    pub fn member_limit(&self, id: u64) -> u32 {
        unsafe { (self.member_limit)(self.matchmaking, id) }.max(0) as u32
    }

    pub fn set_joinable(&self, id: u64, joinable: bool) -> bool {
        unsafe { (self.set_joinable)(self.matchmaking, id, joinable) }
    }
}

impl Drop for Api {
    fn drop(&mut self) {
        // Steam then sees the game close; the library unloads after this (`_lib` is the last field).
        unsafe { (self.shutdown)() };
    }
}

fn read<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at + N)?.try_into().ok()
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    read(bytes, at).map(u32::from_le_bytes)
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    read(bytes, at).map(u64::from_le_bytes)
}

/// What an `EResult` from Steam means, for the few a lobby call returns.
pub fn result_text(code: i32) -> String {
    match code {
        2 => "failed".into(),
        3 => "no connection to Steam".into(),
        15 => "access denied".into(),
        16 => "timed out".into(),
        25 => "limit exceeded".into(),
        n => format!("Steam error {n}"),
    }
}

/// `LobbyCreated_t`: result i32 @0, lobby id u64 @8 on Windows (aligned) and @4 elsewhere (packed).
pub fn lobby_created(bytes: &[u8]) -> Result<u64, String> {
    let id_at = if cfg!(windows) { 8 } else { 4 };
    let result = u32_at(bytes, 0).ok_or("a short lobby result")? as i32;
    if result != 1 {
        return Err(result_text(result));
    }
    u64_at(bytes, id_at).ok_or_else(|| "a short lobby result".to_string())
}

/// `LobbyEnter_t`: lobby id u64 @0, enter response u32 @16 (1 is success).
pub fn lobby_enter(bytes: &[u8]) -> Result<u64, String> {
    let response = u32_at(bytes, 16).ok_or("a short join result")?;
    if response != 1 {
        return Err(format!("Steam refused the join (response {response})"));
    }
    u64_at(bytes, 0).ok_or_else(|| "a short join result".to_string())
}

/// `LobbyMatchList_t`: the number of lobbies u32 @0.
pub fn lobby_match_count(bytes: &[u8]) -> Option<u32> {
    u32_at(bytes, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // M8-STM-10
    #[test]
    fn a_missing_library_is_an_error_not_a_crash() {
        let nowhere = Path::new("definitely-not-here").join(LIBRARY_NAME);
        let e = Api::load_from(&nowhere).err().expect("no such library");
        assert!(e.starts_with("could not load"), "{e}");
    }

    // M8-STM-10
    #[test]
    fn the_library_is_looked_for_beside_the_program_first() {
        let c = library_candidates();
        assert_eq!(
            c.last().map(PathBuf::as_path),
            Some(Path::new(LIBRARY_NAME))
        );
        if c.len() == 2 {
            assert!(c[0].ends_with(LIBRARY_NAME) && c[0].is_absolute());
        }
    }

    // M8-STM-11
    #[test]
    fn lobby_results_decode_with_the_platforms_layout() {
        let id_at = if cfg!(windows) { 8 } else { 4 };
        let mut created = vec![0u8; 16];
        created[..4].copy_from_slice(&1i32.to_le_bytes());
        created[id_at..id_at + 8].copy_from_slice(&109775240000000007u64.to_le_bytes());
        assert_eq!(lobby_created(&created), Ok(109775240000000007));
        created[..4].copy_from_slice(&15i32.to_le_bytes());
        assert_eq!(lobby_created(&created), Err("access denied".to_string()));
        assert!(lobby_created(&[1, 0]).is_err());

        let mut enter = vec![0u8; 24];
        enter[..8].copy_from_slice(&42u64.to_le_bytes());
        enter[16..20].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(lobby_enter(&enter), Ok(42));
        enter[16..20].copy_from_slice(&5u32.to_le_bytes());
        assert!(lobby_enter(&enter).unwrap_err().contains("response 5"));

        assert_eq!(lobby_match_count(&3u32.to_le_bytes()), Some(3));
        assert_eq!(lobby_match_count(&[]), None);
    }
}
