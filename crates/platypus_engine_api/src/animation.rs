// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The script API of animation: `Keyframe`, `AnimationCurve` and `AnimationClip` over
//! `platypus_animation`. The design and what differs from `js/engine/src/animation.ts` is
//! `docs/research/platypus-native-animation.md`.
//!
//! Differences so far: a curve is built key by key (`fromPoints` waits for arrays), a clip's
//! target is a class name string (`"Light"`), an event parameter is a number, and `toJSON()` is
//! text (`toJson()`), with `AnimationClip.fromJson(text)`.

use platypus_animation::{
    Binding, ClipEvent, ClipId, Curve, CurveId, Keyframe, NoComponents, Parameter, WrapMode,
    with_world,
};
use platypus_scene::ObjectId;
use ts_api::{Handle, ts_api};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurveRef(pub u64);

impl Handle for CurveRef {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        CurveRef(raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipRef(pub u64);

impl Handle for ClipRef {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        ClipRef(raw)
    }
}

impl CurveRef {
    fn id(self) -> CurveId {
        CurveId(self.0 as u32)
    }
}

impl ClipRef {
    fn id(self) -> ClipId {
        ClipId(self.0 as u32)
    }
}

fn zero_if_nan(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x }
}

fn new_curve(c: Curve) -> CurveRef {
    CurveRef(u64::from(with_world(|w| w.add_curve(c)).0))
}

#[ts_api(namespace = "Keyframe", values(
    Keyframe = Keyframe [time: f64, value: f64, inTangent: f64, outTangent: f64]))]
pub mod keyframe_api {
    use super::{Keyframe, zero_if_nan};

    /// One key on a curve: tangents are slopes (value per second); an infinite tangent holds a
    /// value (a step).
    #[optional(in_tangent, out_tangent)]
    pub fn create(time: f64, value: f64, in_tangent: f64, out_tangent: f64) -> Keyframe {
        Keyframe {
            time,
            value,
            in_tangent: zero_if_nan(in_tangent),
            out_tangent: zero_if_nan(out_tangent),
        }
    }
}

#[ts_api(namespace = "AnimationCurve", handles(AnimationCurve = CurveRef), values(
    Keyframe = Keyframe [time: f64, value: f64, inTangent: f64, outTangent: f64],
    Vector2 = V2 [x: f64, y: f64]))]
pub mod animation_curve_api {
    use super::{Curve, CurveRef, Keyframe, WrapMode, new_curve, with_world, zero_if_nan};
    use platypus_animation::PointMode;
    use platypus_math::V2;

    impl CurveRef {
        /// An empty curve (add keys with `addKey`).
        pub fn create() -> CurveRef {
            new_curve(Curve::default())
        }

        /// A straight line from (t0, v0) to (t1, v1).
        pub fn linear(t0: f64, v0: f64, t1: f64, v1: f64) -> CurveRef {
            new_curve(Curve::linear(t0, v0, t1, v1))
        }

        /// From (t0, v0) to (t1, v1), easing in and out (flat tangents).
        pub fn ease_in_out(t0: f64, v0: f64, t1: f64, v1: f64) -> CurveRef {
            new_curve(Curve::ease_in_out(t0, v0, t1, v1))
        }

        /// `value` from t0 to t1.
        pub fn constant(t0: f64, t1: f64, value: f64) -> CurveRef {
            new_curve(Curve::constant(t0, t1, value))
        }

        /// Keys through points (x is the time, y the value; `[time, value]` pairs work too) with
        /// tangents made to match: `"linear"` (the default), `"smooth"` or `"step"`.
        #[optional(mode)]
        #[ts_param(points = "readonly (Vector2Like | readonly [number, number])[]")]
        pub fn from_points(points: &[V2], mode: &str) -> CurveRef {
            let pts: Vec<(f64, f64)> = points.iter().map(|p| (p.x, p.y)).collect();
            let mode = if mode.is_empty() {
                PointMode::Linear
            } else {
                PointMode::parse(mode).unwrap_or(PointMode::Linear)
            };
            new_curve(Curve::from_points(&pts, mode))
        }

        /// Adds a key (flat tangents), keeping them in time order; returns its index. For
        /// tangents add a `Keyframe` with `addKeyframe`.
        #[optional(value)]
        pub fn add_key(self, time: f64, value: f64) -> f64 {
            let key = Keyframe::new(time, zero_if_nan(value));
            with_world(|w| {
                w.curve_mut(self.id())
                    .map_or(-1.0, |c| c.add_key(key) as f64)
            })
        }

