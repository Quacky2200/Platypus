// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Spline` (a path through knots, `platypus_math::spline`) and `SplineAnimate` (a component that moves
//! its object along one). A `Spline` is a handle to a spline kept here per thread, so compiled
//! scripts and the engine use the same code; a scene file saves one inline (`{"$type":"Spline", ..}`) inside the
//! `SplineAnimate` that uses it. The host calls [`update`] once a frame next to
//! `animation::update`; [`reset`] forgets everything (a reload).

use platypus_kinds::{AssetKind, Kind, asset_handle, asset_ref, object_ref};
use platypus_math::spline::{
    Alignment, Knot, LoopMode, Spline, SplineHit, SplineNearest, Tangent, animate_pose, animate_t,
};
use platypus_math::{Q, V3, v3};
use platypus_scene::ObjectId;
use platypus_scene::api::{TransformRef, scene_api, with_scene};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use ts_api::{Handle, ts_api};

macro_rules! numbered_handle {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub u32);

        impl Handle for $name {
            fn to_raw(self) -> u64 {
                u64::from(self.0)
            }
            fn from_raw(raw: u64) -> Self {
                $name(raw as u32)
            }
        }
    };
}

numbered_handle!(
    /// A `Spline` by number (1-based: 0 is "none").
    SplineRef
);
numbered_handle!(
    /// A `SplineAnimate` component by number (1-based: 0 is "none").
    SplineAnimateRef
);

/// What a `SplineAnimate` is made of (its `@property` fields).
#[derive(Clone, Debug)]
pub struct AnimateData {
    pub object: ObjectId,
    pub spline: Option<SplineRef>,
    /// The spline's points are in this object's space (none: the world).
    pub container: Option<ObjectId>,
    pub duration: f64,
    pub loop_mode: LoopMode,
    pub alignment: Alignment,
    pub playing: bool,
    /// Seconds played so far.
    pub elapsed: f64,
}

#[derive(Default)]
struct State {
    splines: Vec<Option<Spline>>,
    animates: Vec<Option<AnimateData>>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Forgets every spline and component (a reload, or a test).
pub fn reset() {
    st(|s| *s = State::default());
}

/// Runs `f` on a spline (`None`: no such spline).
pub fn with_spline<R>(r: SplineRef, f: impl FnOnce(&mut Spline) -> R) -> Option<R> {
    st(|st| {
        st.splines
            .get_mut((r.0 as usize).checked_sub(1)?)?
            .as_mut()
            .map(f)
    })
}

/// Keeps a spline and returns its handle.
pub fn add_spline(spline: Spline) -> SplineRef {
    st(|st| {
        st.splines.push(Some(spline));
        SplineRef(st.splines.len() as u32)
    })
}

fn with_animate<R>(r: SplineAnimateRef, f: impl FnOnce(&mut AnimateData) -> R) -> Option<R> {
    st(|st| {
        st.animates
            .get_mut((r.0 as usize).checked_sub(1)?)?
            .as_mut()
            .map(f)
    })
}

/// Runs `f` on a `SplineAnimate`'s data (the scene-file kind reads and writes through this).
pub fn with_spline_animate<R>(
    r: SplineAnimateRef,
    f: impl FnOnce(&mut AnimateData) -> R,
) -> Option<R> {
    with_animate(r, f)
}

fn none_object() -> ObjectId {
    // `from_raw(0)` wraps to a generation no slot has, so it misses everywhere.
    ObjectId::from_raw(0)
}

fn animates_on(object: ObjectId) -> Vec<SplineAnimateRef> {
    st(|st| {
        st.animates
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                d.as_ref()
                    .filter(|d| d.object == object)
                    .map(|_| SplineAnimateRef(i as u32 + 1))
            })
            .collect()
    })
}

fn attach_animate(object: ObjectId) -> SplineAnimateRef {
    st(|st| {
        st.animates.push(Some(AnimateData {
            object,
            spline: None,
            container: None,
            duration: 5.0,
            loop_mode: LoopMode::Loop,
            alignment: Alignment::Path,
            playing: true,
            elapsed: 0.0,
        }));
        SplineAnimateRef(st.animates.len() as u32)
    })
}

fn remove_animate(r: SplineAnimateRef) {
    st(|st| {
        if let Some(slot) = (r.0 as usize)
            .checked_sub(1)
            .and_then(|i| st.animates.get_mut(i))
        {
            *slot = None;
        }
    });
}

