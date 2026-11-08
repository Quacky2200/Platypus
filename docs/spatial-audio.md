# Spatial audio: effects, zones, occlusion, sensors, Doppler, the microphone

Crates: [platypus_audio](../crates/platypus_audio/src/lib.rs) (the mixer: `fx.rs` effect chains, `live.rs` live input),
[platypus_acoustics](../crates/platypus_acoustics/src/lib.rs) (the world-aware part), and the API declarations
`audio.rs` / `audio_components.rs` of `platypus_engine_api`. Examples: `audio-house`, `audio-radar`, `audio-cave`,
`audio-road` (sounds made by `tools/make_audio_demo_sounds.py`, nothing downloaded). Basics of the mixer are in
[physics-and-audio.md](physics-and-audio.md).

## Effect chains (mixer)

A bus or emitter made with effects owns kira filters; scripts drive them with `audio.fx` ops (the host applies them like
any `audio.*` op) and the track glides to the new value (`fade`, seconds).

| Track | Chain |
|---|---|
| Master (track `0`) | low-pass, high-pass, reverb, echo (always) |
| Bus made with `effects: true` | the same |
| Emitter | a low-pass (always); with `effects: true` also the rest |
| Bus or emitter made without | none (`fx` on it is an error) |

Properties: `lowpass` / `highpass` (Hz; 22000 and 0 are open and bypassed), `reverb` and `echo` (wet, 0 to 1),
`reverbSize`, `reverbDamping` (0 to 1). The echo repeats every 0.27 s at -9 dB. A property the chain lacks is ignored
(a zone can drive a mixed set). The cutoff is held under 45% of the sample rate: kira's filter blows up past Nyquist
(`Audio::set_sample_rate`, called for offline mixers).

Script side: `audio.createBus({ volume, parent, effects })`, `audio.createEmitter(pos, { minDistance, maxDistance, bus,
effects })`; `bus.setLowPass(hz, fade)`, `setHighPass`, `setReverb`, `setReverbSize`, `setReverbDamping`, `setEcho`
(an emitter has `setLowPass`, `setHighPass`, `setReverb`, `setEcho`). `AudioSource` gains `effects`, `dopplerLevel`,
`loudness`, `occlusion` and an `emitter` property.

## Zones (`AudioZone`)

