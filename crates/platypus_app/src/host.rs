// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Which renderer this run draws with. `platypus-app` is the one host of every game: the
//! renderer comes from the command line (`--renderer`), then the project (`renderer` in
//! `project.json`, which `platypus-build package --renderer` writes into a packaged copy), then
//! what the compiled program uses (the bundle's tier, found at build time): a program that is
//! only UI starts on the light GLES path, one that uses the world on wgpu with the world
//! renderer. The editor always has the world.

use std::sync::atomic::{AtomicU8, Ordering};

use platypus_core::project::Renderer;
use platypus_core::renderer::Tier;

/// The renderer a run starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    /// wgpu with the world renderer.
    World,
    /// GLES through EGL (static ANGLE on Windows), else wgpu with small GPU blocks; UI only.
    Gles,
    /// The CPU renderer through softbuffer, else as `Gles`'s fallback; UI only.
    Cpu,
}

static CHOICE: AtomicU8 = AtomicU8::new(0);

impl Choice {
    fn index(self) -> u8 {
        match self {
            Choice::World => 0,
            Choice::Gles => 1,
            Choice::Cpu => 2,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Choice::World => "vulkan",
            Choice::Gles => "gles",
            Choice::Cpu => "cpu",
        }
    }
}

/// The renderer for a program of `tier`: the first of `cli` and `project` that is set, else
/// from the tier.
pub(crate) fn resolve(cli: Renderer, project: Renderer, tier: Tier) -> Choice {
    match Renderer::first_set([cli, project]) {
        Renderer::Vulkan => Choice::World,
        Renderer::Gles => Choice::Gles,
        Renderer::Cpu => Choice::Cpu,
        Renderer::Auto if tier == Tier::Ui => Choice::Gles,
        Renderer::Auto => Choice::World,
    }
}

/// The warning for a UI-only renderer asked to run a program that uses the world.
pub(crate) fn world_warning(choice: Choice, tier: Tier, uses: &[String]) -> Option<String> {
    if choice == Choice::World {
        return None;
    }
    platypus_core::renderer::world_warning(choice.name(), tier, uses)
}

/// Sets this run's renderer (once, before the window opens).
pub(crate) fn set(choice: Choice) {
    CHOICE.store(choice.index(), Ordering::Relaxed);
}

/// This run's renderer.
pub(crate) fn choice() -> Choice {
    match CHOICE.load(Ordering::Relaxed) {
        1 => Choice::Gles,
        2 => Choice::Cpu,
        _ => Choice::World,
    }
}

/// Whether this run takes a light path: no world renderer unless something asks for it.
pub(crate) fn ui_only() -> bool {
    choice() != Choice::World
}

#[cfg(test)]
mod tests {
    use super::*;
    use Renderer::*;

    // M9-HOST-10
    #[test]
    fn an_automatic_renderer_follows_the_programs_tier() {
        assert_eq!(resolve(Auto, Auto, Tier::Ui), Choice::Gles);
        assert_eq!(resolve(Auto, Auto, Tier::World2d), Choice::World);
        assert_eq!(resolve(Auto, Auto, Tier::World3d), Choice::World);
    }

    // M9-HOST-11
    #[test]
    fn the_command_line_beats_the_project_which_beats_the_tier() {
        assert_eq!(resolve(Auto, Vulkan, Tier::Ui), Choice::World);
        assert_eq!(resolve(Auto, Gles, Tier::World3d), Choice::Gles);
        assert_eq!(resolve(Auto, Cpu, Tier::Ui), Choice::Cpu);
        assert_eq!(resolve(Cpu, Vulkan, Tier::Ui), Choice::Cpu);
        assert_eq!(resolve(Vulkan, Gles, Tier::Ui), Choice::World);
        assert_eq!(resolve(Gles, Cpu, Tier::World3d), Choice::Gles);
    }

    // M9-HOST-12
    #[test]
    fn a_ui_only_renderer_for_a_world_game_warns_naming_what_it_uses() {
        let uses = vec!["Camera".to_string(), "MeshRenderer".to_string()];
        let w = world_warning(Choice::Gles, Tier::World3d, &uses).unwrap();
        assert!(
            w.contains("renderer is gles") && w.contains("Camera, MeshRenderer"),
            "{w}"
        );
        assert!(w.contains("3D Not Supported"));
        assert!(world_warning(Choice::Cpu, Tier::World2d, &uses).is_some());
        assert!(world_warning(Choice::World, Tier::World3d, &uses).is_none());
        assert!(world_warning(Choice::Gles, Tier::Ui, &[]).is_none());
    }
}
