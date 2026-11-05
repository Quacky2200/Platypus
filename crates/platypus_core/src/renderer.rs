// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What a program needs from the renderer, found at build time: the highest tier among the
//! engine classes and namespaces its compiled code calls (`Output::externs`, after the unused
//! top-level functions were left out) and the component types its scene files name. The host
//! picks its renderer from it (`docs/rendering.md`, "Choosing the renderer").
//!
//! The tags live here, in one table keyed on the API's owner names (a class or a namespace of
//! the `#[ts_api]` manifests, or the prelude's `Component`); a name that isn't listed needs
//! nothing beyond the UI. Only code counts: names in strings or comments, and top-level
//! functions nothing calls, don't.
//! `platypus_build`'s tests check the table against the real API: every class and namespace of
//! `platypus_render_api` is listed, and every listed name exists.

use std::collections::{BTreeSet, HashMap};

/// What a program needs from the renderer, from least to most.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Only the UI: the light renderer (GLES through ANGLE on Windows) can draw it.
    #[default]
    Ui,
    /// A 2D world: cameras, sprites, 2D lines and shapes, world text.
    World2d,
    /// The 3D world: meshes, materials, lights, skies, post-processing, world panels.
    World3d,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Ui => "ui",
            Tier::World2d => "world2d",
            Tier::World3d => "world3d",
        }
    }

    pub fn parse(s: &str) -> Option<Tier> {
        match s {
            "ui" => Some(Tier::Ui),
            "world2d" => Some(Tier::World2d),
            "world3d" => Some(Tier::World3d),
            _ => None,
        }
    }
}

/// The prelude class of components (`class Player extends Component`, `register(..)`): not an
/// engine class of the manifests, the compiler notes its use itself (`Output::uses`).
pub const COMPONENT: &str = "Component";

/// Names of the table that are not classes or namespaces of the manifests.
pub const PRELUDE_NAMES: &[&str] = &[COMPONENT];

/// The engine API that needs the world renderer, by owner name. Cameras are 2D: a 2D game needs
/// one too, and what makes a world 3D is what it draws. So is the scene itself: a program with a
/// `GameObject`, a component class or a scene file runs the world, even if it draws only UI.
pub const TAGS: &[(&str, Tier)] = &[
    // A 2D world.
    ("Camera", Tier::World2d),
    ("Cameras", Tier::World2d),
    ("CameraFollow", Tier::World2d),
    (COMPONENT, Tier::World2d),
    ("GameObject", Tier::World2d),
    ("Prefab", Tier::World2d),
    ("Scene", Tier::World2d),
    ("SceneManager", Tier::World2d),
    ("Transform", Tier::World2d),
    ("LineRenderer2D", Tier::World2d),
    ("Sprite", Tier::World2d),
    ("SpriteRenderer", Tier::World2d),
    ("TextMesh", Tier::World2d),
    ("Texture", Tier::World2d),
    // The 3D world.
    ("CameraOrbit", Tier::World3d),
    ("Cloth", Tier::World3d),
    ("FogVolume", Tier::World3d),
    ("Gizmos", Tier::World3d),
    ("GraphicsSettings", Tier::World3d),
    ("LODGroup", Tier::World3d),
    ("Light", Tier::World3d),
    ("LineRenderer", Tier::World3d),
    ("Material", Tier::World3d),
    ("Mesh", Tier::World3d),
    ("MeshRenderer", Tier::World3d),
    ("Mirror", Tier::World3d),
    ("Model", Tier::World3d),
    ("ParticleSystem", Tier::World3d),
    ("PlaneRenderer", Tier::World3d),
    ("PostProcess", Tier::World3d),
    ("PrimitiveType", Tier::World3d),
    ("Primitives", Tier::World3d),
    ("ReflectionProbe", Tier::World3d),
    ("RenderSettings", Tier::World3d),
    ("RenderStats", Tier::World3d),
    ("Rope", Tier::World3d),
    ("SkinnedMeshRenderer", Tier::World3d),
    ("WorldPanel", Tier::World3d),
];

/// The tier of one owner name (`Ui` when it isn't listed).
pub fn tier_of(owner: &str) -> Tier {
    TAGS.iter()
        .find(|(n, _)| *n == owner)
        .map_or(Tier::Ui, |(_, t)| *t)
}

/// A program's tier and the names that set it (those above `Ui`, sorted).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub tier: Tier,
    pub uses: Vec<String>,
}

impl Usage {
    /// Adds the owner names a program uses.
    pub fn add<'a>(&mut self, owners: impl IntoIterator<Item = &'a str>) {
        let mut uses: BTreeSet<String> = self.uses.drain(..).collect();
        for o in owners {
            let t = tier_of(o);
            if t > Tier::Ui {
                self.tier = self.tier.max(t);
                uses.insert(o.to_string());
            }
        }
        self.uses = uses.into_iter().collect();
    }
}