        /// Adds a ready key; returns its index.
        pub fn add_keyframe(self, key: Keyframe) -> f64 {
            with_world(|w| {
                w.curve_mut(self.id())
                    .map_or(-1.0, |c| c.add_key(key) as f64)
            })
        }

        pub fn remove_key(self, index: f64) {
            with_world(|w| {
                if let Some(c) = w.curve_mut(self.id())
                    && index >= 0.0
                {
                    c.remove_key(index as usize);
                }
            });
        }

        /// The number of keys.
        #[property]
        pub fn length(self) -> f64 {
            with_world(|w| w.curve(self.id()).map_or(0.0, |c| c.keys.len() as f64))
        }

        /// The i-th key (a zero key past the end).
        pub fn key(self, index: f64) -> Keyframe {
            with_world(|w| {
                w.curve(self.id())
                    .and_then(|c| c.keys.get(index as usize).copied())
                    .unwrap_or(Keyframe::new(0.0, 0.0))
            })
        }

        /// The curve's value at `time`.
        pub fn evaluate(self, time: f64) -> f64 {
            with_world(|w| w.curve(self.id()).map_or(0.0, |c| c.evaluate(time)))
        }

        /// `once`, `loop`, `pingPong` or `clampForever` (the default): before the first key.
        #[property]
        pub fn pre_wrap_mode(self) -> String {
            with_world(|w| {
                w.curve(self.id())
                    .map_or(WrapMode::ClampForever, |c| c.pre_wrap)
                    .name()
                    .to_string()
            })
        }

        pub fn set_pre_wrap_mode(self, mode: &str) {
            if let Some(m) = WrapMode::parse(mode) {
                with_world(|w| {
                    if let Some(c) = w.curve_mut(self.id()) {
                        c.pre_wrap = m;
                    }
                });
            }
        }

        /// After the last key.
        #[property]
        pub fn post_wrap_mode(self) -> String {
            with_world(|w| {
                w.curve(self.id())
                    .map_or(WrapMode::ClampForever, |c| c.post_wrap)
                    .name()
                    .to_string()
            })
        }

        pub fn set_post_wrap_mode(self, mode: &str) {
            if let Some(m) = WrapMode::parse(mode) {
                with_world(|w| {
                    if let Some(c) = w.curve_mut(self.id()) {
                        c.post_wrap = m;
                    }
                });
            }
        }
    }
}

#[ts_api(namespace = "AnimationClip", handles(
    AnimationClip = ClipRef, AnimationCurve = CurveRef, GameObject = ObjectId))]
pub mod animation_clip_api {
    use super::{
        Binding, ClipEvent, ClipRef, NoComponents, ObjectId, Parameter, WrapMode, with_world,
    };

    impl ClipRef {
        /// A clip with a name (`"clip"` without one).
        #[optional(name)]
        pub fn create(name: &str) -> ClipRef {
            let n = if name.is_empty() { "clip" } else { name };
            ClipRef(u64::from(
                with_world(|w| w.add_clip(platypus_animation::clip::Clip::new(n))).0,
            ))
        }

        #[property]
        pub fn name(self) -> String {
            with_world(|w| {
                w.clip(self.id())
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            })
        }

        pub fn set_name(self, name: &str) {
            with_world(|w| {
                if let Some(c) = w.clip_mut(self.id()) {
                    c.name = name.to_string();
                }
            });
        }

        /// What happens past the end when played (`once` stops at the end, holding it).
        #[property]
        pub fn wrap_mode(self) -> String {
            with_world(|w| {
                w.clip(self.id())
                    .map_or(WrapMode::Once, |c| c.wrap_mode)
                    .name()
                    .to_string()
            })
        }

        pub fn set_wrap_mode(self, mode: &str) {
            if let Some(m) = WrapMode::parse(mode) {
                with_world(|w| {
                    if let Some(c) = w.clip_mut(self.id()) {
                        c.wrap_mode = m;
                    }
                });
            }
        }

        #[property]
        pub fn frame_rate(self) -> f64 {
            with_world(|w| w.clip(self.id()).map_or(60.0, |c| c.frame_rate))
        }