/// Removes the `SplineAnimate`s of a destroyed object.
pub fn remove_on(object: ObjectId) {
    for a in animates_on(object) {
        remove_animate(a);
    }
}

/// Puts a component's object where its elapsed time says (also what `place()` does).
fn place(r: SplineAnimateRef) {
    let Some(d) = with_animate(r, |d| d.clone()) else {
        return;
    };
    let Some(spline) = d.spline else {
        return;
    };
    let pose = with_spline(spline, |s| {
        (s.count() > 0).then(|| animate_pose(s, d.elapsed, d.duration, d.loop_mode))
    })
    .flatten();
    let Some((at, dir)) = pose else {
        return;
    };
    let container = d.container.filter(|c| c.to_raw() != 0);
    let world = |p: V3| match container {
        Some(c) => TransformRef(c).transform_point(p),
        None => p,
    };
    scene_api::set_position(d.object, world(at));
    if d.alignment != Alignment::Path || dir.magnitude() < 1e-9 {
        return;
    }
    let dir = match container {
        Some(c) => TransformRef(c).transform_direction(dir),
        None => dir,
    };
    scene_api::set_rotation(d.object, facing(dir));
}

/// The rotation that turns an object to face along `d`: its +x on a flat, 2D path (z is nearly
/// 0), its forward otherwise.
fn facing(d: V3) -> Q {
    if d.z.abs() < 1e-6 {
        Q::euler(0.0, 0.0, (d.y.atan2(d.x) * 180.0) / std::f64::consts::PI)
    } else {
        Q::look_rotation(d, V3::UP)
    }
}

/// Advances every playing `SplineAnimate` by `dt` seconds and puts its object on the path. The
/// host calls it once a frame next to `animation::update`, inside the scene's scope.
pub fn update(dt: f64) {
    let all: Vec<SplineAnimateRef> = st(|st| {
        st.animates
            .iter()
            .enumerate()
            .filter_map(|(i, d)| d.as_ref().map(|_| SplineAnimateRef(i as u32 + 1)))
            .collect()
    });
    for r in all {
        let Some(object) = with_animate(r, |d| d.object) else {
            continue;
        };
        // Like a script's `onUpdate`: only for an object that is active in the hierarchy.
        if !with_scene(|s| s.active(object)).unwrap_or(false) {
            continue;
        }
        with_animate(r, |d| {
            if d.playing {
                d.elapsed += dt;
            }
        });
        place(r);
    }
}

#[ts_api(
    namespace = "Spline", like,
    handles(Spline = SplineRef),
    values(Vector3 = V3 [x: f64, y: f64, z: f64]),
    enums(
        /// `auto`: smooth through the neighbours; `bezier`: the knot's own handles; `linear`: none.
        TangentMode = Tangent [Auto, Bezier, Linear]
    ),
    records(
        /// One knot of a `Spline` (a copy: edit a spline with its methods).
        SplineKnot = Knot [
            position: Vector3,
            /// The handle before the knot, relative to it (used in `bezier` mode).
            tangent_in: Vector3,
            /// The handle after the knot, relative to it (used in `bezier` mode).
            tangent_out: Vector3,
            mode: TangentMode
        ],
        /// Where `t` (0 to 1 by distance) falls on a spline: a segment and its own parameter.
        SplineHit = SplineHit [segment: f64, u: f64],
        /// The nearest point on a spline: its `t` (by distance), the point and how far.
        SplineNearest = SplineNearest [t: f64, point: Vector3, distance: f64]
    )
)]
pub mod spline_api {
    use super::*;

    /// A path through knots (Unity's Splines package, the core of it): smooth unless told
    /// otherwise, every segment a cubic Bezier. `evaluate(t)` goes by distance (t 0 to 1 along
    /// the whole length) so things move at a steady speed; `segmentPoint(i, u)` goes by each
    /// segment's own parameter.
    impl SplineRef {
        /// `new Spline(points?, { closed?, tension?, mode? })`: a knot at each point, all in `mode`.
        #[options(after = points)]
        #[optional(points, closed = false, tension = 0.5, mode)]
        pub fn create(points: &[V3], closed: bool, tension: f64, mode: Tangent) -> SplineRef {
            add_spline(Spline::new(points, closed, tension, mode))
        }

