// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Animation culling's eyes (`Animator.cullingMode`, `Animation.cullingType`): which renderers
//! under culled animated objects the renderer saw. Each frame the animation update asks about the
//! last frame's drawing (one frame late), then the renderers under culled objects are watched while
//! this frame draws (`MeshInstance::visibility_key`, `WorldRenderer::seen`).
//!
//! **The safeguard** (Platypus's, Unity has none): a culled animator writes no bone transforms, so
//! what hangs on its bones would be left behind. An animated object is never culled (always
//! seen) while any object strictly below it has a physics component (Rigidbody, a collider, a
//! joint, a rope; 2D too), an `AudioSource`, a `ParticleSystem`, a `Light` or a script component
//! (a game's class). A warning names the first such object and component, once per animated
//! object. The answer is kept until the set of components changes (`platypus_kinds::
//! components_version`) or the culled objects or their tree versions do.

use super::render_sync::SKIN_BASE;
use platypus_scene::{ObjectId, Scene, ScriptObject, ScriptScene};
use std::collections::{HashMap, HashSet};
use ts_api::Handle;

#[derive(Default)]
pub(crate) struct Watch {
    /// Renderers (component ids) whose sight the renderer reports this frame.
    watched: HashSet<u32>,
    /// Those of the frame last drawn, and which of them it saw.
    drawn: HashSet<u32>,
    seen: HashSet<u32>,
    guard: Guard,
}

/// Which culled objects the safeguard keeps animating, and what it was worked out for.
#[derive(Default)]
struct Guard {
    /// The components version, and each culled object with its tree version.
    key: Option<(u64, Vec<(ObjectId, u64)>)>,
    /// Kept animating: the object below and the component that block culling.
    blocked: HashMap<ObjectId, (ObjectId, &'static str)>,
    warned: HashSet<ObjectId>,
}

/// The warning for an animated object the safeguard keeps animating.
pub(crate) fn blocked_message(root: &str, object: &str, component: &str) -> String {
    format!(
        "animation culling is off for `{root}`: `{object}` below it has a {component}, which a \
         culled animation would leave behind"
    )
}

impl Guard {
    /// Drops from `roots` those with a blocking component below them (`blockers`, asked only
    /// when something changed). Returns the warnings not given before.
    fn filter(
        &mut self,
        scene: &Scene,
        roots: &mut Vec<ObjectId>,
        components: u64,
        blockers: impl FnOnce() -> Vec<(ObjectId, &'static str)>,
    ) -> Vec<String> {
        let key = (
            components,
            roots.iter().map(|&r| (r, scene.tree_version(r))).collect(),
        );
        let mut warnings = Vec::new();
        if self.key.as_ref() != Some(&key) {
            self.blocked.clear();
            let culled: HashSet<ObjectId> = roots.iter().copied().collect();
            for (object, component) in blockers() {
                let mut at = scene.parent(object);
                while let Some(a) = at {
                    if culled.contains(&a) {
                        self.blocked.entry(a).or_insert((object, component));
                    }
                    at = scene.parent(a);
                }
            }
            for &r in roots.iter() {
                if let Some(&(object, component)) = self.blocked.get(&r)
                    && self.warned.insert(r)
                {
                    let name = |o| scene.name(o).unwrap_or("");
                    warnings.push(blocked_message(name(r), name(object), component));
                }
            }
            self.key = Some(key);
        }
        if !self.blocked.is_empty() {
            roots.retain(|r| !self.blocked.contains_key(r));
        }
        warnings
    }
}

/// Every component the safeguard looks for, with its object: physics components, `AudioSource`s,
/// `ParticleSystem`s, `Light`s and script components.
pub(crate) fn blockers() -> Vec<(ObjectId, &'static str)> {
    let mut out = platypus_physics_api::component_objects();
    out.extend(
        platypus_engine_api::audio_components::source_objects()
            .into_iter()
            .map(|o| (o, "AudioSource")),
    );
    platypus_render_api::for_each_particle_system(|_, d| out.push((d.object, "ParticleSystem")));
    platypus_render_api::for_each_light(|_, d| out.push((d.object, "Light")));
    out.extend(
        platypus_components::live_components()
            .into_iter()
            .filter(|&(o, _)| o != 0)
            .map(|(o, class)| (ObjectId::from_raw(o), class)),
    );
    out
}

/// Whether each culled object had a renderer seen; objects with no renderers are seen.
pub(crate) struct Sight(HashMap<ObjectId, bool>);

impl platypus_animation::Visibility for Sight {
    fn visible(&mut self, _: &platypus_scene::Scene, root: ObjectId) -> bool {
        self.0.get(&root).copied().unwrap_or(true)
    }
}

impl Watch {
    /// The key a renderer's mesh is drawn with (0: not watched).
    pub(crate) fn key(&self, renderer: u32) -> u32 {
        if self.watched.contains(&renderer) {
            renderer + 1
        } else {
            0
        }
    }