/// The warning for a UI-only renderer (`gles`, `cpu`) chosen for a program of `tier` that uses
/// the world (`uses`); `None` when the program is only UI.
pub fn world_warning(renderer: &str, tier: Tier, uses: &[String]) -> Option<String> {
    (tier > Tier::Ui).then(|| {
        format!(
            "renderer is {renderer} but this game uses the world ({}): it will show 3D Not Supported",
            uses.join(", ")
        )
    })
}

/// Owner (class or namespace) of every function symbol in `manifests`.
pub fn symbol_owners(manifests: &[String]) -> HashMap<String, String> {
    let mut owners = HashMap::new();
    for m in manifests {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(m) else {
            continue;
        };
        let ns = v["namespace"].as_str().unwrap_or_default();
        for f in v["functions"].as_array().into_iter().flatten() {
            if let Some(s) = f["symbol"].as_str() {
                owners.insert(s.to_string(), ns.to_string());
            }
        }
        for c in v["classes"].as_array().into_iter().flatten() {
            let cname = c["name"].as_str().unwrap_or_default();
            for f in c["functions"].as_array().into_iter().flatten() {
                if let Some(s) = f["symbol"].as_str() {
                    owners.insert(s.to_string(), cname.to_string());
                }
            }
        }
    }
    owners
}

/// Every class and namespace name the manifests declare.
pub fn owner_names(manifests: &[String]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for m in manifests {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(m) else {
            continue;
        };
        if let Some(ns) = v["namespace"].as_str() {
            names.insert(ns.to_string());
        }
        for c in v["classes"].as_array().into_iter().flatten() {
            if let Some(n) = c["name"].as_str() {
                names.insert(n.to_string());
            }
        }
        for e in v["enums"].as_array().into_iter().flatten() {
            if let Some(n) = e["name"].as_str() {
                names.insert(n.to_string());
            }
        }
    }
    names
}

/// The usage of a compiled program: the owners of the extern symbols it calls, and the names it
/// uses without a call (`Output::uses`).
pub fn usage_of_symbols(symbols: &[String], uses: &[String], manifests: &[String]) -> Usage {
    let owners = symbol_owners(manifests);
    let mut usage = Usage::default();
    usage.add(
        symbols
            .iter()
            .filter_map(|s| owners.get(s).map(String::as_str))
            .chain(uses.iter().map(String::as_str)),
    );
    usage
}

/// The names a scene or prefab file brings: its component and asset types, and `GameObject`,
/// since loading one makes objects.
pub fn scene_file_names(json: &str) -> BTreeSet<String> {
    let mut names = scene_types(json);
    names.insert("GameObject".to_string());
    names
}

/// The component and asset types a scene or prefab file names (`"type": "MeshRenderer"`,
/// `"$type": "Material"`).
pub fn scene_types(json: &str) -> BTreeSet<String> {
    fn walk(v: &serde_json::Value, out: &mut BTreeSet<String>) {
        match v {
            serde_json::Value::Object(o) => {
                for (k, x) in o {
                    if (k == "type" || k == "$type")
                        && let Some(s) = x.as_str()
                    {
                        out.insert(s.to_string());
                    }
                    walk(x, out);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
        walk(&v, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-HOST-01
    #[test]
    fn the_highest_tier_wins_and_only_world_names_are_listed() {
        let mut u = Usage::default();
        u.add(["Time", "Vector3"]);
        assert_eq!(u.tier, Tier::Ui);
        assert!(u.uses.is_empty());
        u.add(["TextMesh"]);
        assert_eq!(u.tier, Tier::World2d);
        u.add(["MeshRenderer", "Camera", "TextMesh"]);
        assert_eq!(u.tier, Tier::World3d);
        assert_eq!(u.uses, ["Camera", "MeshRenderer", "TextMesh"]);
    }

    // M9-HOST-02
    #[test]
    fn scene_files_name_their_component_and_asset_types() {
        let t = scene_types(
            r#"{"objects":[{"components":[{"type":"MeshRenderer","properties":{"material":{"$type":"Material"}}}]}]}"#,
        );
        assert_eq!(
            t.into_iter().collect::<Vec<_>>(),
            ["Material", "MeshRenderer"]
        );
    }

    // M9-HOST-15
    #[test]
    fn a_scene_file_is_game_objects() {
        let t = scene_file_names(r#"{"objects":[{"components":[{"type":"Spinner"}]}]}"#);
        assert_eq!(t.into_iter().collect::<Vec<_>>(), ["GameObject", "Spinner"]);
        let mut u = Usage::default();
        u.add(["Spinner", "GameObject"]);
        assert_eq!(
            (u.tier, u.uses.as_slice()),
            (Tier::World2d, &["GameObject".to_string()][..])
        );
    }

    // M9-HOST-03
    #[test]
    fn tiers_round_trip_through_their_names() {
        for t in [Tier::Ui, Tier::World2d, Tier::World3d] {
            assert_eq!(Tier::parse(t.as_str()), Some(t));
        }
        assert_eq!(Tier::parse("3d"), None);
    }
}