        /// Forgets the spline (a handle is a number, nothing frees it by itself).
        pub fn destroy(self) {
            st(|st| {
                if let Some(slot) = (self.0 as usize)
                    .checked_sub(1)
                    .and_then(|i| st.splines.get_mut(i))
                {
                    *slot = None;
                }
            });
        }

        /// A copy of every knot (edit with `setPosition`, `setTangents`, `setMode`, ...).
        #[property]
        pub fn knots(self) -> Vec<Knot> {
            with_spline(self, |s| s.knots().to_vec()).unwrap_or_default()
        }

        #[property]
        pub fn count(self) -> f64 {
            with_spline(self, |s| s.count() as f64).unwrap_or(0.0)
        }

        /// Whether the last knot joins back to the first.
        #[property]
        pub fn closed(self) -> bool {
            with_spline(self, |s| s.closed()).unwrap_or(false)
        }

        pub fn set_closed(self, on: bool) {
            with_spline(self, |s| s.set_closed(on));
        }

        /// How strongly `auto` knots bend: 0 is straight lines, 0.5 Catmull-Rom.
        #[property]
        pub fn tension(self) -> f64 {
            with_spline(self, |s| s.tension()).unwrap_or(0.5)
        }

        pub fn set_tension(self, t: f64) {
            with_spline(self, |s| s.set_tension(t));
        }

        #[property]
        pub fn segment_count(self) -> f64 {
            with_spline(self, |s| s.segment_count() as f64).unwrap_or(0.0)
        }

        /// Goes up on every change, so anything drawing the spline can tell it moved.
        #[property]
        pub fn version(self) -> f64 {
            with_spline(self, |s| s.version()).unwrap_or(0.0)
        }

        /// The whole length.
        #[property]
        pub fn length(self) -> f64 {
            with_spline(self, |s| s.length()).unwrap_or(0.0)
        }

        /// Adds a knot at the end; returns its index.
        #[optional(mode, tangent_in, tangent_out)]
        pub fn add_knot(self, position: V3, mode: Tangent, tangent_in: V3, tangent_out: V3) -> f64 {
            self.insert_knot(f64::INFINITY, position, mode, tangent_in, tangent_out)
        }

        /// Puts a knot before index `index` (the count: at the end); returns its index.
        #[optional(mode, tangent_in, tangent_out)]
        pub fn insert_knot(
            self,
            index: f64,
            position: V3,
            mode: Tangent,
            tangent_in: V3,
            tangent_out: V3,
        ) -> f64 {
            // A handle left out is NaN in every field.
            let given = |v: V3| (!v.x.is_nan()).then_some(v);
            with_spline(self, |s| {
                s.insert_knot(index, position, mode, given(tangent_in), given(tangent_out)) as f64
            })
            .unwrap_or(0.0)
        }

        pub fn remove_knot(self, index: f64) {
            with_spline(self, |s| s.remove_knot(index));
        }

        pub fn set_position(self, index: f64, position: V3) {
            with_spline(self, |s| s.set_position(index, position));
        }

        /// Sets a knot's handles (relative to it) and makes it a `bezier` knot.
        pub fn set_tangents(self, index: f64, tangent_in: V3, tangent_out: V3) {
            with_spline(self, |s| s.set_tangents(index, tangent_in, tangent_out));
        }

        /// Changes a knot's mode; a knot turning `bezier` keeps the handles it had (auto ones included).
        pub fn set_mode(self, index: f64, mode: Tangent) {
            with_spline(self, |s| s.set_mode(index, mode));
        }

        /// A knot's handle after it, relative to it, whatever its mode (zero when straight).
        pub fn handle_out(self, index: f64) -> V3 {
            with_spline(self, |s| s.handle_out(index)).unwrap_or(V3::ZERO)
        }

        /// A knot's handle before it, relative to it, whatever its mode.
        pub fn handle_in(self, index: f64) -> V3 {
            with_spline(self, |s| s.handle_in(index)).unwrap_or(V3::ZERO)
        }

        /// A point on segment `i` (0 to `segmentCount` - 1), `u` 0 to 1 by the segment's own parameter.
        pub fn segment_point(self, i: f64, u: f64) -> V3 {
            with_spline(self, |s| s.segment_point(i, u)).unwrap_or(V3::ZERO)
        }

