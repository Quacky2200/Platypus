// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The per-frame work: where the listener stands in each zone, what stands between each sound
//! and the listener, how fast sources move, and what sensors hear. The host calls
//! [`update`] once a frame, after transforms are final and before the audio commands are taken.

use crate::model::{
    FxTarget, OccluderSpec, Probe, air_cutoff, average, blend_zones, cone_amount, cone_effect,
    detour_wins, doppler, head_cue, heard_level, pattern_effect, pattern_gain, rolloff_gain,
    scaled, through, through_portal, zone_weight,
};
use crate::state::{Event, SoundEventRef, Zone, push_event, st};
use glam::{Quat, Vec3};
use platypus_engine_api::audio::{
    self, BusRef, PlayOptions, PlaybackRef, play_sound, set_fx, set_track_volume,
};
use platypus_engine_api::audio_components::{
    Directivity, Rolloff, SourceView, set_doppler_factor, set_virtual_position, source_views,
};
use platypus_math::V3;
use platypus_scene::ObjectId;
use platypus_scene::api::with_scene;
use std::collections::{BTreeMap, HashMap};
use ts_api::callback::HandleArg;

/// How long filters and volumes glide to a new value (seconds).
const GLIDE: f64 = 0.15;
/// A source's occlusion is re-probed every this many frames (staggered by source).
const PROBE_EVERY: u64 = 4;

fn pose(object: ObjectId) -> Option<(Vec3, Quat, Vec3)> {
    with_scene(|s| {
        let t = s.world(object);
        (t.position, t.rotation, t.scale)
    })
}

fn position(object: ObjectId) -> [f64; 3] {
    pose(object).map_or([0.0; 3], |(p, _, _)| {
        [f64::from(p.x), f64::from(p.y), f64::from(p.z)]
    })
}

