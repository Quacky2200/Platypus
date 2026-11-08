// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Acoustics: sound that knows about the world. Zones that colour what the listener hears (a
//! cave's echo, the sea's waves, a tunnel's dullness) with gradients that fade in with
//! distance; occluders that muffle sound through walls and doors; sensors that hear noises
//! like guards do; Doppler for things that move past. It sits on the mixer's effect chains
//! (`platypus_audio`) and the physics rays (`platypus_physics_api`).
//!
//! The host calls [`update`] once a frame (after scripts and transforms, before it takes the
//! audio commands), [`register_kinds`] once, [`object_destroyed`] for removed objects and
//! [`reset`] on a new game. The maths is in [`model`] and needs none of that.

mod components;
mod kinds;
pub mod model;
mod state;
#[cfg(test)]
mod tests;
mod update;

pub use components::object_destroyed;
pub use kinds::register_kinds;
pub use state::{
    AudioOccluderRef, AudioPortalRef, AudioZoneRef, SoundEventRef, SoundSensorRef, reset,
};
pub use update::update;

/// The manifests of this crate's script API.
pub fn manifests() -> Vec<String> {
    vec![
        components::audio_zone_api::MANIFEST.to_json(),
        components::audio_occluder_api::MANIFEST.to_json(),
        components::audio_portal_api::MANIFEST.to_json(),
        components::sound_sensor_api::MANIFEST.to_json(),
        components::sound_event_api::MANIFEST.to_json(),
        components::acoustics_api::MANIFEST.to_json(),
    ]
}