        /// The direction (not normalized) segment `i` is going at `u`.
        pub fn segment_derivative(self, i: f64, u: f64) -> V3 {
            with_spline(self, |s| s.segment_derivative(i, u)).unwrap_or(V3::ZERO)
        }

        /// Where `t` (0 to 1 by distance) falls: a segment and its own parameter.
        pub fn locate(self, t: f64) -> SplineHit {
            with_spline(self, |s| s.locate(t)).unwrap_or(SplineHit {
                segment: 0.0,
                u: 0.0,
            })
        }

        /// The point `t` of the way along (0 to 1, by distance).
        pub fn evaluate(self, t: f64) -> V3 {
            with_spline(self, |s| s.evaluate(t)).unwrap_or(V3::ZERO)
        }

        /// The direction the spline goes at `t` (by distance), normalized.
        pub fn tangent(self, t: f64) -> V3 {
            with_spline(self, |s| s.tangent(t)).unwrap_or(V3::ZERO)
        }

        /// Points along the whole spline, `perSegment` to a segment; open ones include the last knot.
        #[optional(per_segment = 16)]
        pub fn sample(self, per_segment: f64) -> Vec<V3> {
            with_spline(self, |s| s.sample(per_segment)).unwrap_or_default()
        }

        /// The nearest point on the spline to `p`: its `t` (by distance), the point and how far.
        pub fn nearest(self, p: V3) -> SplineNearest {
            with_spline(self, |s| s.nearest(p)).unwrap_or(SplineNearest {
                t: 0.0,
                point: V3::ZERO,
                distance: 0.0,
            })
        }
    }
}

#[ts_api(
    namespace = "SplineAnimate", like,
    handles(
        SplineAnimate = SplineAnimateRef,
        Spline = SplineRef,
        GameObject = ObjectId
    ),
    values(
        Vector3 = V3 [x: f64, y: f64, z: f64],
        Quaternion = Q [x: f64, y: f64, z: f64, w: f64]
    ),
    enums(
        /// How a `SplineAnimate` repeats: `loop` starts over, `pingPong` goes back and forth,
        /// `once` stops at the end.
        SplineLoopMode = LoopMode [Loop, PingPong, Once],
        /// `path` turns the object to face along the path, `none` leaves its rotation alone.
        SplineAlignment = Alignment [None, Path]
    ),
    records(
        /// Where a `SplineAnimate` puts its object: a point and the direction along the path.
        SplineAnimatePose = AnimatePose [position: Vector3, direction: Vector3]
    )
)]
pub mod spline_animate_api {
    use super::*;

    /// Moves its object along a spline (Unity's `SplineAnimate`): the whole path in `duration`
    /// seconds at a steady speed, looping, back and forth, or once. `alignment: "path"` turns the
    /// object to face along it (its +x on a flat, 2D spline; its forward otherwise). With a
    /// `container`, the spline's points are in that object's space, so moving it moves the path.
    impl SplineAnimateRef {
        /// Adds a `SplineAnimate` to a GameObject (what `go.addComponent(SplineAnimate)` does).
        pub fn attach(object: ObjectId) -> SplineAnimateRef {
            attach_animate(object)
        }

        /// The first `SplineAnimate` on an object, or none.
        pub fn find_on(object: ObjectId) -> SplineAnimateRef {
            animates_on(object)
                .first()
                .copied()
                .unwrap_or(SplineAnimateRef(0))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            with_animate(self, |d| d.object).unwrap_or_else(none_object)
        }

        /// The path.
        #[property]
        #[nullable]
        pub fn spline(self) -> SplineRef {
            with_animate(self, |d| d.spline.unwrap_or(SplineRef(0))).unwrap_or(SplineRef(0))
        }

        #[nullable]
        pub fn set_spline(self, spline: SplineRef) {
            with_animate(self, |d| d.spline = (spline.0 != 0).then_some(spline));
        }

        /// The spline's points are in this object's space (none: the world).
        #[property]
        #[nullable]
        pub fn container(self) -> ObjectId {
            with_animate(self, |d| d.container.unwrap_or_else(none_object))
                .unwrap_or_else(none_object)
        }

        #[nullable]
        pub fn set_container(self, container: ObjectId) {
            with_animate(self, |d| {
                d.container = (container.to_raw() != 0).then_some(container)
            });
        }

        /// Seconds for the whole path.
        #[property]
        pub fn duration(self) -> f64 {
            with_animate(self, |d| d.duration).unwrap_or(5.0)
        }