        pub fn set_frame_rate(self, rate: f64) {
            with_world(|w| {
                if let Some(c) = w.clip_mut(self.id()) {
                    c.frame_rate = rate;
                }
            });
        }

        /// The last key's time over all curves (or the last event's).
        #[property]
        pub fn length(self) -> f64 {
            with_world(|w| w.clip_length(self.id()))
        }

        /// Sets the curve of one property: `path` is "" for the animated object or "Arm/Hand" for
        /// a descendant by names; `type` is `Transform`, `GameObject` (`active`) or a component
        /// class name; Transform properties are `localPosition.x|y|z`, `localScale.x|y|z`,
        /// `localRotation.x|y|z|w` and `localEulerAngles.x|y|z`.
        pub fn set_curve(
            self,
            path: &str,
            type_name: &str,
            property: &str,
            curve: super::CurveRef,
        ) {
            with_world(|w| w.set_curve(self.id(), path, type_name, property, Some(curve.id())));
        }

        /// Removes the curve of one property.
        pub fn remove_curve(self, path: &str, type_name: &str, property: &str) {
            with_world(|w| w.set_curve(self.id(), path, type_name, property, None));
        }

        /// How many curves the clip has.
        #[property]
        pub fn curve_count(self) -> f64 {
            with_world(|w| w.clip(self.id()).map_or(0.0, |c| c.bindings.len() as f64))
        }

        /// The i-th curve's path, type and property, joined with `|` ("" past the end).
        pub fn curve_at(self, index: f64) -> String {
            with_world(|w| {
                w.clip(self.id())
                    .and_then(|c| c.bindings.get(index as usize))
                    .map(|b: &Binding| format!("{}|{}|{}", b.path, b.type_name, b.property))
                    .unwrap_or_default()
            })
        }

        /// The curve of one property (no curve, a zero handle, when there is none).
        pub fn curve(self, path: &str, type_name: &str, property: &str) -> super::CurveRef {
            with_world(|w| {
                w.clip(self.id())
                    .and_then(|c| {
                        c.bindings.iter().find(|b| {
                            b.path == path && b.type_name == type_name && b.property == property
                        })
                    })
                    .map_or(super::CurveRef(0), |b| {
                        super::CurveRef(u64::from(b.curve.0))
                    })
            })
        }

        /// Calls a method named `function_name` on the animated object's components when the
        /// clip passes `time`, with a number if one is given.
        #[optional(parameter)]
        pub fn add_event(self, time: f64, function_name: &str, parameter: f64) {
            let parameter = (!parameter.is_nan()).then_some(Parameter::Num(parameter));
            with_world(|w| {
                w.add_event(
                    self.id(),
                    ClipEvent {
                        time,
                        function_name: function_name.to_string(),
                        parameter,
                    },
                )
            });
        }

        #[property]
        pub fn event_count(self) -> f64 {
            with_world(|w| w.clip(self.id()).map_or(0.0, |c| c.events.len() as f64))
        }

        /// Poses `go` as the clip is at `time` (Unity's `SampleAnimation`).
        pub fn sample_animation(self, go: ObjectId, time: f64) {
            with_world(|w| {
                platypus_scene::api::with_scene(|scene| {
                    platypus_animation::sample(scene, w, &mut NoComponents, go, self.id(), time)
                })
            });
        }

        /// The clip as JSON text (for files and scenes).
        pub fn to_json(self) -> String {
            with_world(|w| w.clip_to_json(self.id()).to_string())
        }

        /// A clip from the JSON `toJson` wrote (an empty clip if the text is not JSON).
        pub fn from_json(text: &str) -> ClipRef {
            let data = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
            ClipRef(u64::from(with_world(|w| w.clip_from_json(&data)).0))
        }
    }
}

// ---- the Animation component ----

use platypus_animation::{AnimationPlayer, RegistrySink, register_setter, register_type};
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationRef(pub u64);

impl Handle for AnimationRef {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        AnimationRef(raw)
    }
}

/// A state of an `Animation`: the player number in the high half, the state index plus one below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateRef(pub u64);

impl Handle for StateRef {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        StateRef(raw)
    }
}

impl AnimationRef {
    fn index(self) -> usize {
        (self.0 as usize).wrapping_sub(1)
    }
}

impl StateRef {
    fn parts(self) -> (usize, usize) {
        (
            ((self.0 >> 32) as usize).wrapping_sub(1),
            ((self.0 & 0xffff_ffff) as usize).wrapping_sub(1),
        )
    }
}

