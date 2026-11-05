// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The script API of the animation controller and the `Animator` component: `AnimationParameter`,
//! `AnimationCondition`, `AnimationTransition`, `AnimationBlendTree`, `AnimationNode` (a state),
//! `AnimationController`, the chained `AnimationBuilder` that makes them, and `Animator`. The design
//! is `docs/research/platypus-native-animation.md` (section 8); the original is `AnimatorController` and
//! `Animator` in `js/engine/src/animation.ts`.
//!
//! Differences: the pieces are classes (and a builder) instead of object literals, a parameter
//! default is a number (a flag is 0 or 1), `getCurrentAnimatorStateInfo()` is the properties
//! `currentStateName`, `currentNormalizedTime` and `currentLength` (and `next...`), and `toJSON()` is
//! text (`toJson()`).

use crate::animation::ClipRef;
use platypus_animation::{
    AnimParameter, AnimatorPlayer, BlendChild, BlendTree, Condition, ConditionMode, Controller,
    ControllerId, CullingMode, Motion, Node, NodeId, Param, ParameterType, Transition,
    TransitionId, with_world,
};
use platypus_scene::ObjectId;
use std::cell::RefCell;
use ts_api::{Handle, ts_api};

macro_rules! handle {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(pub u64);
        impl Handle for $name {
            fn to_raw(self) -> u64 { self.0 }
            fn from_raw(raw: u64) -> Self { $name(raw) }
        }
    )*};
}

handle!(
    ParameterRef,
    ConditionRef,
    TransitionRef,
    BlendRef,
    NodeRef,
    ControllerRef,
    BuilderRef,
    AnimatorRef
);

fn none_if_nan(x: f64) -> Option<f64> {
    (!x.is_nan()).then_some(x)
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

fn clip_id(c: ClipRef) -> platypus_animation::ClipId {
    platypus_animation::ClipId(c.0 as u32)
}

fn nan_or(x: Option<f64>) -> f64 {
    x.unwrap_or(f64::NAN)
}

#[ts_api(namespace = "AnimationParameter", handles(AnimationParameter = ParameterRef))]
pub mod animation_parameter_api {
    use super::*;

    impl ParameterRef {
        /// A controller parameter: `type` is `float`, `int`, `bool` or `trigger`; the default is a
        /// number (a flag is 0 or 1).
        #[optional(default_value)]
        pub fn create(name: &str, type_name: &str, default_value: f64) -> ParameterRef {
            let ty = ParameterType::parse(type_name).unwrap_or(ParameterType::Float);
            let default = none_if_nan(default_value).map(|v| match ty {
                ParameterType::Bool | ParameterType::Trigger => Param::Bool(v != 0.0),
                _ => Param::Num(v),
            });
            ParameterRef(u64::from(
                with_world(|w| {
                    w.graphs.add_parameter(AnimParameter {
                        name: name.to_string(),
                        ty,
                        default,
                    })
                })
                .0,
            ))
        }

        #[property]
        pub fn name(self) -> String {
            with_world(|w| {
                w.graphs
                    .parameter(platypus_animation::ParameterId(self.0 as u32))
                    .map(|p| p.name.clone())
                    .unwrap_or_default()
            })
        }

        #[property]
        #[js_name = "type"]
        pub fn type_name(self) -> String {
            with_world(|w| {
                w.graphs
                    .parameter(platypus_animation::ParameterId(self.0 as u32))
                    .map(|p| p.ty.name().to_string())
                    .unwrap_or_default()
            })
        }

        /// The default value as a number (NaN if it has none).
        #[property]
        pub fn default_value(self) -> f64 {
            with_world(|w| {
                w.graphs
                    .parameter(platypus_animation::ParameterId(self.0 as u32))
                    .and_then(|p| p.default)
                    .map_or(f64::NAN, Param::number)
            })
        }
    }
}

#[ts_api(namespace = "AnimationCondition", handles(AnimationCondition = ConditionRef))]
pub mod animation_condition_api {
    use super::*;

    impl ConditionRef {
        /// When a parameter passes a test: `mode` is `if`, `ifNot`, `greater`, `less`, `equals` or
        /// `notEqual` (the last four against `threshold`).
        #[optional(threshold)]
        pub fn create(parameter: &str, mode: &str, threshold: f64) -> ConditionRef {
            ConditionRef(u64::from(
                with_world(|w| {
                    w.graphs.add_condition(Condition {
                        parameter: parameter.to_string(),
                        mode: ConditionMode::parse(mode).unwrap_or(ConditionMode::If),
                        threshold: none_if_nan(threshold),
                    })
                })
                .0,
            ))
        }

        #[property]
        pub fn parameter(self) -> String {
            with_world(|w| {
                w.graphs
                    .condition(platypus_animation::ConditionId(self.0 as u32))
                    .map(|c| c.parameter.clone())
                    .unwrap_or_default()
            })
        }

        #[property]
        pub fn mode(self) -> String {
            with_world(|w| {
                w.graphs
                    .condition(platypus_animation::ConditionId(self.0 as u32))
                    .map(|c| c.mode.name().to_string())
                    .unwrap_or_default()
            })
        }

        #[property]
        pub fn threshold(self) -> f64 {
            with_world(|w| {
                w.graphs
                    .condition(platypus_animation::ConditionId(self.0 as u32))
                    .map_or(f64::NAN, |c| nan_or(c.threshold))
            })
        }

        pub fn set_threshold(self, v: f64) {
            with_world(|w| {
                if let Some(c) = w
                    .graphs
                    .condition_mut(platypus_animation::ConditionId(self.0 as u32))
                {
                    c.threshold = none_if_nan(v);
                }
            });
        }
    }
}

fn transition<R>(t: TransitionRef, f: impl FnOnce(&mut Transition) -> R) -> Option<R> {
    with_world(|w| w.graphs.transition_mut(TransitionId(t.0 as u32)).map(f))
}

#[ts_api(namespace = "AnimationTransition", handles(
    AnimationTransition = TransitionRef, AnimationCondition = ConditionRef))]
