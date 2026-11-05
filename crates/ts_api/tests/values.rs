// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Value classes in `#[ts_api]`: a few numbers that scripts see as an object, passed across the
//! C ABI as one scalar per field (a result through a trailing pointer). IDs are in TESTS.md
//! (M9-API-*).
use ts_api::{Ty, ValueType, ts_api};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl ValueType<3> for Vec3 {
    fn from_parts(p: [f64; 3]) -> Self {
        Vec3 {
            x: p[0] as f32,
            y: p[1] as f32,
            z: p[2] as f32,
        }
    }
    fn to_parts(self) -> [f64; 3] {
        [self.x as f64, self.y as f64, self.z as f64]
    }
}

#[ts_api(namespace = "Vec", values(Vector3 = Vec3 [x: f32, y: f32, z: f32]))]
pub mod vec_api {
    use super::Vec3;

    /// The length of a vector.
    pub fn length(v: Vec3) -> f32 {
        (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
    }

    /// `v * k`.
    pub fn scaled(v: Vec3, k: f32) -> Vec3 {
        Vec3 {
            x: v.x * k,
            y: v.y * k,
            z: v.z * k,
        }
    }

    pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
        Vec3 {
            x: a.y * b.z - a.z * b.y,
            y: a.z * b.x - a.x * b.z,
            z: a.x * b.y - a.y * b.x,
        }
    }
}

// M9-API-10
#[test]
fn the_manifest_and_declarations_describe_the_value_class() {
    let m = vec_api::MANIFEST;
    assert_eq!(m.values.len(), 1);
    assert_eq!(m.values[0].name, "Vector3");
    assert_eq!(m.values[0].fields.len(), 3);
    assert_eq!(m.values[0].fields[2].name, "z");
    assert_eq!(m.values[0].fields[2].ty, Ty::F32);
    assert_eq!(m.functions[1].params[0].ty, Ty::Value("Vector3"));
    assert_eq!(m.functions[1].ret, Ty::Value("Vector3"));
    let d = m.to_dts();
    assert!(d.contains("declare class Vector3 {"), "{d}");
    assert!(
        d.contains("constructor(x: number, y: number, z: number);"),
        "{d}"
    );
    assert!(
        d.contains("function scaled(v: Vector3, k: number): Vector3;"),
        "{d}"
    );
    let j = m.to_json();
    assert!(
        j.contains(
            "\"values\":[{\"name\":\"Vector3\",\"fields\":[{\"name\":\"x\",\"type\":\"f32\"}"
        ),
        "{j}"
    );
    assert!(j.contains("\"type\":\"value:Vector3\""), "{j}");
    assert!(j.contains("\"returns\":\"value:Vector3\""), "{j}");
}

// M9-API-11
//
// A value parameter is passed as its fields (`v_0, v_1, v_2`); a value result is written through a
// trailing pointer.
#[test]
fn the_shims_pass_a_value_as_scalars_and_return_one_through_a_pointer() {
    assert_eq!(vec_api::platypus_Vec_length(3.0, 0.0, 4.0), 5.0);
    let mut out = [0.0f32; 3];
    unsafe { vec_api::platypus_Vec_scaled(1.0, 2.0, 3.0, 2.0, out.as_mut_ptr()) };
    assert_eq!(out, [2.0, 4.0, 6.0]);
    unsafe { vec_api::platypus_Vec_cross(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, out.as_mut_ptr()) };
    assert_eq!(out, [0.0, 0.0, 1.0]);
}

#[ts_api(namespace = "VecMath", values(Vector3 = Vec3 [x: f32, y: f32, z: f32]))]
pub mod vec_methods {
    use super::Vec3;

    /// Its length.
    #[getter(Vector3)]
    pub fn magnitude(v: Vec3) -> f32 {
        (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
    }

    /// `v * k`.
    #[method(Vector3)]
    pub fn scale(v: Vec3, k: f32) -> Vec3 {
        Vec3 {
            x: v.x * k,
            y: v.y * k,
            z: v.z * k,
        }
    }

    pub fn origin() -> Vec3 {
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

// M9-API-20
#[test]
fn value_class_methods_and_getters_are_declared_inside_the_value_class() {
    let m = vec_methods::MANIFEST;
    assert_eq!(
        m.functions.len(),
        1,
        "only `origin` is a namespace function"
    );
    assert_eq!(m.classes.len(), 1);
    assert_eq!(m.classes[0].name, "Vector3");
    let scale = m.classes[0]
        .functions
        .iter()
        .find(|f| f.name == "scale")
        .unwrap();
    assert_eq!(scale.symbol, "platypus_Vector3_scale");
    assert_eq!(scale.params[0].name, "this");
    assert_eq!(scale.params[0].ty, Ty::Value("Vector3"));
    let d = m.to_dts();
    assert_eq!(d.matches("declare class Vector3 {").count(), 1, "{d}");
    assert!(d.contains("scale(k: number): Vector3;"), "{d}");
    assert!(d.contains("get magnitude(): number;"), "{d}");
    assert!(!d.contains("declare type Vector3"), "{d}");
    // The shims take the value as its fields first.
    assert_eq!(vec_methods::platypus_Vector3_magnitude(3.0, 4.0, 0.0), 5.0);
    let mut out = [0.0f32; 3];
    unsafe { vec_methods::platypus_Vector3_scale(1.0, 2.0, 3.0, 2.0, out.as_mut_ptr()) };
    assert_eq!(out, [2.0, 4.0, 6.0]);
}