fn player<R>(
    a: AnimationRef,
    f: impl FnOnce(&mut AnimationPlayer, &platypus_animation::World) -> R,
) -> Option<R> {
    with_world(|w| {
        let mut p = w.players.get(a.index())?.clone();
        let r = f(&mut p, w);
        w.players[a.index()] = p;
        Some(r)
    })
}

fn state<R>(
    s: StateRef,
    f: impl FnOnce(&mut platypus_animation::AnimState, &platypus_animation::World) -> R,
) -> Option<R> {
    let (pi, si) = s.parts();
    with_world(|w| {
        let mut p = w.players.get(pi)?.clone();
        let r = f(p.states.get_mut(si)?, w);
        w.players[pi] = p;
        Some(r)
    })
}

thread_local! {
    /// The clip events that came due and were not taken yet.
    static EVENTS: RefCell<Vec<platypus_animation::FiredEvent>> = const { RefCell::new(Vec::new()) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Keeps clip events that came due outside the frame's update (`Animator.update`) for
/// `Animations.takeEvents()`.
pub(crate) fn keep_events(events: &[platypus_animation::FiredEvent]) {
    EVENTS.with(|e| e.borrow_mut().extend(events.iter().cloned()));
}

/// Makes the engine components animatable (their number properties).
pub(crate) fn ensure_registered() {
    if REGISTERED.with(Cell::get) {
        return;
    }
    REGISTERED.with(|r| r.set(true));
    use crate::camera::{camera_on, with_camera};
    register_type("Camera", |o| camera_on(o).is_some());
    macro_rules! cam {
        ($prop:literal, |$d:ident, $v:ident| $body:expr) => {
            register_setter("Camera", $prop, |o, $v| {
                if let Some(c) = camera_on(o) {
                    with_camera(c, |$d| $body);
                }
            });
        };
    }
    cam!("fieldOfView", |d, v| d.fov = (v as f32).clamp(1.0, 179.0));
    cam!("nearClipPlane", |d, v| d.near = v as f32);
    cam!("farClipPlane", |d, v| d.far = v as f32);
    cam!("depth", |d, v| d.depth = v as f32);
    cam!("orthographicSize", |d, v| d.orthographic_size = v as f32);
    cam!("orthographic", |d, v| d.orthographic = v > 0.5);
    cam!("backgroundColor.r", |d, v| d.background.r = v);
    cam!("backgroundColor.g", |d, v| d.background.g = v);
    cam!("backgroundColor.b", |d, v| d.background.b = v);
    cam!("backgroundColor.a", |d, v| d.background.a = v);
}

/// Advances every `Animation` by `dt` after the component updates and poses the objects. The clip
/// events that came due are returned (and kept for `Animations.takeEvents()`): the host delivers
/// them to the components of the object. `other` finds the rest of the components (the compiled
/// ones of a game).
pub fn update(
    dt: f64,
    other: Option<&mut dyn platypus_animation::PropertySink>,
) -> Vec<platypus_animation::FiredEvent> {
    update_culled(dt, other, &mut platypus_animation::AllVisible)
}

/// [`update`], with `visible` saying which animated objects were seen last frame (for those whose
/// `cullingMode` / `cullingType` culls).
pub fn update_culled(
    dt: f64,
    other: Option<&mut dyn platypus_animation::PropertySink>,
    visible: &mut dyn platypus_animation::Visibility,
) -> Vec<platypus_animation::FiredEvent> {
    ensure_registered();
    let mut sink = RegistrySink { other };
    let events = with_world(|w| {
        platypus_scene::api::with_scene(|scene| {
            platypus_animation::update_culled(w, scene, &mut sink, dt, visible)
        })
    })
    .unwrap_or_default();
    EVENTS.with(|e| e.borrow_mut().extend(events.iter().cloned()));
    events
}

/// The objects whose `Animator` or `Animation` may be culled (the host watches their renderers).
pub fn culled_roots() -> Vec<ObjectId> {
    with_world(|w| platypus_animation::culled_roots(w))
}

/// Forgets every animation object and pending event (a test, or a new game).
pub fn reset() {
    platypus_animation::clip::reset();
    EVENTS.with(|e| e.borrow_mut().clear());
}

#[ts_api(namespace = "Animation", handles(
    Animation = AnimationRef, AnimationState = StateRef, AnimationClip = ClipRef, GameObject = ObjectId),
    enums(
        /// What an `Animation` does while none of its object's renderers was drawn (last frame):
        /// `alwaysAnimate` (the default) or `basedOnRenderers` (paused until seen again).
        AnimationCullingType = CullingType [AlwaysAnimate, BasedOnRenderers]
    ))]