pub mod animation_transition_api {
    use super::*;

    impl TransitionRef {
        /// A move from a state (or `Any`) to another. Options: `duration` (the cross-fade in
        /// seconds, 0.25 by default), `hasExitTime` and `exitTime` (normalized, 1 by default),
        /// `offset` (where the destination starts, normalized).
        pub fn create(from: &str, to: &str) -> TransitionRef {
            TransitionRef(u64::from(
                with_world(|w| {
                    w.graphs.add_transition(Transition {
                        from: from.to_string(),
                        to: to.to_string(),
                        conditions: Vec::new(),
                        has_exit_time: false,
                        exit_time: None,
                        duration: None,
                        offset: None,
                    })
                })
                .0,
            ))
        }

        #[property]
        pub fn from(self) -> String {
            transition(self, |t| t.from.clone()).unwrap_or_default()
        }

        #[property]
        pub fn to(self) -> String {
            transition(self, |t| t.to.clone()).unwrap_or_default()
        }

        #[property]
        pub fn duration(self) -> f64 {
            transition(self, |t| t.duration.unwrap_or(0.25)).unwrap_or(0.25)
        }

        pub fn set_duration(self, v: f64) {
            transition(self, |t| t.duration = Some(v));
        }

        #[property]
        pub fn has_exit_time(self) -> bool {
            transition(self, |t| t.has_exit_time).unwrap_or(false)
        }

        pub fn set_has_exit_time(self, v: bool) {
            transition(self, |t| t.has_exit_time = v);
        }

        #[property]
        pub fn exit_time(self) -> f64 {
            transition(self, |t| t.exit_time.unwrap_or(1.0)).unwrap_or(1.0)
        }

        pub fn set_exit_time(self, v: f64) {
            transition(self, |t| t.exit_time = Some(v));
        }

        #[property]
        pub fn offset(self) -> f64 {
            transition(self, |t| t.offset.unwrap_or(0.0)).unwrap_or(0.0)
        }

        pub fn set_offset(self, v: f64) {
            transition(self, |t| t.offset = Some(v));
        }

        /// Adds a condition (all of them must hold).
        pub fn add_condition(self, condition: ConditionRef) {
            transition(self, |t| {
                t.conditions
                    .push(platypus_animation::ConditionId(condition.0 as u32))
            });
        }

        #[property]
        pub fn condition_count(self) -> f64 {
            transition(self, |t| t.conditions.len() as f64).unwrap_or(0.0)
        }

        pub fn condition_at(self, index: f64) -> ConditionRef {
            transition(self, |t| {
                t.conditions
                    .get(index as usize)
                    .map_or(ConditionRef(0), |c| ConditionRef(u64::from(c.0)))
            })
            .unwrap_or(ConditionRef(0))
        }
    }
}

#[ts_api(namespace = "AnimationBlendTree", handles(
    AnimationBlendTree = BlendRef, AnimationClip = ClipRef))]
pub mod animation_blend_tree_api {
    use super::*;

    impl BlendRef {
        /// A 1D blend tree: clips placed along a float parameter, the two around its value blended.
        pub fn create(parameter: &str) -> BlendRef {
            BlendRef(u64::from(
                with_world(|w| {
                    w.graphs.add_blend(BlendTree {
                        parameter: parameter.to_string(),
                        children: Vec::new(),
                    })
                })
                .0,
            ))
        }

        #[property]
        pub fn parameter(self) -> String {
            with_world(|w| {
                w.graphs
                    .blend(platypus_animation::BlendId(self.0 as u32))
                    .map(|b| b.parameter.clone())
                    .unwrap_or_default()
            })
        }

        /// Places a clip at a parameter value; `timeScale` (1 by default) stretches its time.
        #[optional(time_scale)]
        pub fn add_child(self, clip: ClipRef, threshold: f64, time_scale: f64) {
            with_world(|w| {
                if let Some(b) = w
                    .graphs
                    .blend_mut(platypus_animation::BlendId(self.0 as u32))
                {
                    b.children.push(BlendChild {
                        clip: clip_id(clip),
                        threshold,
                        time_scale: none_if_nan(time_scale),
                    });
                }
            });
        }