fn v3(p: [f64; 3]) -> V3 {
    V3 {
        x: p[0],
        y: p[1],
        z: p[2],
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn scale3(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// The occluder on an object or the nearest ancestor that has one.
fn occluder_of(
    mut object: ObjectId,
    specs: &HashMap<ObjectId, OccluderSpec>,
) -> Option<OccluderSpec> {
    for _ in 0..16 {
        if let Some(s) = specs.get(&object) {
            return Some(*s);
        }
        object = with_scene(|s| s.parent(object)).flatten()?;
    }
    None
}

/// The spec of every occluder, by object.
fn occluder_specs() -> HashMap<ObjectId, OccluderSpec> {
    st(|s| {
        s.occluders
            .iter()
            .map(|(_, o)| {
                (
                    o.object,
                    OccluderSpec {
                        openness: o.openness,
                        absorption: o.absorption,
                        low_pass: o.low_pass,
                    },
                )
            })
            .collect()
    })
}

/// What a straight line from `from` to `to` crosses, as a probe.
fn ray(
    from: [f64; 3],
    to: [f64; 3],
    specs: &HashMap<ObjectId, OccluderSpec>,
    layers: u32,
) -> Probe {
    // Anything on a probed layer without an occluder counts as an ordinary wall.
    const WALL: OccluderSpec = OccluderSpec {
        openness: 0.0,
        absorption: 0.5,
        low_pass: 1200.0,
    };
    let hits: Vec<OccluderSpec> = platypus_physics_api::segment_objects(v3(from), v3(to), u32::MAX)
        .into_iter()
        .filter_map(|o| {
            occluder_of(o, specs).or_else(|| {
                let layer = with_scene(|s| s.layer(o)).unwrap_or(0);
                (layers & (1u32 << layer) != 0).then_some(WALL)
            })
        })
        .collect();
    through(&hits)
}

/// What stands between the listener at `from` and a sound at `to`: the centre ray and four around
/// the sound (`spread` metres out), so a doorway the straight line misses still lets some in.
pub(crate) fn probe(
    from: [f64; 3],
    to: [f64; 3],
    specs: &HashMap<ObjectId, OccluderSpec>,
    layers: u32,
    spread: f64,
) -> Probe {
    let d = sub(to, from);
    let dist = len(d);
    if dist < 1e-3 {
        return Probe::CLEAR;
    }
    let dir = scale3(d, 1.0 / dist);
    let up = if dir[1].abs() > 0.95 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let right = cross(dir, up);
    let right = scale3(right, 1.0 / len(right).max(1e-9));
    let above = cross(right, dir);
    let reach = spread.min(dist * 0.5);
    let points = [
        to,
        add(to, scale3(right, reach)),
        add(to, scale3(right, -reach)),
        add(to, scale3(above, reach)),
        add(to, scale3(above, -reach)),
    ];
    let rays: Vec<Probe> = points
        .iter()
        .map(|p| ray(from, *p, specs, layers))
        .collect();
    average(&rays)
}

/// The most useful thing to send is a change: quantised so a drifting value does not flood the
/// mixer with commands.
fn changed(old: f64, new: f64, rel: f64) -> bool {
    (new - old).abs() > old.abs().max(new.abs()).max(1e-6) * rel
}

/// Sends an effect property of a track if it moved.
fn send(
    sent: &mut HashMap<(u32, &'static str), f64>,
    track: u32,
    property: &'static str,
    value: f64,
    open: f64,
    rel: f64,
) {
    let old = *sent.get(&(track, property)).unwrap_or(&open);
    if changed(old, value, rel) {
        set_fx(track, property, value, GLIDE);
        sent.insert((track, property), value);
    }
}

fn send_target(
    sent: &mut HashMap<(u32, &'static str), f64>,
    track: u32,
    t: &FxTarget,
    drive_volume: bool,
) {
    let o = FxTarget::OPEN;
    send(sent, track, "lowpass", t.low_pass, o.low_pass, 0.02);
    // The high-pass is open at 0 (relative changes mean nothing there).
    let hp_old = *sent.get(&(track, "highpass")).unwrap_or(&0.0);
    if (t.high_pass - hp_old).abs() > 5.0 + hp_old * 0.02 {
        set_fx(track, "highpass", t.high_pass, GLIDE);
        sent.insert((track, "highpass"), t.high_pass);
    }
    for (name, v, open) in [
        ("reverb", t.reverb, o.reverb),
        ("reverbSize", t.reverb_size, o.reverb_size),
        ("reverbDamping", t.reverb_damping, o.reverb_damping),
        ("echo", t.echo, o.echo),
    ] {
        let old = *sent.get(&(track, name)).unwrap_or(&open);
        if (v - old).abs() > 0.004 {
            set_fx(track, name, v, GLIDE);
            sent.insert((track, name), v);
        }
    }
    if drive_volume {
        let old = *sent.get(&(track, "volume")).unwrap_or(&1.0);
        if changed(old, t.volume, 0.02) {
            set_track_volume(Some(track), t.volume, GLIDE);
            sent.insert((track, "volume"), t.volume);
        }
    }
}

/// How strongly a zone applies at a world point (0 when its object is gone).
pub(crate) fn weight_at(z: &Zone, point: [f64; 3]) -> f64 {
    let Some((p, r, s)) = pose(z.object) else {
        return 0.0;
    };
    let rel = r.inverse() * (Vec3::new(point[0] as f32, point[1] as f32, point[2] as f32) - p);
    let local = [f64::from(rel.x), f64::from(rel.y), f64::from(rel.z)];
    let (sx, sy, sz) = (
        f64::from(s.x.abs()),
        f64::from(s.y.abs()),
        f64::from(s.z.abs()),
    );
    let extent = match z.shape {
        crate::model::Shape::Sphere => [z.radius * sx.max(sy).max(sz), 0.0, 0.0],
        crate::model::Shape::Box => [
            z.size[0] * 0.5 * sx,
            z.size[1] * 0.5 * sy,
            z.size[2] * 0.5 * sz,
        ],
    };
    zone_weight(z.shape, local, extent, z.inner, z.curve, z.invert)
}

/// How strongly each zone applies where the listener is.
fn zone_weights(zones: &[(u32, Zone)], listener: [f64; 3]) -> Vec<f64> {
    zones.iter().map(|(_, z)| weight_at(z, listener)).collect()
}

/// What `probe` needs: the occluders, the probed layers and the ray spread.
pub(crate) fn probe_settings() -> (HashMap<ObjectId, OccluderSpec>, u32, f64) {
    let (layers, spread) = st(|s| (s.settings.occlusion_layers, s.settings.ray_spread));
    (occluder_specs(), layers, spread)
}

/// How loud the world is at a point: playing loud sources and fresh noises that reach it.
pub(crate) fn level_at(point: [f64; 3]) -> f64 {
    let (specs, layers, spread) = probe_settings();
    let (events, time, lifetime) = st(|s| (s.events.clone(), s.time, s.settings.event_lifetime));
    let gain = |at: [f64; 3]| probe(at, point, &specs, layers, spread).gain;
    let mut level = 0.0f64;
    for v in source_views()
        .iter()
        .filter(|v| v.loudness > 0.0 && v.playing)
    {
        let at = position(v.object);
        let d = len(sub(at, point));
        if d < v.loudness {
            level = level.max(heard_level(
                v.loudness,
                d,
                gain(at) * v.volume.clamp(0.0, 1.0),
                1.0,
            ));
        }
    }
    for e in events.iter().filter(|e| !e.pulse) {
        let d = len(sub(e.position, point));
        if d < e.loudness {
            let fade = (1.0 - (time - e.born) / lifetime).clamp(0.0, 1.0);
            level = level.max(heard_level(e.loudness, d, gain(e.position), 1.0) * fade);
        }
    }
    level
}

fn update_zones(listener: [f64; 3]) {
    let zones: Vec<(u32, Zone)> = st(|s| {
        s.zones
            .iter()
            .filter(|(_, z)| z.enabled)
            .map(|(i, z)| (i, z.clone()))
            .collect()
    });
    let weights = zone_weights(&zones, listener);
    // Per track, the (priority, weight, target) of the zones that process it.
    let mut per_track: BTreeMap<u32, Vec<(f64, f64, FxTarget)>> = BTreeMap::new();
    for ((_, z), w) in zones.iter().zip(&weights) {
        let tracks: &[u32] = if z.buses.is_empty() { &[0] } else { &z.buses };
        for t in tracks {
            per_track
                .entry(*t)
                .or_default()
                .push((z.priority, *w, z.target));
        }
    }
    let ambience: Vec<(u32, Zone, f64)> = zones
        .iter()
        .zip(&weights)
        .map(|((i, z), w)| (*i, z.clone(), *w))
        .collect();
    st(|s| {
        for ((i, _), w) in zones.iter().zip(&weights) {
            if let Some(z) = s.zones.get_mut(*i) {
                z.weight = *w;
            }
        }
        // Tracks that lost their last zone go back to open.
        let now: Vec<u32> = per_track.keys().copied().collect();
        let gone: Vec<u32> = s
            .driven
            .iter()
            .copied()
            .filter(|t| !now.contains(t))
            .collect();
        for t in gone {
            send_target(&mut s.sent, t, &FxTarget::OPEN, t != 0);
        }
        for (track, mut list) in per_track {
            list.sort_by(|a, b| a.0.total_cmp(&b.0));
            let blended = blend_zones(&list.iter().map(|(_, w, t)| (*w, *t)).collect::<Vec<_>>());
            // The master's volume is the game's (`AudioListener.volume`): zones leave it.
            send_target(&mut s.sent, track, &blended, track != 0);
        }
        s.driven = now.into_iter().collect();
    });
    for (i, z, w) in ambience {
        update_ambience(i, &z, w);
    }
}

/// Starts, fades and pauses a zone's ambience bed with its weight.
fn update_ambience(id: u32, z: &Zone, weight: f64) {
    if !audio::sound_exists(z.ambience) {
        return;
    }
    let volume = (weight * z.ambience_volume).clamp(0.0, 4.0);
    let audible = volume > 0.002;
    let mut start = false;
    let mut resume = None;
    let mut pause = None;
    let mut set = None;
    st(|s| {
        let Some(zone) = s.zones.get_mut(id) else {
            return;
        };
        match (zone.playback, audible) {
            (None, true) => start = true,
            (Some(p), true) => {
                if zone.paused {
                    zone.paused = false;
                    resume = Some(p);
                }
                if changed(zone.sent_volume.max(0.0), volume, 0.03) || zone.sent_volume < 0.0 {
                    zone.sent_volume = volume;
                    set = Some(p);
                }
            }
            (Some(p), false) if !zone.paused => {
                zone.paused = true;
                zone.sent_volume = 0.0;
                pause = Some(p);
            }
            _ => {}
        }
    });
    if start {
        let p = play_sound(
            z.ambience,
            PlayOptions {
                volume,
                looped: true,
                fade_in: 0.4,
                bus: (z.ambience_bus != 0).then_some(BusRef(z.ambience_bus)),
                ..PlayOptions::default()
            },
        );
        st(|s| {
            if let Some(zone) = s.zones.get_mut(id) {
                zone.playback = Some(p);
                zone.paused = false;
                zone.sent_volume = volume;
            }
        });
    }
    if let Some(p) = resume {
        audio::resume_playback(p, 0.3);
    }
    if let Some(p) = set {
        audio::set_playback(p, "volume", volume, 0.2);
    }
    if let Some(p) = pause {
        audio::pause_playback(p, 0.4);
    }
}

/// Stops the ambience of a removed zone.
pub(crate) fn stop_ambience(p: PlaybackRef) {
    audio::stop_playback(p, 0.3);
}

fn update_sources(
    dt: f64,
    listener: [f64; 3],
    listener_forward: [f64; 3],
    listener_velocity: [f64; 3],
    views: &[SourceView],
    specs: &HashMap<ObjectId, OccluderSpec>,
) {
    let (settings, frame, any_occluder) = st(|s| {
        (
            s.settings,
            s.frame,
            s.occluders.iter().next().is_some() || s.settings.occlusion_layers != 0,
        )
    });
    let portals = portal_list();
    let source_zones = source_zone_list();
    for v in views {
        let pos = position(v.object);
        let state = st(|s| s.src.get(&v.source.0).copied().unwrap_or_default());
        let mut next = state;
        if next.gain == 0.0 {
            next.gain = 1.0;
            next.low_pass = 22000.0;
            next.doppler = 1.0;
            next.occ_gain = 1.0;
            next.occ_low_pass = 22000.0;
        }
        if let Some(emitter) = v.emitter
            && v.playing
        {
            // Occlusion, or the way round by a portal: re-probed every few frames.
            let due = !state.started
                || (frame + u64::from(v.source.0)).is_multiple_of(PROBE_EVERY)
                    && frame != state.checked;
            if any_occluder && due {
                let direct = scaled(
                    probe(
                        listener,
                        pos,
                        specs,
                        settings.occlusion_layers,
                        settings.ray_spread,
                    ),
                    v.occlusion,
                );
                next.checked = frame;
                next.started = true;
                let mut result = direct;
                let mut heard_from = None;
                if v.occlusion > 0.0
                    && direct.gain < 0.85
                    && let Some((round, at)) = best_detour(
                        listener,
                        pos,
                        &portals,
                        specs,
                        settings.occlusion_layers,
                        direct,
                    )
                {
                    result = scaled(round, v.occlusion);
                    heard_from = Some(at);
                }
                next.occ_gain = result.gain;
                next.occ_low_pass = result.low_pass;
                if differs(state.virtual_pos, heard_from) {
                    next.virtual_pos = heard_from;
                    set_virtual_position(v.source, heard_from);
                }
            }
            // The cues that change every frame: the head's shading of what is behind or
            // above, and the zones that colour sources where they stand.
            let apparent = next.virtual_pos.unwrap_or(pos);
            let to = sub(apparent, listener);
            let dist = len(to);
            let head = if dist > 1e-3 {
                head_cue(
                    listener_forward,
                    scale3(to, 1.0 / dist),
                    settings.head_shadow,
                )
            } else {
                22000.0
            };
            // Air: the highs fade with the distance the sound travelled (the way round, if by a portal).
            let air = air_cutoff(dist, settings.air_absorption * v.air_absorption);
            // The cone: how far the listener is from the way the source faces.
            let (cone_gain, cone_low) = if v.directivity != Directivity::Omni
                || v.cone_inner < 360.0
            {
                let facing = pose(v.object).map_or([0.0, 0.0, -1.0], |(_, r, _)| {
                    let f = r * Vec3::NEG_Z;
                    [f64::from(f.x), f64::from(f.y), f64::from(f.z)]
                });
                let away = sub(listener, pos);
                let d = len(away);
                let angle = if d > 1e-3 {
                    let c = (facing[0] * away[0] + facing[1] * away[1] + facing[2] * away[2]) / d;
                    c.clamp(-1.0, 1.0).acos().to_degrees()
                } else {
                    0.0
                };
                if v.directivity != Directivity::Omni {
                    pattern_effect(
                        pattern_gain(
                            v.directivity,
                            angle,
                            v.directivity_width,
                            v.directivity_rear,
                        ),
                        v.cone_outer_low_pass,
                        v.directivity_strength,
                    )
                } else {
                    cone_effect(
                        cone_amount(angle, v.cone_inner, v.cone_outer),
                        v.cone_outer_gain,
                        v.cone_outer_low_pass,
                    )
                }
            } else {
                (1.0, 22000.0)
            };
            let zone = source_zone_fx(&source_zones, pos);
            let mut target = if v.effects { zone } else { FxTarget::OPEN };
            target.low_pass = next
                .occ_low_pass
                .min(head)
                .min(zone.low_pass)
                .min(air)
                .min(cone_low);
            // Distance decay beyond the mixer's own (which is moved out of the way for these).
            let decay = if v.rolloff == Rolloff::Default {
                1.0
            } else {
                rolloff_gain(
                    v.rolloff,
                    dist,
                    v.min_distance,
                    v.max_distance,
                    v.rolloff_factor,
                )
            };
            target.volume = next.occ_gain * zone.volume * cone_gain * decay;
            st(|s| send_target(&mut s.sent, emitter.0, &target, true));
        }
        // Doppler: the pitch follows how fast the source closes on the listener.
        if v.doppler_level > 0.0 && v.emitter.is_some() {
            let velocity = st(|s| s.prev_pos.get(&v.object).copied())
                .map_or([0.0; 3], |p| scale3(sub(pos, p), 1.0 / dt));
            let to = sub(pos, listener);
            let dist = len(to);
            let target = if dist < 1e-3 {
                1.0
            } else {
                doppler(
                    scale3(to, 1.0 / dist),
                    velocity,
                    listener_velocity,
                    settings.speed_of_sound,
                    v.doppler_level,
                )
            };
            // Smoothed over a tenth of a second: a position that jumps does not click.
            let k = 1.0 - (-dt * 12.0).exp();
            next.doppler += (target - next.doppler) * k;
            set_doppler_factor(v.source, next.doppler);
        } else if (next.doppler - 1.0).abs() > 1e-4 {
            next.doppler = 1.0;
            set_doppler_factor(v.source, 1.0);
        }
        st(|s| {
            s.src.insert(v.source.0, next);
            if v.doppler_level > 0.0 {
                s.prev_pos.insert(v.object, pos);
            }
        });
    }
}

/// The enabled portals: where each is, and what it lets through.
fn portal_list() -> Vec<PortalAt> {
    let all: Vec<_> = st(|s| s.portals.iter().map(|(_, p)| *p).collect());
    all.into_iter()
        .filter(|p| p.enabled)
        .map(|p| PortalAt {
            at: position(p.object),
            gain: p.gain,
            radius: p.radius,
            low_pass: p.low_pass,
        })
        .collect()
}

struct PortalAt {
    at: [f64; 3],
    gain: f64,
    radius: f64,
    low_pass: f64,
}

/// The zones that colour sources too, lowest priority first.
fn source_zone_list() -> Vec<Zone> {
    let mut zones: Vec<Zone> = st(|s| {
        s.zones
            .iter()
            .filter(|(_, z)| z.enabled && z.affects_sources)
            .map(|(_, z)| z.clone())
            .collect()
    });
    zones.sort_by(|a, b| a.priority.total_cmp(&b.priority));
    zones
}

/// What the source zones make of a sound that stands at `pos`.
fn source_zone_fx(zones: &[Zone], pos: [f64; 3]) -> FxTarget {
    blend_zones(
        &zones
            .iter()
            .map(|z| (weight_at(z, pos), z.target))
            .collect::<Vec<_>>(),
    )
}

fn differs(a: Option<[f64; 3]>, b: Option<[f64; 3]>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(a), Some(b)) => len(sub(a, b)) > 0.05,
        _ => true,
    }
}

/// The best way round a wall by a portal, if it beats going through: what reaches the listener
/// by it, and where the sound is heard from (the portal's direction, the whole path away, so
/// the distance falloff fits).
fn best_detour(
    listener: [f64; 3],
    pos: [f64; 3],
    portals: &[PortalAt],
    specs: &HashMap<ObjectId, OccluderSpec>,
    layers: u32,
    direct: Probe,
) -> Option<(Probe, [f64; 3])> {
    let mut best: Option<(Probe, f64, [f64; 3])> = None;
    for p in portals {
        let to_portal = len(sub(p.at, listener));
        let from_portal = len(sub(pos, p.at));
        if to_portal < 1e-3 || from_portal < 1e-3 {
            continue;
        }
        let first = ray(listener, p.at, specs, layers);
        // The far leg starts a little short of the portal, so a door in it is counted once
        // (by the near leg, which runs into its middle).
        let start = add(
            p.at,
            scale3(
                sub(pos, p.at),
                p.radius.min(from_portal * 0.5) / from_portal,
            ),
        );
        let second = ray(start, pos, specs, layers);
        let (round, path) =
            through_portal(to_portal, from_portal, first, second, p.gain, p.low_pass);
        let better = best
            .as_ref()
            .is_none_or(|(b, bp, _)| round.gain / path.max(1.0) > b.gain / bp.max(1.0));
        if better {
            best = Some((round, path, scale3(sub(p.at, listener), 1.0 / to_portal)));
        }
    }
    let (round, path, dir) = best?;
    detour_wins(direct, len(sub(pos, listener)), round, path)
        .then(|| (round, add(listener, scale3(dir, path))))
}

/// Pulses from sources that are loud, so sensors and the radar see them.
fn pulse_sources(views: &[SourceView]) {
    let (time, interval) = st(|s| (s.time, s.settings.pulse_interval));
    for v in views {
        if v.loudness <= 0.0 || !v.playing {
            continue;
        }
        let due = st(|s| {
            let state = s.src.entry(v.source.0).or_default();
            if time - state.last_pulse >= interval {
                state.last_pulse = time;
                true
            } else {
                false
            }
        });
        if due {
            let kind = "source";
            push_event(position(v.object), v.loudness, kind, Some(v.object), true);
        }
    }
}

type Delivery = (Vec<crate::state::HearCallback>, u32, f64);

/// What each sensor hears: new noises (once each) and the level of what is playing.
fn update_sensors(
    dt: f64,
    views: &[SourceView],
    specs: &HashMap<ObjectId, OccluderSpec>,
) -> Vec<Delivery> {
    let (events, settings, any_occluder, sensors) = st(|s| {
        (
            s.events.clone(),
            s.settings,
            s.occluders.iter().next().is_some() || s.settings.occlusion_layers != 0,
            s.sensors
                .iter()
                .filter(|(_, x)| x.enabled)
                .map(|(i, x)| (i, x.object, x.sensitivity, x.threshold, x.occlusion))
                .collect::<Vec<_>>(),
        )
    });
    let mut deliveries = Vec::new();
    for (id, object, sensitivity, threshold, occlusion) in sensors {
        let here = position(object);
        let gain_to = |at: [f64; 3]| {
            if occlusion && any_occluder {
                probe(
                    here,
                    at,
                    specs,
                    settings.occlusion_layers,
                    settings.ray_spread,
                )
                .gain
            } else {
                1.0
            }
        };
        // Sounds that are playing.
        let mut level = 0.0f64;
        let mut hearing_now = Vec::new();
        for v in views.iter().filter(|v| v.loudness > 0.0 && v.playing) {
            let at = position(v.object);
            let dist = len(sub(at, here));
            if dist > v.loudness * sensitivity.max(0.0) {
                continue;
            }
            let l = heard_level(
                v.loudness,
                dist,
                gain_to(at) * v.volume.clamp(0.0, 1.0),
                sensitivity,
            );
            level = level.max(l);
            if l >= threshold {
                hearing_now.push(v.object);
            }
        }
        // Noises not heard yet.
        let mut fresh: Vec<(u32, f64)> = Vec::new();
        let (mut heard_ids, mut hearing_sources) = st(|s| {
            s.sensors
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, x)| (x.heard.clone(), x.hearing.clone()))
                .unwrap_or_default()
        });
        for e in &events {
            if heard_ids.contains(&e.id) {
                continue;
            }
            heard_ids.insert(e.id);
            let dist = len(sub(e.position, here));
            if dist > e.loudness * sensitivity.max(0.0) {
                continue;
            }
            let l = heard_level(e.loudness, dist, gain_to(e.position), sensitivity);
            if e.pulse {
                // A pulse wakes a sensor once, when the source starts to be heard.
                let src = e.source.map_or(0, |o| o.index + 1);
                if l >= threshold && hearing_sources.insert(src) {
                    fresh.push((e.id, l));
                }
            } else if l >= threshold {
                fresh.push((e.id, l));
            }
        }
        // A source that went quiet or far can wake the sensor again.
        let still: Vec<u32> = hearing_now.iter().map(|o| o.index + 1).collect();
        hearing_sources.retain(|s| still.contains(s));
        let callbacks = st(|s| {
            let Some(sensor) = s.sensors.iter_mut().find(|(i, _)| *i == id).map(|(_, x)| x) else {
                return Vec::new();
            };
            let spike_in = fresh.iter().fold(0.0f64, |m, (_, l)| m.max(*l));
            sensor.spike = (sensor.spike * (-dt * 2.5).exp()).max(spike_in);
            sensor.level = level.max(sensor.spike);
            // Event ids older than anything alive are forgotten.
            let alive: Vec<u32> = events.iter().map(|e| e.id).collect();
            heard_ids.retain(|i| alive.contains(i));
            sensor.heard = heard_ids;
            sensor.hearing = hearing_sources;
            sensor.callbacks.clone()
        });
        for (event, l) in fresh {
            deliveries.push((callbacks.clone(), event, l));
        }
    }
    deliveries
}

/// Runs one frame of acoustics: zones, occlusion, doppler, noise events and sensors. `dt` is the
/// frame's time in seconds. Call after scripts and transforms, before the host takes the audio
/// commands. Does nothing until something uses it (no zones, occluders, sensors or events and
/// no source asking for doppler).
pub fn update(dt: f64) {
    let dt = dt.clamp(1e-4, 0.25);
    let (enabled, any) = st(|s| {
        s.time += dt;
        s.frame += 1;
        let cutoff = s.time - s.settings.event_lifetime;
        s.events.retain(|e| e.born >= cutoff);
        (
            s.settings.enabled,
            s.zones.iter().next().is_some()
                || !s.driven.is_empty()
                || s.occluders.iter().next().is_some()
                || s.sensors.iter().next().is_some()
                || !s.events.is_empty()
                || s.settings.occlusion_layers != 0,
        )
    });
    let views = source_views();
    let doppler_used = views
        .iter()
        .any(|v| v.doppler_level > 0.0 || v.rolloff != Rolloff::Default);
    if !enabled || !(any || doppler_used || views.iter().any(|v| v.loudness > 0.0)) {
        return;
    }
    let (listener, listener_forward) =
        audio::listener().map_or(([0.0; 3], [0.0, 0.0, -1.0]), |(p, r)| {
            // The listener looks down its -z, as the camera does.
            let f =
                Quat::from_xyzw(r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32) * Vec3::NEG_Z;
            (p, [f64::from(f.x), f64::from(f.y), f64::from(f.z)])
        });
    let listener_velocity = st(|s| {
        if let Some(prev) = s.listener_prev {
            let v = scale3(sub(listener, prev), 1.0 / dt);
            // A little smoothing: the listener's position is a camera and can jitter.
            let k = 1.0 - (-dt * 20.0).exp();
            for (cur, new) in s.listener_velocity.iter_mut().zip(v) {
                *cur += (new - *cur) * k;
            }
        }
        s.listener_prev = Some(listener);
        s.listener_velocity
    });
    let specs = occluder_specs();
    update_zones(listener);
    update_sources(
        dt,
        listener,
        listener_forward,
        listener_velocity,
        &views,
        &specs,
    );
    // Emitters that were sent round a portal move now, not next frame.
    platypus_engine_api::audio_components::pre_render();
    pulse_sources(&views);
    let deliveries = update_sensors(dt, &views, &specs);
    // Callbacks run last, with no state borrowed: they may emit, move sensors or add zones.
    for (callbacks, event, level) in deliveries {
        for c in callbacks {
            c.call((HandleArg(SoundEventRef(event)), level));
        }
    }
}

/// The noise events alive now, oldest first.
pub(crate) fn events() -> Vec<Event> {
    st(|s| s.events.clone())
}
