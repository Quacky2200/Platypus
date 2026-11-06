// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Physics components from compiled programs (`bench/physics_comp.ts`, `platypus_physics_api`
//! components, joints, `Rope`, `CharacterController` and 2D). IDs are in TESTS.md (M9-TS-1100..).
use platypus_core::Value;
use platypus_ts_tests::physics_comp_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    let run = || {
        platypus_physics_api::reset();
        let mut scene = platypus_scene::Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        unsafe { ts_phc_init() };
        f();
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
}

fn text(p: *mut u8) -> String {
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn arr(v: &[f64]) -> Value {
    Value::Array(v.iter().copied().map(n).collect())
}

// M9-TS-1100
//
// A ball with a Rigidbody and a SphereCollider falls onto a BoxCollider floor and gives the same
// height as the same world built through the physics ops (`platypus_physics::Physics::apply`).
#[test]
fn components_match_the_ops_they_stand_for() {
    in_every_gc_mode(|| unsafe {
        let y = ts_phc_compFall(100.0);
        // The reference: a fixed floor body, a dynamic body with a ball, friction 0.6, mass 1.
        let mut p = platypus_physics::Physics::new();
        let call = |p: &mut platypus_physics::Physics, name: &str, args: Vec<Value>| {
            p.apply(name, &args).unwrap();
        };
        let opts = |friction: f64| {
            Value::Array(vec![
                n(friction),
                n(0.0),
                n(1.0),
                Value::Bool(false),
                Value::Bool(true),
                Value::Null,
            ])
        };
        let ident = || arr(&[0.0, 0.0, 0.0, 1.0]);
        let body_opts = || {
            Value::Array(vec![
                n(1.0),
                n(0.0),
                n(0.0),
                Value::Bool(false),
                Value::Bool(false),
            ])
        };
        call(
            &mut p,
            "body",
            vec![
                n(1.0),
                Value::String("fixed".into()),
                arr(&[0.0; 3]),
                ident(),
                arr(&[0.0; 3]),
                body_opts(),
            ],
        );
        call(
            &mut p,
            "collider",
            vec![
                n(1.0),
                n(1.0),
                Value::String("box".into()),
                arr(&[10.0, 0.5, 10.0]),
                arr(&[0.0; 3]),
                ident(),
                opts(0.6),
            ],
        );
        call(
            &mut p,
            "body",
            vec![
                n(2.0),
                Value::String("dynamic".into()),
                arr(&[0.0; 3]),
                ident(),
                arr(&[0.0; 3]),
                body_opts(),
            ],
        );
        call(
            &mut p,
            "set",
            vec![
                n(2.0),
                Value::String("position".into()),
                arr(&[0.0, 5.0, 0.0]),
            ],
        );
        call(
            &mut p,
            "collider",
            vec![
                n(2.0),
                n(2.0),
                Value::String("ball".into()),
                arr(&[0.5]),
                arr(&[0.0; 3]),
                ident(),
                opts(0.6),
            ],
        );
        call(
            &mut p,
            "set",
            vec![n(2.0), Value::String("mass".into()), n(1.0)],
        );
        for _ in 0..100 {
            p.step();
        }
        let reference = f64::from(p.position(2).unwrap()[1]);
        assert!((y - reference).abs() < 1e-4, "{y} vs {reference}");
    });
}

// M9-TS-1101
//
// Properties set by `addComponent` options read back through `getComponent`.
#[test]
fn component_properties_round_trip() {
    in_every_gc_mode(|| unsafe {
        // size.x 3 + center.y 2 * 10 + trigger 100 + bounciness 0.25 * 1000.
        assert_eq!(ts_phc_colliderProps(), 3.0 + 20.0 + 100.0 + 250.0);
        // mass 4 + drag 0.5 * 10, gravity off, constraints 112 * 10000.
        assert_eq!(ts_phc_bodyProps(), 4.0 + 5.0 + 1_120_000.0);
        assert_eq!(ts_phc_noComponent(), 0.0);
    });
}

// M9-TS-1102
//
// ForceMode.Impulse on a 2 kg body gives velocity 2.
#[test]
fn an_impulse_changes_velocity_by_mass() {
    in_every_gc_mode(|| unsafe {
        let v = ts_phc_impulseMode();
        assert!((v - 2.0).abs() < 1e-4, "{v}");
    });
}

// M9-TS-1103
//
// A FixedJoint to a kinematic body keeps a body where it hangs, and a Rope hangs from its object.
#[test]
fn joints_and_ropes_hold() {
    in_every_gc_mode(|| unsafe {
        let y = ts_phc_fixedJointHolds(180.0);
        assert!((y - 3.5).abs() < 0.05, "{y}");
        platypus_physics_api::reset();
        let r = ts_phc_ropeHangs();
        assert_eq!((r / 1000.0).floor(), 5.0, "five points: {r}");
        assert!((r - 5000.0 - 3.0).abs() < 0.3, "{r}");
    });
}

// M9-TS-1104
//
// A CharacterController lands, reports it is grounded and moves its object.
#[test]
fn a_character_controller_lands() {
    in_every_gc_mode(|| unsafe {
        let r = ts_phc_characterLands();
        assert!((r - 101.5).abs() < 0.1, "{r}");
    });
}

// M9-TS-1105
//
// 2D bodies fall in the XY plane and keep their z; a 2D ray names the GameObject it hit.
#[test]
fn two_d_components_work() {
    in_every_gc_mode(|| unsafe {
        let r = ts_phc_twoDFall(240.0);
        // y 1.0 (floor top 0.5 plus the radius), z 0.7: packed as y * 100 + z.
        assert!((r - (100.0 + 0.7)).abs() < 6.0, "{r}");
        platypus_physics_api::reset();
        assert_eq!(text(ts_phc_ray2D()), "ground2d:4.5");
        platypus_physics_api::reset();
        assert_eq!(ts_phc_gravity2D(), -2.0);
    });
}