        pub fn set_duration(self, v: f64) {
            with_animate(self, |d| d.duration = v);
        }

        #[property]
        pub fn loop_mode(self) -> LoopMode {
            with_animate(self, |d| d.loop_mode).unwrap_or_default()
        }

        pub fn set_loop_mode(self, v: LoopMode) {
            with_animate(self, |d| d.loop_mode = v);
        }

        #[property]
        pub fn alignment(self) -> Alignment {
            with_animate(self, |d| d.alignment).unwrap_or_default()
        }

        pub fn set_alignment(self, v: Alignment) {
            with_animate(self, |d| d.alignment = v);
        }

        #[property]
        pub fn playing(self) -> bool {
            with_animate(self, |d| d.playing).unwrap_or(false)
        }

        pub fn set_playing(self, v: bool) {
            with_animate(self, |d| d.playing = v);
        }

        /// Seconds played so far (set it to jump).
        #[property]
        pub fn elapsed(self) -> f64 {
            with_animate(self, |d| d.elapsed).unwrap_or(0.0)
        }

        pub fn set_elapsed(self, v: f64) {
            with_animate(self, |d| d.elapsed = v);
        }

        /// How far along the path it is now (0 to 1, by distance).
        #[property]
        pub fn t(self) -> f64 {
            with_animate(self, |d| animate_t(d.elapsed, d.duration, d.loop_mode).0).unwrap_or(0.0)
        }

        /// Puts the object where `t` says (call it after moving the spline mid-frame).
        pub fn place(self) {
            super::place(self);
        }

        /// Where a `SplineAnimate` would put its object `elapsed` seconds in, on `spline` (in the
        /// spline's space): what the JavaScript component calls instead of repeating the maths.
        pub fn pose(
            spline: SplineRef,
            elapsed: f64,
            duration: f64,
            loop_mode: LoopMode,
        ) -> AnimatePose {
            with_spline(spline, |s| {
                let (position, direction) = animate_pose(s, elapsed, duration, loop_mode);
                AnimatePose {
                    position,
                    direction,
                }
            })
            .unwrap_or(AnimatePose {
                position: V3::ZERO,
                direction: V3::ZERO,
            })
        }

        /// How far along the path (0 to 1, by distance) a `SplineAnimate` is `elapsed` seconds in.
        pub fn progress(elapsed: f64, duration: f64, loop_mode: LoopMode) -> f64 {
            animate_t(elapsed, duration, loop_mode).0
        }

        /// The rotation that faces an object along `direction`: its +x on a flat, 2D path, its
        /// forward otherwise.
        pub fn facing(direction: V3) -> Q {
            super::facing(direction)
        }
    }
}

/// A point and a direction: what [`spline_animate_api`] `pose` returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimatePose {
    pub position: V3,
    pub direction: V3,
}

// ---- scene files ----

fn vec3_json(v: V3) -> Value {
    json!([v.x, v.y, v.z])
}

/// A vector from `[x, y, z]` or `{"$type":"Vector3","value":[x, y, z]}`.
fn vec3_of(v: &Value) -> Option<V3> {
    let a = match v {
        Value::Array(a) => a,
        Value::Object(o) => o.get("value")?.as_array()?,
        _ => return None,
    };
    (a.len() == 3).then(|| {
        v3(
            a[0].as_f64().unwrap_or(0.0),
            a[1].as_f64().unwrap_or(0.0),
            a[2].as_f64().unwrap_or(0.0),
        )
    })
}

/// The saved form of a spline: `{ closed, tension, knots: [{ position, tangentIn, tangentOut, mode }] }`.
fn spline_def(s: &Spline) -> Value {
    let knots: Vec<Value> = s
        .knots()
        .iter()
        .map(|k| {
            json!({
                "position": vec3_json(k.position),
                "tangentIn": vec3_json(k.tangent_in),
                "tangentOut": vec3_json(k.tangent_out),
                "mode": spline_api::tangent_mode_name(k.mode),
            })
        })
        .collect();
    json!({ "closed": s.closed(), "tension": s.tension(), "knots": knots })
}