pub mod animation_component_api {
    use super::{
        AnimationPlayer, AnimationRef, ClipId, ClipRef, ObjectId, StateRef, player, with_world,
    };
    use platypus_animation::CullingType;

    impl AnimationRef {
        /// Adds an `Animation` to a GameObject (what `go.addComponent(Animation)` does).
        pub fn attach(object: ObjectId) -> AnimationRef {
            AnimationRef(with_world(|w| {
                w.players.push(AnimationPlayer::new(object));
                w.players.len() as u64
            }))
        }

        /// The `Animation` on an object, or none (what `go.getComponent(Animation)` does).
        pub fn find_on(object: ObjectId) -> AnimationRef {
            AnimationRef(with_world(|w| {
                w.players
                    .iter()
                    .position(|p| p.object == object)
                    .map_or(0, |i| i as u64 + 1)
            }))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            player(self, |p, _| p.object).unwrap_or(ObjectId {
                index: u32::MAX,
                generation: u32::MAX,
            })
        }

        /// The default clip (it plays by itself when `playAutomatically`).
        #[property]
        pub fn clip(self) -> ClipRef {
            player(self, |p, _| {
                p.clip.map_or(ClipRef(0), |c| ClipRef(u64::from(c.0)))
            })
            .unwrap_or(ClipRef(0))
        }

        pub fn set_clip(self, clip: ClipRef) {
            player(self, |p, _| {
                p.clip = (clip.0 != 0).then_some(ClipId(clip.0 as u32))
            });
        }

        #[property]
        pub fn play_automatically(self) -> bool {
            player(self, |p, _| p.play_automatically).unwrap_or(false)
        }

        pub fn set_play_automatically(self, on: bool) {
            player(self, |p, _| p.play_automatically = on);
        }

        /// Whether it pauses while its renderers are not seen (Unity's `cullingType`).
        #[property]
        pub fn culling_type(self) -> CullingType {
            player(self, |p, _| p.culling_type).unwrap_or_default()
        }

        pub fn set_culling_type(self, culling: CullingType) {
            player(self, |p, _| p.culling_type = culling);
        }

        /// How many clips were added.
        #[property]
        pub fn clip_count(self) -> f64 {
            player(self, |p, _| p.clips.len() as f64).unwrap_or(0.0)
        }

        /// The i-th clip added (no clip past the end).
        pub fn clip_at(self, index: f64) -> ClipRef {
            player(self, |p, _| {
                p.clips
                    .get(index as usize)
                    .map_or(ClipRef(0), |c| ClipRef(u64::from(c.0)))
            })
            .unwrap_or(ClipRef(0))
        }

        /// Adds a clip under a name (its own by default).
        #[optional(name)]
        pub fn add_clip(self, clip: ClipRef, name: &str) {
            let name = (!name.is_empty()).then_some(name);
            player(self, |p, w| p.add_clip(w, ClipId(clip.0 as u32), name));
        }

        /// The state of a clip by name (no state if there is none).
        pub fn state(self, name: &str) -> StateRef {
            let found = player(self, |p, w| p.state_index(w, name)).flatten();
            match found {
                Some(i) => StateRef((self.0 << 32) | (i as u64 + 1)),
                None => StateRef(0),
            }
        }

        /// Plays a clip from the start at full weight, stopping the others. False if unknown.
        #[optional(name)]
        pub fn play(self, name: &str) -> bool {
            let name = (!name.is_empty()).then_some(name);
            player(self, |p, w| p.play(w, name)).unwrap_or(false)
        }

        /// Fades a clip in over `fadeLength` seconds (0.3 by default) while the others fade out.
        #[optional(fade_length)]
        pub fn cross_fade(self, name: &str, fade_length: f64) {
            let fade = if fade_length.is_nan() {
                0.3
            } else {
                fade_length
            };
            player(self, |p, w| p.cross_fade(w, name, fade));
        }

        /// Stops a clip, or all of them without a name.
        #[optional(name)]
        pub fn stop(self, name: &str) {
            let name = (!name.is_empty()).then_some(name);
            player(self, |p, _| p.stop(name));
        }