        #[property]
        pub fn child_count(self) -> f64 {
            with_world(|w| {
                w.graphs
                    .blend(platypus_animation::BlendId(self.0 as u32))
                    .map_or(0.0, |b| b.children.len() as f64)
            })
        }
    }
}

fn node<R>(n: NodeRef, f: impl FnOnce(&mut Node) -> R) -> Option<R> {
    with_world(|w| w.graphs.node_mut(NodeId(n.0 as u32)).map(f))
}

#[ts_api(namespace = "AnimationNode", handles(
    AnimationNode = NodeRef, AnimationClip = ClipRef, AnimationBlendTree = BlendRef))]
pub mod animation_node_api {
    use super::*;

    impl NodeRef {
        /// A state that plays a clip. Options: `speed` (1 by default) and `speedParameter` (a float
        /// parameter multiplying it).
        pub fn create(name: &str, clip: ClipRef) -> NodeRef {
            NodeRef(u64::from(
                with_world(|w| {
                    w.graphs.add_node(Node {
                        name: name.to_string(),
                        motion: Motion::Clip(clip_id(clip)),
                        speed: None,
                        speed_parameter: None,
                    })
                })
                .0,
            ))
        }

        /// A state that plays a blend tree.
        pub fn blend(name: &str, tree: BlendRef) -> NodeRef {
            NodeRef(u64::from(
                with_world(|w| {
                    w.graphs.add_node(Node {
                        name: name.to_string(),
                        motion: Motion::Blend(platypus_animation::BlendId(tree.0 as u32)),
                        speed: None,
                        speed_parameter: None,
                    })
                })
                .0,
            ))
        }

        #[property]
        pub fn name(self) -> String {
            node(self, |n| n.name.clone()).unwrap_or_default()
        }

        #[property]
        pub fn speed(self) -> f64 {
            node(self, |n| n.speed.unwrap_or(1.0)).unwrap_or(1.0)
        }

        pub fn set_speed(self, v: f64) {
            node(self, |n| n.speed = Some(v));
        }

        /// A float parameter multiplying the speed ("" if none).
        #[property]
        pub fn speed_parameter(self) -> String {
            node(self, |n| n.speed_parameter.clone().unwrap_or_default()).unwrap_or_default()
        }

        pub fn set_speed_parameter(self, name: &str) {
            node(self, |n| n.speed_parameter = non_empty(name));
        }

        /// The clip it plays (no clip for a blend tree).
        #[property]
        pub fn clip(self) -> ClipRef {
            node(self, |n| match n.motion {
                Motion::Clip(c) => ClipRef(u64::from(c.0)),
                Motion::Blend(_) => ClipRef(0),
            })
            .unwrap_or(ClipRef(0))
        }

        /// The blend tree it plays (no tree for a clip).
        #[property]
        pub fn blend_tree(self) -> BlendRef {
            node(self, |n| match n.motion {
                Motion::Blend(b) => BlendRef(u64::from(b.0)),
                Motion::Clip(_) => BlendRef(0),
            })
            .unwrap_or(BlendRef(0))
        }
    }
}

/// The speed options of a state made from a description.
fn apply_state_options(n: NodeRef, opts: Option<StateOptionsData>) {
    if let Some(o) = opts {
        node(n, |n| {
            if let Some(v) = o.speed {
                n.speed = Some(v);
            }
            if let Some(p) = o.speed_parameter {
                n.speed_parameter = non_empty(&p);
            }
        });
    }
}

fn controller<R>(c: ControllerRef, f: impl FnOnce(&mut Controller) -> R) -> Option<R> {
    with_world(|w| w.graphs.controller_mut(ControllerId(c.0 as u32)).map(f))
}

/// A clip placed in a blend tree.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendChildSpec {
    pub motion: ClipRef,
    pub threshold: f64,
    pub time_scale: Option<f64>,
}

/// A state that blends clips along a float parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendSpecData {
    pub blend_parameter: String,
    pub children: Vec<BlendChildSpec>,
}

/// The options of a state.
#[derive(Clone, Debug, PartialEq)]
pub struct StateOptionsData {
    pub speed: Option<f64>,
    pub speed_parameter: Option<String>,
}

/// A test of a parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionData {
    pub parameter: String,
    pub mode: String,
    pub threshold: Option<f64>,
}

/// A move from a state to another.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionData {
    pub from: String,
    pub to: String,
    pub conditions: Option<Vec<ConditionData>>,
    pub has_exit_time: Option<bool>,
    pub exit_time: Option<f64>,
    pub duration: Option<f64>,
    pub offset: Option<f64>,
}