/// A spline from its saved form (a field that does not fit is skipped).
fn spline_from(def: &Value) -> Spline {
    let mut s = Spline::new(
        &[],
        def.get("closed").and_then(Value::as_bool).unwrap_or(false),
        def.get("tension").and_then(Value::as_f64).unwrap_or(0.5),
        Tangent::Auto,
    );
    for k in def
        .get("knots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(position) = k.get("position").and_then(vec3_of) else {
            continue;
        };
        let mode = k
            .get("mode")
            .and_then(Value::as_str)
            .and_then(spline_api::tangent_mode_parse)
            .unwrap_or_default();
        let tin = k.get("tangentIn").and_then(vec3_of);
        let tout = k.get("tangentOut").and_then(vec3_of);
        s.add_knot(position, mode, tin, tout);
    }
    s
}

fn spline_asset_kind() -> AssetKind {
    AssetKind {
        name: "Spline".into(),
        also: Vec::new(),
        shared: false,
        create: Box::new(|def| Ok(u64::from(add_spline(spline_from(def)).0))),
        describe: Box::new(|h| with_spline(SplineRef(h as u32), |s| spline_def(s))),
    }
}

fn object_value(o: Option<ObjectId>) -> Value {
    o.map_or(Value::Null, |o| object_ref(o.to_raw()))
}

/// A GameObject placeholder (`None` for null).
fn object_of(v: &Value) -> Option<Option<ObjectId>> {
    match v {
        Value::Null => Some(None),
        Value::Object(o) if o.get("$ref").and_then(Value::as_str) == Some("GameObject") => {
            Some(Some(ObjectId::from_raw(o.get("object")?.as_u64()?)))
        }
        _ => None,
    }
}

fn animate_kind() -> Kind {
    Kind {
        name: "SplineAnimate".into(),
        create: Box::new(|o| Ok(u64::from(attach_animate(ObjectId::from_raw(o)).0))),
        on: Box::new(|o| {
            animates_on(ObjectId::from_raw(o))
                .into_iter()
                .map(|a| u64::from(a.0))
                .collect()
        }),
        save: Box::new(|h| {
            let mut m = Map::new();
            with_animate(SplineAnimateRef(h as u32), |d| {
                m.insert(
                    "spline".into(),
                    asset_ref("Spline", u64::from(d.spline.map_or(0, |s| s.0))),
                );
                m.insert("container".into(), object_value(d.container));
                m.insert("duration".into(), json!(d.duration));
                m.insert(
                    "loopMode".into(),
                    json!(spline_animate_api::spline_loop_mode_name(d.loop_mode)),
                );
                m.insert(
                    "alignment".into(),
                    json!(spline_animate_api::spline_alignment_name(d.alignment)),
                );
                m.insert("playing".into(), json!(d.playing));
                m.insert("elapsed".into(), json!(d.elapsed));
            });
            m
        }),
        load: Box::new(|h, props| {
            let mut warnings = Vec::new();
            with_animate(SplineAnimateRef(h as u32), |d| {
                for (k, v) in props {
                    let ok = match k.as_str() {
                        "spline" => asset_handle(v, "Spline")
                            .map(|x| d.spline = (x != 0).then_some(SplineRef(x as u32))),
                        "container" => object_of(v).map(|x| d.container = x),
                        "duration" => v.as_f64().map(|x| d.duration = x),
                        "loopMode" => v
                            .as_str()
                            .and_then(spline_animate_api::spline_loop_mode_parse)
                            .map(|x| d.loop_mode = x),
                        "alignment" => v
                            .as_str()
                            .and_then(spline_animate_api::spline_alignment_parse)
                            .map(|x| d.alignment = x),
                        "playing" => v.as_bool().map(|x| d.playing = x),
                        "elapsed" => v.as_f64().map(|x| d.elapsed = x),
                        other => {
                            warnings.push(format!("no property `{other}`; dropped"));
                            continue;
                        }
                    };
                    if ok.is_none() {
                        warnings.push(format!("property `{k}` has a value of the wrong shape"));
                    }
                }
            });
            warnings
        }),
        awake: Box::new(|_| {}),
        object: Box::new(|h| with_animate(SplineAnimateRef(h as u32), |d| d.object.to_raw())),
        destroy: Box::new(|h| remove_animate(SplineAnimateRef(h as u32))),
    }
}

/// Registers the `SplineAnimate` scene-file kind and the `Spline` inline asset kind on this thread.
pub fn register_kinds() {
    platypus_kinds::register(animate_kind());
    platypus_kinds::register_asset(spline_asset_kind());
}