A component whose volume (a `sphere` with `radius`, or a `box` with `size`, both times the object's scale) colours what the
**listener** hears by how deep inside the listener is. The gradient is the editor's control:

- `innerFraction` (0 to 1): how much of the volume is the full-strength core.
- `curve`: `linear`, `smooth`, `easeIn`, `easeOut` between the edge (0) and the core (1).
- `invert`: strongest outside (the nature a cave's exit lets in).
- Targets at full strength: `lowPass`, `highPass`, `reverb`, `reverbSize`, `reverbDamping`, `echo`, `volume`.
- `addBus(bus)` names the buses it processes; none means the master mix (its volume is the game's and zones leave it).
- `ambience` (a `Sound`), `ambienceVolume`, `ambienceBus`: a loop that fades in with the strength (started on first use,
  paused when inaudible).
- `priority`, `enabled`, `weight` (read: the strength at the listener), `weightAt(point)`.

Overlapping zones blend per track from "open", lowest `priority` first, each pulling by its weight; frequencies blend on a
log scale. Only changes are sent to the mixer; a track whose last zone goes away returns to open.

## Occlusion (`AudioOccluder`)

Walls, doors and floors carry an `AudioOccluder` (`openness` 0 shut to 1 open, `absorption`, `lowPass`) and a collider.
Each frame (each source every fourth frame) acoustics casts rays **from the listener** to the source and four points
around it (`Acoustics.raySpread`, 0.4 m), asks `platypus_physics_api::segment_objects` which objects' colliders they cross
(the listener side first, because a ray does not hit a collider it starts inside) and combines what it finds: gains
multiply, cutoff ratios multiply, down to 180 Hz. Averaging the rays lets some sound through a doorway the centre ray
misses. The result sets the source's emitter low-pass and track volume. `AudioSource.occlusion` (0 to 1) asks for less.
`Acoustics.occlusionLayers` makes plain colliders of those layers count as ordinary walls. `Acoustics.transmission(from, to)`
answers the same question for scripts (1 clear, 0 blocked).

Floors are occluders too; the stairwell is a hole in them, so a sound upstairs is dull through the floor and clearer
where the rays find the opening (`audio-house`).

## Round corners: portals (`AudioPortal`)

A portal is an object standing in an opening (a doorway, a window, a stairwell). When occlusion finds a source dulled below
85% and portals exist, acoustics tries each: a ray from the listener to the portal (running into a door standing in it, so
the door counts), and one from a little short of the portal (`radius`, 0.25 m) to the source. The way round is the product
of the two legs and the portal's `gain`, with the cutoffs multiplied and the portal's own `lowPass` (9000 Hz: edges take
the highs off); its length is the two legs added. It is used when gain over length beats the straight way's by 10%. Then
the source's emitter is moved (`audio_components::set_virtual_position`, gliding over 0.12 s) to the portal's direction at the
whole path's distance, so it is heard from the opening and fades as the walk round would. Closing a portal (`gain` 0 or
`enabled` false) puts the source back behind its wall. Only the first hop is tried.

## Head cue and sources in zones

`Acoustics.headShadow` (0 to 1, default 0.5) low-passes a source by where it is: directly behind the listener down to 4.5 kHz,
straight above or below to 9 kHz, at full strength. Panning cannot tell front from back or up from down; dullness can.

A zone with `affectsSources` also blends its target (by the weight at each source's place, priority order) into the emitters
of `AudioSource`s: low-pass and volume on any emitter, reverb, echo and high-pass on those made with `effects: true`. This is
combined with occlusion and the head cue (the lowest cutoff wins, gains multiply) and sent as one change per property.

## Noises and sensors (`Acoustics`, `SoundSensor`)

`Acoustics.emit(position, loudness, kind, source?)` makes a noise (loudness is the reach in metres). Playing `AudioSource`s
with `loudness` > 0 make a `source` pulse every `pulseInterval` s. Noises live `eventLifetime` s in `Acoustics.eventCount`
/ `eventAt(i)` (a `SoundEvent`: `position`, `kind`, `loudness`, `age`, `source`, `pulse`), for radars and maps.

A `SoundSensor` (`sensitivity`, `threshold`, `occlusion`) hears each non-pulse noise once, scaled by distance squared and
the walls' gain, and calls `onHear(event, level)`; it hears a pulsing source once when it becomes audible (again after it
went quiet). `sensor.level` is the loudest thing it hears now (noises spike it, then it falls). `Acoustics.levelAt(point)`
gives the loudness of the world at a point, for heat maps. Ground rules: sensors measure straight-line distance and the
occluders between, not paths around corners.

## Doppler

`AudioSource.dopplerLevel` (0 off, 1 physical) shifts the pitch of everything the source plays by its speed towards or
away from the listener (`Acoustics.speedOfSound`, 343 m/s; speeds come from per-frame movement, smoothed over ~0.1 s).
The default is 0 (Unity's is 1): nothing changes for existing games.

## The microphone

`Microphone.start({ bus, emitter, volume })` plays the default input device through the mixer as a `Playback`, on a bus
(which zones and filters then colour), an emitter (a place) or the main mix; `stop()`, `level`, `running`, `error`. cpal
captures on its own thread into a ring buffer (`LiveFeed`); a kira sound reads it with 60 ms of latency, resampling from the
device rate. Use headphones: nothing prevents feedback. There is no VoIP: `MediaStream` tracks are web-ui's job; the same
bus and zones would colour a remote voice once it can be played through the mixer.

## The host

`platypus_acoustics::update(dt)` runs once a frame in `Native::pre_render`, after `audio_components::pre_render`
and before the audio commands are taken; `register_kinds` makes the three components scene-file kinds (and registers
Inspector fields, the zone's gradient controls among them); `object_destroyed` cleans up; `reset` is for new games.

## Tests

`ACO-FX-*` (mixer effects, `crates/platypus_audio/tests`), `ACO-LIVE-01`, `ACO-MOD-*` (the maths, `model.rs`),
`ACO-SYS-*` (scene, physics and commands together, `crates/platypus_acoustics/src/tests.rs`).

## Known limits

- Sound bends round a wall only through an `AudioPortal` you place, one hop (listener to portal to source): no path
  search, no two-door routes, and there is no early-reflection or room-size model beyond the zone's reverb settings.
- No HRTF: panning is left to right, and front/back and up/down are told only by the head cue's dullness
  (`Acoustics.headShadow`); there are no per-ear delays or spectral notches.
- A zone colours sources only when `affectsSources` is set, and only emitters made with `effects: true` take its reverb
  and echo (the low-pass and volume apply to any emitter); it does not move the sound, and a source inside a zone does
  not echo for another zone's listener.
- The microphone has no echo cancellation, device choice or input gain beyond its `volume` yet.