#[ts_api(namespace = "AnimationController", handles(
    AnimationController = ControllerRef, AnimationParameter = ParameterRef,
    AnimationNode = NodeRef, AnimationTransition = TransitionRef, AnimationBuilder = BuilderRef,
    AnimationClip = ClipRef),
    aliases(AnimatorController = AnimationController),
    records(
        /// A clip in a blend tree, placed at a parameter value (`timeScale` is 1 by default).
        BlendChild = BlendChildSpec [motion: AnimationClip, threshold: f64, time_scale: Option<f64>],
        /// A state that blends clips along a float parameter.
        BlendSpec = BlendSpecData [blend_parameter: string, children: Vec<BlendChild>],
        /// The speed of a state, and a float parameter that multiplies it.
        StateOpts = StateOptionsData [speed: Option<f64>, speed_parameter: Option<string>],
        /// When a parameter passes a test (`mode`: `if`, `ifNot`, `greater`, `less`, `equals` or `notEqual`).
        ConditionSpec = ConditionData [parameter: string, mode: string, threshold: Option<f64>],
        /// A move from a state (or `Any`) to another: all its conditions must hold.
        TransitionSpec = TransitionData [
            from: string,
            to: string,
            conditions: Option<Vec<ConditionSpec>>,
            has_exit_time: Option<bool>,
            exit_time: Option<f64>,
            duration: Option<f64>,
            offset: Option<f64>
        ]
    ))]
pub mod animation_controller_api {
    use super::*;

    impl ControllerRef {
        /// An empty controller (one layer): add parameters, states and transitions, or make one
        /// with `AnimationController.build()`.
        pub fn create() -> ControllerRef {
            ControllerRef(u64::from(
                with_world(|w| w.graphs.add_controller(Controller::default())).0,
            ))
        }

        /// A builder that makes a controller with chained calls.
        pub fn build() -> BuilderRef {
            let controller = with_world(|w| w.graphs.add_controller(Controller::default()));
            BUILDERS.with(|b| {
                let mut b = b.borrow_mut();
                b.push(BuilderState {
                    controller,
                    transition: None,
                    blend: None,
                });
                BuilderRef(b.len() as u64)
            })
        }

        pub fn add_parameter(self, parameter: ParameterRef) {
            controller(self, |c| {
                c.parameters
                    .push(platypus_animation::ParameterId(parameter.0 as u32))
            });
        }

        /// Adds a parameter (`type`: `float`, `int`, `bool` or `trigger`; the default is a
        /// number, a flag is 0 or 1) and returns the controller, so calls chain.
        #[js_name = "addParameter"]
        #[optional(default_value)]
        pub fn add_parameter_named(
            self,
            name: &str,
            type_name: &str,
            default_value: f64,
        ) -> ControllerRef {
            self.add_parameter(ParameterRef::create(name, type_name, default_value));
            self
        }

        /// Adds a parameter with a flag for its default (a `bool`'s `true`, as Unity's) and
        /// returns the controller.
        #[js_name = "addParameter"]
        pub fn add_parameter_flag(
            self,
            name: &str,
            type_name: &str,
            default_value: bool,
        ) -> ControllerRef {
            let v = if default_value { 1.0 } else { 0.0 };
            self.add_parameter(ParameterRef::create(name, type_name, v));
            self
        }

        /// Adds a state that plays a clip and returns the controller.
        #[js_name = "addState"]
        #[optional(opts)]
        #[ts_param(opts = "StateOpts")]
        pub fn add_state_clip(
            self,
            name: &str,
            motion: ClipRef,
            opts: Option<StateOptionsData>,
        ) -> ControllerRef {
            let n = NodeRef::create(name, motion);
            super::apply_state_options(n, opts);
            self.add_node(n);
            self
        }

        /// Adds a state that blends clips along a float parameter and returns the controller.
        #[js_name = "addState"]
        #[optional(opts)]
        #[ts_param(motion = "BlendSpec", opts = "StateOpts")]
        pub fn add_state_blend(
            self,
            name: &str,
            motion: Option<BlendSpecData>,
            opts: Option<StateOptionsData>,
        ) -> ControllerRef {
            let Some(spec) = motion else { return self };
            let tree = BlendRef::create(&spec.blend_parameter);
            for c in &spec.children {
                tree.add_child(c.motion, c.threshold, c.time_scale.unwrap_or(f64::NAN));
            }
            let n = NodeRef::blend(name, tree);
            super::apply_state_options(n, opts);
            self.add_node(n);
            self
        }

        /// Adds a transition made from a description and returns the controller.
        #[js_name = "addTransition"]
        #[ts_param(t = "TransitionSpec")]
        pub fn add_transition_spec(self, t: Option<TransitionData>) -> ControllerRef {
            let Some(t) = t else { return self };
            let tr = TransitionRef::create(&t.from, &t.to);
            transition(tr, |x| {
                if let Some(v) = t.has_exit_time {
                    x.has_exit_time = v;
                }
                if let Some(v) = t.exit_time {
                    x.exit_time = Some(v);
                }
                if let Some(v) = t.duration {
                    x.duration = Some(v);
                }
                if let Some(v) = t.offset {
                    x.offset = Some(v);
                }
            });
            for c in t.conditions.iter().flatten() {
                let threshold = c.threshold.unwrap_or(f64::NAN);
                tr.add_condition(ConditionRef::create(&c.parameter, &c.mode, threshold));
            }
            self.add_transition(tr);
            self
        }