    /// The renderer's report on the frame just drawn (keys from [`Self::key`]), passed on to
    /// `Renderer.isVisible`.
    pub(crate) fn note_seen(&mut self, keys: &[u32]) {
        self.drawn.clone_from(&self.watched);
        self.seen.clear();
        self.seen.extend(keys.iter().map(|k| k - 1));
        platypus_render_api::set_seen_renderers(self.seen.iter().map(|&id| renderer_id(id)));
    }

    /// What the last frame saw of each of `roots` (through any renderer on it or below it), and
    /// watches those renderers this frame, and those scripts asked `isVisible` of. A renderer
    /// not watched last frame counts as seen; the safeguard's roots (`blockers`, see the module
    /// doc) are always seen.
    pub(crate) fn sight(
        &mut self,
        scene: &ScriptScene,
        renderers: impl Iterator<Item = (u32, ScriptObject)>,
        roots: &[ObjectId],
        blockers: impl FnOnce() -> Vec<(ObjectId, &'static str)>,
    ) -> Sight {
        let mut roots = roots.to_vec();
        let components = platypus_kinds::components_version();
        for w in self
            .guard
            .filter(&scene.scene, &mut roots, components, blockers)
        {
            tracing::warn!(target: "animation", "{w}");
        }
        self.watched.clear();
        self.watched
            .extend(
                platypus_render_api::watched_renderers()
                    .into_iter()
                    .map(|r| match r {
                        platypus_render_api::RendererId::Mesh(id) => id,
                        platypus_render_api::RendererId::Skinned(id) => SKIN_BASE + id,
                    }),
            );
        let mut out = HashMap::new();
        if roots.is_empty() {
            return Sight(out);
        }
        let roots: HashSet<ObjectId> = roots.iter().copied().collect();
        for (id, object) in renderers {
            let Some(go) = scene.object(object) else {
                continue;
            };
            let seen = !self.drawn.contains(&id) || self.seen.contains(&id);
            let mut at = Some(go);
            while let Some(a) = at {
                if roots.contains(&a) {
                    self.watched.insert(id);
                    *out.entry(a).or_insert(false) |= seen;
                }
                at = scene.scene.parent(a);
            }
        }
        Sight(out)
    }
}

/// A renderer table id as its component (skinned renderers from `SKIN_BASE`).
fn renderer_id(id: u32) -> platypus_render_api::RendererId {
    match id.checked_sub(SKIN_BASE) {
        Some(skinned) => platypus_render_api::RendererId::Skinned(skinned),
        None => platypus_render_api::RendererId::Mesh(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_animation::Visibility;

    /// A log written to memory.
    #[derive(Clone, Default)]
    struct Log(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Log {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // M5-ANI-13
    #[test]
    fn a_culled_object_is_seen_through_any_renderer_below_it_one_frame_late() {
        let mut scene = ScriptScene::new();
        let figure = scene.create_engine_object("Figure");
        let body = scene.create_engine_object("Body");
        let empty = scene.create_engine_object("Empty");
        let (f, b, e) = (
            scene.object(figure).unwrap(),
            scene.object(body).unwrap(),
            scene.object(empty).unwrap(),
        );
        scene.scene.set_parent(b, Some(f), false);
        let mut w = Watch::default();
        let renderers = || [(7u32, body)].into_iter();
        // Not watched before: seen (no report yet); and now watched.
        let mut s = w.sight(&scene, renderers(), &[f, e], Vec::new);
        assert!(s.visible(&scene.scene, f));
        assert!(s.visible(&scene.scene, e), "no renderers: always seen");
        assert_eq!(w.key(7), 8);
        assert_eq!(w.key(9), 0);
        // The frame drew without seeing it.
        w.note_seen(&[]);
        let mut s = w.sight(&scene, renderers(), &[f], Vec::new);
        assert!(!s.visible(&scene.scene, f));
        w.note_seen(&[8]);
        assert!(
            w.sight(&scene, renderers(), &[f], Vec::new)
                .visible(&scene.scene, f)
        );
        // No culled objects: nothing watched.
        w.sight(&scene, renderers(), &[], Vec::new);
        assert_eq!(w.key(7), 0);
    }

    // M5-ANI-14
    #[test]
    fn a_figure_with_a_collider_or_script_below_it_is_never_culled_and_warns_once() {
        use std::cell::Cell;
        let mut scene = ScriptScene::new();
        let mut make = |name: &str| {
            let o = scene.create_engine_object(name);
            (o, scene.object(o).unwrap())
        };
        let (_, f) = make("Figure");
        let (body, b) = make("Body");
        let (_, hand) = make("Hand");
        let (_, o) = make("Other");
        let (other_body, ob) = make("OtherBody");
        let (_, sword) = make("Sword");
        scene.scene.set_parent(b, Some(f), false);
        scene.scene.set_parent(hand, Some(b), false);
        scene.scene.set_parent(ob, Some(o), false);
        let mut w = Watch::default();
        let renderers = || [(1u32, body), (2u32, other_body)].into_iter();
        let asked = Cell::new(0);
        // The hand holds a collider; a script sits on the figure itself (not below: no block).
        let blockers = || {
            asked.set(asked.get() + 1);
            vec![(hand, "BoxCollider"), (f, "Walker")]
        };
        let log = Log::default();
        let writer = log.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..3 {
                let mut s = w.sight(&scene, renderers(), &[f, o], blockers);
                w.note_seen(&[]);
                assert!(s.visible(&scene.scene, f), "kept animating");
                assert_eq!(w.key(1), 0, "and not watched");
                assert_eq!(w.key(2), 3);
            }
        });
        let s = w.sight(&scene, renderers(), &[f, o], blockers);
        assert!(s.0.get(&o) == Some(&false), "the other figure is culled");
        assert_eq!(asked.get(), 1, "worked out once while nothing changed");
        let text = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
        let line = format!(
            "WARN animation: {}",
            blocked_message("Figure", "Hand", "BoxCollider")
        );
        assert_eq!(
            text.lines().map(str::trim).collect::<Vec<_>>(),
            [line.as_str()],
            "one warning naming the object and component"
        );
        // A component added anywhere, or a new link below a culled object: worked out again.
        platypus_kinds::components_changed();
        w.sight(&scene, renderers(), &[f, o], blockers);
        assert_eq!(asked.get(), 2);
        scene.scene.set_parent(sword, Some(ob), false);
        let mut s = w.sight(&scene, renderers(), &[f, o], || {
            asked.set(asked.get() + 1);
            vec![(sword, "AudioSource")]
        });
        assert_eq!(asked.get(), 3);
        assert!(
            s.visible(&scene.scene, o),
            "the sword's sound keeps it going"
        );
        // The collider gone (the next version): the figure culls again.
        platypus_kinds::components_changed();
        w.note_seen(&[]);
        w.sight(&scene, renderers(), &[f, o], Vec::new);
        w.note_seen(&[]);
        let mut s = w.sight(&scene, renderers(), &[f, o], Vec::new);
        assert!(!s.visible(&scene.scene, f));
    }

    // M5-ANI-14
    #[test]
    fn the_safeguard_finds_physics_lights_particles_sounds_and_scripts() {
        let mut scene = platypus_scene::Scene::new();
        let (a, b, c, d) = (
            scene.create("a"),
            scene.create("b"),
            scene.create("c"),
            scene.create("d"),
        );
        let _scope = platypus_scene::api::enter(&mut scene);
        platypus_render_api::reset();
        let light = platypus_render_api::attach_light(a);
        let particles = platypus_render_api::attach_particle_system(b);
        let source = platypus_engine_api::audio_components::attach_source(c);
        let collider = platypus_physics_api::BoxColliderRef::attach(d);
        let found = blockers();
        for want in [
            (a, "Light"),
            (b, "ParticleSystem"),
            (c, "AudioSource"),
            (d, "BoxCollider"),
        ] {
            assert!(found.contains(&want), "{want:?} in {found:?}");
        }
        let v = platypus_kinds::components_version();
        platypus_render_api::remove_light(light);
        assert!(platypus_kinds::components_version() > v, "removing counts");
        platypus_render_api::remove_particle_system(particles);
        platypus_engine_api::audio_components::remove_source(source);
        collider.destroy();
        assert!(blockers().is_empty());
        platypus_render_api::reset();
    }

    // M5-CUL-04
    #[test]
    fn renderers_scripts_ask_is_visible_of_are_watched_and_told_what_was_seen() {
        platypus_render_api::reset();
        let scene = ScriptScene::new();
        let mut w = Watch::default();
        let (mesh, skinned) = (
            platypus_render_api::MeshRendererRef(4),
            platypus_render_api::SkinnedMeshRendererRef(2),
        );
        let skin_id = SKIN_BASE + 2;
        assert!(!mesh.is_visible() && !skinned.is_visible());
        // Asked: watched with no animator culling at all.
        w.sight(&scene, std::iter::empty(), &[], Vec::new);
        assert_eq!(w.key(4), 5);
        assert_eq!(w.key(skin_id), skin_id + 1);
        w.note_seen(&[skin_id + 1]);
        assert!(!mesh.is_visible());
        assert!(skinned.is_visible());
        w.note_seen(&[5]);
        assert!(mesh.is_visible() && !skinned.is_visible());
        platypus_render_api::reset();
    }
}