        /// Is a clip, or any clip without a name, playing?
        #[optional(name)]
        pub fn is_playing(self, name: &str) -> bool {
            let name = (!name.is_empty()).then_some(name);
            player(self, |p, _| p.is_playing(name)).unwrap_or(false)
        }

        /// Removes it from its object: it stops posing it (another component, an `Animator`,
        /// can take over).
        pub fn destroy(self) {
            player(self, |p, _| {
                // Players are numbered by place, so the slot stays, empty and on no object.
                *p = AnimationPlayer::new(ObjectId {
                    index: u32::MAX,
                    generation: u32::MAX,
                });
                p.play_automatically = false;
            });
        }
    }
}

#[ts_api(namespace = "AnimationState", handles(AnimationState = StateRef, AnimationClip = ClipRef))]
pub mod animation_state_api {
    use super::{ClipRef, StateRef, state};
    use platypus_animation::WrapMode;

    impl StateRef {
        #[property]
        pub fn name(self) -> String {
            state(self, |s, _| s.name.clone()).unwrap_or_default()
        }

        #[property]
        pub fn clip(self) -> ClipRef {
            state(self, |s, _| ClipRef(u64::from(s.clip.0))).unwrap_or(ClipRef(0))
        }

        /// Seconds into the clip.
        #[property]
        pub fn time(self) -> f64 {
            state(self, |s, _| s.time).unwrap_or(0.0)
        }

        pub fn set_time(self, v: f64) {
            state(self, |s, _| s.time = v);
        }

        #[property]
        pub fn speed(self) -> f64 {
            state(self, |s, _| s.speed).unwrap_or(1.0)
        }

        pub fn set_speed(self, v: f64) {
            state(self, |s, _| s.speed = v);
        }

        #[property]
        pub fn weight(self) -> f64 {
            state(self, |s, _| s.weight).unwrap_or(0.0)
        }

        pub fn set_weight(self, v: f64) {
            state(self, |s, _| s.weight = v);
        }

        #[property]
        pub fn enabled(self) -> bool {
            state(self, |s, _| s.enabled).unwrap_or(false)
        }

        pub fn set_enabled(self, v: bool) {
            state(self, |s, _| s.enabled = v);
        }

        #[property]
        pub fn wrap_mode(self) -> String {
            state(self, |s, _| s.wrap_mode.name().to_string()).unwrap_or_default()
        }

        pub fn set_wrap_mode(self, mode: &str) {
            if let Some(m) = WrapMode::parse(mode) {
                state(self, |s, _| s.wrap_mode = m);
            }
        }

        #[property]
        pub fn length(self) -> f64 {
            state(self, |s, w| s.length(w)).unwrap_or(0.0)
        }

        /// Time over length (past 1 after a loop).
        #[property]
        pub fn normalized_time(self) -> f64 {
            state(self, |s, w| {
                let l = s.length(w);
                if l > 0.0 { s.time / l } else { 0.0 }
            })
            .unwrap_or(0.0)
        }

        pub fn set_normalized_time(self, t: f64) {
            state(self, |s, w| s.time = t * s.length(w));
        }
    }
}

#[ts_api(namespace = "Animations")]
pub mod animations_api {
    use super::{EVENTS, update};

    /// Advances every `Animation` by `dt` and poses the objects (the host does this each frame
    /// after the component updates; this is for tools and tests).
    pub fn update_all(dt: f64) {
        update(dt, None);
    }

    /// The clip events that came due since the last call, one per line as
    /// `objectName|functionName|parameter`.
    pub fn take_events() -> String {
        let events = EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()));
        let lines: Vec<String> = events
            .iter()
            .map(|e| {
                let name =
                    platypus_scene::api::with_scene(|s| s.name(e.object).unwrap_or("").to_string())
                        .unwrap_or_default();
                let param = match &e.parameter {
                    Some(platypus_animation::Parameter::Num(n)) => {
                        platypus_math::jsnum::to_string(*n)
                    }
                    Some(platypus_animation::Parameter::Str(s)) => s.clone(),
                    Some(platypus_animation::Parameter::Bool(b)) => b.to_string(),
                    None => String::new(),
                };
                format!("{name}|{}|{param}", e.function_name)
            })
            .collect();
        lines.join("\n")
    }
}