        /// Adds a state (an `AnimationNode`).
        pub fn add_node(self, node: NodeRef) {
            controller(self, |c| c.nodes.push(NodeId(node.0 as u32)));
        }

        pub fn add_transition(self, transition: TransitionRef) {
            controller(self, |c| {
                c.transitions.push(TransitionId(transition.0 as u32))
            });
        }

        /// The state the animator starts in ("" is the first one added).
        #[property]
        pub fn default_state(self) -> String {
            controller(self, |c| c.default_state.clone().unwrap_or_default()).unwrap_or_default()
        }

        pub fn set_default_state(self, name: &str) {
            controller(self, |c| c.default_state = non_empty(name));
        }

        #[property]
        pub fn parameter_count(self) -> f64 {
            controller(self, |c| c.parameters.len() as f64).unwrap_or(0.0)
        }

        pub fn parameter_at(self, index: f64) -> ParameterRef {
            controller(self, |c| {
                c.parameters
                    .get(index as usize)
                    .map_or(ParameterRef(0), |p| ParameterRef(u64::from(p.0)))
            })
            .unwrap_or(ParameterRef(0))
        }

        #[property]
        pub fn node_count(self) -> f64 {
            controller(self, |c| c.nodes.len() as f64).unwrap_or(0.0)
        }

        pub fn node_at(self, index: f64) -> NodeRef {
            controller(self, |c| {
                c.nodes
                    .get(index as usize)
                    .map_or(NodeRef(0), |n| NodeRef(u64::from(n.0)))
            })
            .unwrap_or(NodeRef(0))
        }

        #[property]
        pub fn transition_count(self) -> f64 {
            controller(self, |c| c.transitions.len() as f64).unwrap_or(0.0)
        }

        pub fn transition_at(self, index: f64) -> TransitionRef {
            controller(self, |c| {
                c.transitions
                    .get(index as usize)
                    .map_or(TransitionRef(0), |t| TransitionRef(u64::from(t.0)))
            })
            .unwrap_or(TransitionRef(0))
        }

        /// The controller as JSON text (clips inline).
        pub fn to_json(self) -> String {
            with_world(|w| {
                w.controller_to_json(ControllerId(self.0 as u32))
                    .to_string()
            })
        }

        /// A controller from the JSON `toJson` wrote.
        pub fn from_json(text: &str) -> ControllerRef {
            let data = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
            ControllerRef(u64::from(with_world(|w| w.controller_from_json(&data)).0))
        }
    }
}

struct BuilderState {
    controller: ControllerId,
    /// The transition `when`, `exitTime`, `duration` and `offset` apply to.
    transition: Option<TransitionId>,
    /// The blend tree `child` adds to.
    blend: Option<platypus_animation::BlendId>,
}

thread_local! {
    static BUILDERS: RefCell<Vec<BuilderState>> = const { RefCell::new(Vec::new()) };
}

fn builder<R>(b: BuilderRef, f: impl FnOnce(&mut BuilderState) -> R) -> Option<R> {
    BUILDERS.with(|v| {
        v.borrow_mut()
            .get_mut((b.0 as usize).wrapping_sub(1))
            .map(f)
    })
}

#[ts_api(namespace = "AnimationBuilder", handles(
    AnimationBuilder = BuilderRef, AnimationController = ControllerRef, AnimationClip = ClipRef))]
pub mod animation_builder_api {
    use super::*;

    impl BuilderRef {
        /// A parameter: `type` is `float`, `int`, `bool` or `trigger`.
        #[optional(default_value)]
        pub fn parameter(self, name: &str, type_name: &str, default_value: f64) -> BuilderRef {
            let p = ParameterRef::create(name, type_name, default_value);
            if let Some(c) = builder(self, |b| b.controller) {
                with_world(|w| {
                    if let Some(c) = w.graphs.controller_mut(c) {
                        c.parameters
                            .push(platypus_animation::ParameterId(p.0 as u32));
                    }
                });
            }
            self
        }

        /// A state that plays a clip, with a speed (1 by default) and a float parameter that
        /// multiplies it.
        #[optional(speed, speed_parameter)]
        pub fn state(
            self,
            name: &str,
            clip: ClipRef,
            speed: f64,
            speed_parameter: &str,
        ) -> BuilderRef {
            let n = NodeRef::create(name, clip);
            node(n, |n| {
                n.speed = none_if_nan(speed);
                n.speed_parameter = non_empty(speed_parameter);
            });
            if let Some(c) = builder(self, |b| b.controller) {
                controller(ControllerRef(u64::from(c.0)), |c| {
                    c.nodes.push(NodeId(n.0 as u32))
                });
            }
            self
        }

