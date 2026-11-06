// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `@property` reflection tables (`bench/props.ts`): the generated `PROPS_<Class>` constants tell a
//! host which fields are properties, where they are and what they hold, and the host reads and
//! writes them with no script running. IDs are in TESTS.md (M9-TS-139..).
use platypus_ts_tests::prop::*;
use web_ui_rt::{link_anchor, ts_rt_gc_stress};

fn offset(table: &[(&str, usize, &str)], name: &str) -> usize {
    table.iter().find(|e| e.0 == name).expect("a property").1
}

// M9-TS-139
//
// Only the fields marked `@property` are listed, in declaration order, base class first; the
// type names say what a scene loader has to parse.
#[test]
fn the_table_lists_the_marked_fields_with_offsets_and_types() {
    let _serial = platypus_ts_tests::serial();
    assert_eq!(
        PROPS_Player,
        &[
            ("speed", 8, "number"),
            ("alive", 16, "boolean"),
            ("target", 24, "object:Player?"),
            ("label", 32, "number?"),
        ]
    );
    assert_eq!(
        PROPS_Hero,
        &[
            ("speed", 8, "number"),
            ("alive", 16, "boolean"),
            ("target", 24, "object:Player?"),
            ("label", 32, "number?"),
            ("power", 48, "number"),
        ]
    );
}

// M9-TS-140
//
// The host edits a component through the table (what a scene file load does), and the compiled
// code sees the new values; a subclass object works through its base class's table too.
#[test]
fn writing_a_property_through_the_table_changes_what_the_script_sees() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    ts_rt_gc_stress(true);
    unsafe {
        ts_prop_init();
        let p = ts_prop_makePlayer();
        assert_eq!(ts_prop_score(p), 513.0);
        let at = |o: &Player, off: usize| (o as *const Player as *mut u8).add(off);
        *(at(&*p, offset(PROPS_Player, "speed")) as *mut f64) = 7.0;
        assert_eq!(ts_prop_score(p), 713.0);
        *(at(&*p, offset(PROPS_Player, "alive")) as *mut u64) = 0;
        assert_eq!(ts_prop_score(p), 703.0);
        // A Hero through the Hero table, and through the Player table (same prefix).
        let h = ts_prop_makeHero();
        *(at(&*(h as *mut Player), offset(PROPS_Hero, "power")) as *mut f64) = 11.0;
        assert_eq!(ts_prop_heroPower(h), 11.0);
        *(at(&*(h as *mut Player), offset(PROPS_Player, "speed")) as *mut f64) = 2.0;
        assert_eq!(ts_prop_score(h as *mut Player), 213.0);
        assert_eq!(offset(PROPS_Hero, "speed"), offset(PROPS_Player, "speed"));
    }
    ts_rt_gc_stress(false);
}