        /// A state that plays a blend tree over a float parameter; add its clips with `child`.
        pub fn blend_state(self, name: &str, parameter: &str) -> BuilderRef {
            let tree = BlendRef::create(parameter);
            let n = NodeRef::blend(name, tree);
            if let Some(c) = builder(self, |b| {
                b.blend = Some(platypus_animation::BlendId(tree.0 as u32));
                b.controller
            }) {
                controller(ControllerRef(u64::from(c.0)), |c| {
                    c.nodes.push(NodeId(n.0 as u32))
                });
            }
            self
        }

        /// Adds a clip to the blend state made last, at a parameter value.
        #[optional(time_scale)]
        pub fn child(self, clip: ClipRef, threshold: f64, time_scale: f64) -> BuilderRef {
            if let Some(Some(tree)) = builder(self, |b| b.blend) {
                BlendRef(u64::from(tree.0)).add_child(clip, threshold, time_scale);
            }
            self
        }

        /// A transition from a state (or `Any`) to another; the calls after it (`when`,
        /// `exitTime`, `duration`, `offset`) set it up.
        pub fn transition(self, from: &str, to: &str) -> BuilderRef {
            let t = TransitionRef::create(from, to);
            if let Some(c) = builder(self, |b| {
                b.transition = Some(TransitionId(t.0 as u32));
                b.controller
            }) {
                controller(ControllerRef(u64::from(c.0)), |c| {
                    c.transitions.push(TransitionId(t.0 as u32))
                });
            }
            self
        }

        /// A condition on the transition made last.
        #[optional(threshold)]
        pub fn when(self, parameter: &str, mode: &str, threshold: f64) -> BuilderRef {
            if let Some(Some(t)) = builder(self, |b| b.transition) {
                let cond = ConditionRef::create(parameter, mode, threshold);
                TransitionRef(u64::from(t.0)).add_condition(cond);
            }
            self
        }

        /// The transition made last waits until the source state passes this normalized time.
        pub fn exit_time(self, normalized_time: f64) -> BuilderRef {
            if let Some(Some(t)) = builder(self, |b| b.transition) {
                let t = TransitionRef(u64::from(t.0));
                t.set_has_exit_time(true);
                t.set_exit_time(normalized_time);
            }
            self
        }

        /// The cross-fade of the transition made last, in seconds.
        pub fn duration(self, seconds: f64) -> BuilderRef {
            if let Some(Some(t)) = builder(self, |b| b.transition) {
                TransitionRef(u64::from(t.0)).set_duration(seconds);
            }
            self
        }

        /// Where the destination of the transition made last starts, normalized.
        pub fn offset(self, normalized_time: f64) -> BuilderRef {
            if let Some(Some(t)) = builder(self, |b| b.transition) {
                TransitionRef(u64::from(t.0)).set_offset(normalized_time);
            }
            self
        }

        /// The state the animator starts in.
        pub fn default_state(self, name: &str) -> BuilderRef {
            if let Some(c) = builder(self, |b| b.controller) {
                controller(ControllerRef(u64::from(c.0)), |c| {
                    c.default_state = non_empty(name)
                });
            }
            self
        }

        /// The finished controller.
        pub fn create(self) -> ControllerRef {
            builder(self, |b| ControllerRef(u64::from(b.controller.0))).unwrap_or(ControllerRef(0))
        }
    }
}

fn animator<R>(
    a: AnimatorRef,
    f: impl FnOnce(&mut AnimatorPlayer, &platypus_animation::World) -> R,
) -> Option<R> {
    with_world(|w| {
        let mut p = w.animators.get((a.0 as usize).wrapping_sub(1))?.clone();
        let r = f(&mut p, w);
        w.animators[(a.0 as usize) - 1] = p;
        Some(r)
    })
}

/// The state an animator is in.
#[derive(Clone, Debug, PartialEq)]
pub struct StateSummary {
    pub name: String,
    pub normalized_time: f64,
    pub length: f64,
}

#[ts_api(namespace = "Animator", handles(
    Animator = AnimatorRef, AnimationController = ControllerRef, GameObject = ObjectId),
    enums(
        /// What an `Animator` does while none of its object's renderers was drawn (by a camera or
        /// into a light's shadow map, last frame): `alwaysAnimate` (the default), `cullUpdateTransforms`
        /// (the state machine, clock and events go on, nothing is posed) or `cullCompletely` (it
        /// stops, holding its pose, and resumes from there).
        AnimatorCullingMode = CullingMode [AlwaysAnimate, CullUpdateTransforms, CullCompletely]
    ),
    records(
        /// The state an animator is in: its name, how far along it is (past 1 after loops) and its length in seconds.
        AnimatorStateInfo = StateSummary [name: string, normalized_time: f64, length: f64]
    ))]
pub mod animator_api {
    use super::*;

    impl AnimatorRef {
        /// Adds an `Animator` to a GameObject (what `go.addComponent(Animator)` does). Options:
        /// `controller`, `speed` and `cullingMode`.
        pub fn attach(object: ObjectId) -> AnimatorRef {
            AnimatorRef(with_world(|w| {
                w.animators.push(AnimatorPlayer::new(object));
                w.animators.len() as u64
            }))
        }

        /// The `Animator` on an object, or none (what `go.getComponent(Animator)` does).
        pub fn find_on(object: ObjectId) -> AnimatorRef {
            AnimatorRef(with_world(|w| {
                w.animators
                    .iter()
                    .position(|p| p.object == object)
                    .map_or(0, |i| i as u64 + 1)
            }))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            animator(self, |p, _| p.object).unwrap_or(ObjectId {
                index: u32::MAX,
                generation: u32::MAX,
            })
        }

        #[property]
        pub fn controller(self) -> ControllerRef {
            animator(self, |p, _| {
                p.controller
                    .map_or(ControllerRef(0), |c| ControllerRef(u64::from(c.0)))
            })
            .unwrap_or(ControllerRef(0))
        }

        pub fn set_controller(self, c: ControllerRef) {
            animator(self, |p, _| {
                p.controller = (c.0 != 0).then_some(ControllerId(c.0 as u32))
            });
        }

        /// Multiplies every state speed.
        #[property]
        pub fn speed(self) -> f64 {
            animator(self, |p, _| p.speed).unwrap_or(1.0)
        }

        pub fn set_speed(self, v: f64) {
            animator(self, |p, _| p.speed = v);
        }

        /// What it does while its renderers are not seen (Unity's `cullingMode`).
        #[property]
        pub fn culling_mode(self) -> CullingMode {
            animator(self, |p, _| p.culling_mode).unwrap_or_default()
        }

        pub fn set_culling_mode(self, mode: CullingMode) {
            animator(self, |p, _| p.culling_mode = mode);
        }

        /// Off, the frame's update leaves it alone (it holds its pose); `update(dt)` still runs
        /// it (Unity's manual pattern).
        #[property]
        pub fn enabled(self) -> bool {
            animator(self, |p, _| p.enabled).unwrap_or(false)
        }

        pub fn set_enabled(self, on: bool) {
            animator(self, |p, _| p.enabled = on);
        }

        /// Unity's `Animator.Update(deltaTime)`: advances the clock by `deltaTime` seconds, runs
        /// the transitions and writes the pose now, seen or not (culling is ignored). The frame's
        /// own update advances it too unless `enabled` is false: call this on a disabled animator,
        /// or it moves twice as fast.
        pub fn update(self, delta_time: f64) {
            crate::animation::ensure_registered();
            let events = with_world(|w| {
                let i = (self.0 as usize).checked_sub(1)?;
                let mut p = w.animators.get(i)?.clone();
                let mut events = Vec::new();
                platypus_scene::api::with_scene(|scene| {
                    let mut sink = platypus_animation::RegistrySink { other: None };
                    p.animate_seen(w, scene, &mut sink, delta_time, &mut events, true);
                });
                w.animators[i] = p;
                Some(events)
            })
            .unwrap_or_default();
            crate::animation::keep_events(&events);
        }

        pub fn set_float(self, name: &str, value: f64) {
            animator(self, |p, _| p.set_float(name, value));
        }

        pub fn get_float(self, name: &str) -> f64 {
            animator(self, |p, w| p.get_float(w, name)).unwrap_or(0.0)
        }

        pub fn set_integer(self, name: &str, value: f64) {
            animator(self, |p, _| p.set_integer(name, value));
        }

        pub fn get_integer(self, name: &str) -> f64 {
            animator(self, |p, w| p.get_integer(w, name)).unwrap_or(0.0)
        }

        pub fn set_bool(self, name: &str, value: bool) {
            animator(self, |p, _| p.set_bool(name, value));
        }

        pub fn get_bool(self, name: &str) -> bool {
            animator(self, |p, w| p.get_bool(w, name)).unwrap_or(false)
        }

        /// On until a transition uses it.
        pub fn set_trigger(self, name: &str) {
            animator(self, |p, _| p.set_trigger(name));
        }

        pub fn reset_trigger(self, name: &str) {
            animator(self, |p, _| p.reset_trigger(name));
        }

        /// Jumps to a state (no fade), at a normalized time.
        #[optional(normalized_time)]
        pub fn play(self, state_name: &str, normalized_time: f64) {
            let t = if normalized_time.is_nan() {
                0.0
            } else {
                normalized_time
            };
            animator(self, |p, w| p.play(w, state_name, t));
        }

        /// Fades to a state over `duration` seconds (0.25 by default).
        #[optional(duration, normalized_time)]
        pub fn cross_fade(self, state_name: &str, duration: f64, normalized_time: f64) {
            let d = if duration.is_nan() { 0.25 } else { duration };
            let t = if normalized_time.is_nan() {
                0.0
            } else {
                normalized_time
            };
            animator(self, |p, w| p.cross_fade(w, state_name, d, t));
        }

        #[property]
        pub fn is_in_transition(self) -> bool {
            animator(self, |p, _| p.is_in_transition()).unwrap_or(false)
        }

        /// The state it is in (`currentStateName`, `currentNormalizedTime` and `currentLength` together).
        #[js_name = "getCurrentAnimatorStateInfo"]
        pub fn current_animator_state_info(self) -> StateSummary {
            animator(self, |p, w| {
                let i = p.current_info(w);
                StateSummary {
                    name: i.name,
                    normalized_time: i.normalized_time,
                    length: i.length,
                }
            })
            .unwrap_or(StateSummary {
                name: String::new(),
                normalized_time: 0.0,
                length: 0.0,
            })
        }

        /// The name of the state it is in.
        #[property]
        pub fn current_state_name(self) -> String {
            animator(self, |p, w| p.current_info(w).name).unwrap_or_default()
        }

        /// Time over the state length (past 1 after loops).
        #[property]
        pub fn current_normalized_time(self) -> f64 {
            animator(self, |p, w| p.current_info(w).normalized_time).unwrap_or(0.0)
        }

        /// The length of the state it is in, in seconds.
        #[property]
        pub fn current_length(self) -> f64 {
            animator(self, |p, w| p.current_info(w).length).unwrap_or(0.0)
        }

        /// The state it is fading to ("" if none).
        #[property]
        pub fn next_state_name(self) -> String {
            animator(self, |p, w| p.next_info(w).name).unwrap_or_default()
        }

        #[property]
        pub fn next_normalized_time(self) -> f64 {
            animator(self, |p, w| p.next_info(w).normalized_time).unwrap_or(0.0)
        }

        #[property]
        pub fn next_length(self) -> f64 {
            animator(self, |p, w| p.next_info(w).length).unwrap_or(0.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::AnimationRef;
    use platypus_animation::CullingType;

    // M5-ANI-12
    #[test]
    fn culling_modes_are_set_on_the_components_and_list_their_objects() {
        crate::animation::reset();
        let mut scene = platypus_scene::Scene::new();
        let (a, b) = (scene.create("a"), scene.create("b"));
        let animator = AnimatorRef::attach(a);
        let animation = AnimationRef::attach(b);
        assert_eq!(animator.culling_mode(), CullingMode::AlwaysAnimate);
        assert_eq!(animation.culling_type(), CullingType::AlwaysAnimate);
        assert!(crate::animation::culled_roots().is_empty());
        animator.set_culling_mode(CullingMode::CullCompletely);
        animation.set_culling_type(CullingType::BasedOnRenderers);
        assert_eq!(animator.culling_mode(), CullingMode::CullCompletely);
        assert_eq!(crate::animation::culled_roots(), [a, b]);
        crate::animation::reset();
    }

    /// Never seen.
    struct Unseen;

    impl platypus_animation::Visibility for Unseen {
        fn visible(&mut self, _: &platypus_scene::Scene, _: ObjectId) -> bool {
            false
        }
    }

    // M5-ANI-15
    #[test]
    fn update_poses_a_culled_or_disabled_animator_by_hand() {
        use crate::animation::{ClipRef, CurveRef};
        crate::animation::reset();
        let mut scene = platypus_scene::Scene::new();
        let go = scene.create("Figure");
        let arm = scene.create("Arm");
        scene.set_parent(arm, Some(go), false);
        let _scope = platypus_scene::api::enter(&mut scene);
        // A looping 2 s clip moving the arm's x from 0 to 2, an event at 1 s.
        let clip = ClipRef::create("swing");
        clip.set_wrap_mode("loop");
        clip.set_curve(
            "Arm",
            "Transform",
            "localPosition.x",
            CurveRef::linear(0.0, 0.0, 2.0, 2.0),
        );
        clip.add_event(1.0, "Step", f64::NAN);
        let controller = ControllerRef::create();
        controller.add_node(NodeRef::create("Swing", clip));
        let animator = AnimatorRef::attach(go);
        animator.set_controller(controller);
        animator.set_culling_mode(CullingMode::CullCompletely);
        let x = || platypus_scene::api::with_scene(|s| s.local(arm).unwrap().position.x).unwrap();
        let frame = |dt| {
            crate::animation::update_culled(dt, None, &mut Unseen);
        };
        frame(0.5);
        assert_eq!(x(), 0.0, "culled: the frame does not pose it");
        animator.update(0.5);
        assert_eq!(x(), 0.5, "update poses it anyway");
        animator.update(0.75);
        assert_eq!(x(), 1.25);
        let events = crate::animation::animations_api::take_events();
        assert_eq!(events, "Figure|Step|", "its events are kept");
        // Disabled: the frame leaves it alone and it is not watched; update still runs it.
        animator.set_culling_mode(CullingMode::AlwaysAnimate);
        animator.set_enabled(false);
        assert!(!animator.enabled());
        frame(0.25);
        assert_eq!(x(), 1.25);
        animator.update(0.25);
        assert_eq!(x(), 1.5);
        // Enabled with automatic updates too: it advances twice.
        animator.set_enabled(true);
        frame(0.1);
        animator.update(0.1);
        assert!((x() - 1.7).abs() < 1e-6, "{}", x());
        crate::animation::reset();
    }
}
