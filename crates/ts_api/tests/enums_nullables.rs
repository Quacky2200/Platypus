// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Enums shared between namespaces, optional enum parameters, explicit names, enum lists, nullable
//! numbers / strings / values, `#[nullable(undefined)]` and aliases in `#[ts_api]`
//! (docs/platypus-api.md, "Enums and nullables"). IDs are in TESTS.md (M9-API-162x).
use ts_api::{Handle, Ty, ValueType, ts_api};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Soft,
    Hard,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
}

impl ValueType<2> for Rgb {
    fn from_parts(p: [f64; 2]) -> Self {
        Rgb { r: p[0], g: p[1] }
    }
    fn to_parts(self) -> [f64; 2] {
        [self.r, self.g]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item(pub u32);

impl Handle for Item {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        Item(raw as u32)
    }
}

thread_local! {
    static LEVEL: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
    static TINT: std::cell::Cell<Option<Rgb>> = const { std::cell::Cell::new(None) };
    static MODE: std::cell::Cell<Mode> = const { std::cell::Cell::new(Mode::Soft) };
    static LOOK: std::cell::RefCell<Option<LookPatch>> = const { std::cell::RefCell::new(None) };
}

/// The declaring namespace: `Mode` and its converters.
#[ts_api(
    namespace = "Kinds", enums(
        /// How hard.
        Hardness = Mode [Soft, Hard]
    )
)]
pub mod kinds_api {
    use super::*;

    #[property]
    pub fn default_mode() -> Mode {
        Mode::Hard
    }
}

/// Uses `Hardness` without declaring it again.
#[ts_api(
    namespace = "Things", handles(Item = Item),
    values(Rgb = Rgb [r: f64, g: f64]),
    enums(
        extern Hardness = Mode [Soft, Hard],
        /// Names that are not the variant names.
        Shout = Loud [Quiet = "QUIET", Boomy = "Boomy"]
    ),
    records(
        /// A name and a number.
        Label = Tag [text: string, size: f64],
        /// An object argument: optional keys of several kinds.
        Look = LookPatch [
            /// The width.
            width: f64,
            tint: Option<Rgb>,
            sun: Option<Option<RgbLike>>,
            mode: Option<Hardness>,
            curve: Option<pairs>,
            name: Option<string> as "label"
        ]
    ),
    aliases(Stuff = Item, HardnessAlias = Hardness)
)]
pub mod things_api {
    use super::kinds_api::{hardness_from_index, hardness_index, hardness_name, hardness_parse};
    use super::*;

    /// An unknown name leaves the mode as it was.
    pub fn set_mode(m: Option<Mode>) {
        if let Some(m) = m {
            MODE.with(|c| c.set(m));
        }
    }

    #[property]
    pub fn mode() -> Mode {
        MODE.with(|c| c.get())
    }

    /// `null` for none.
    #[property]
    pub fn level() -> Option<f64> {
        LEVEL.with(|c| c.get())
    }

    pub fn set_level(v: Option<f64>) {
        LEVEL.with(|c| c.set(v));
    }

    #[property]
    pub fn tint() -> Option<Rgb> {
        TINT.with(|c| c.get())
    }

    pub fn set_tint(v: Option<Rgb>) {
        TINT.with(|c| c.set(v));
    }

    /// The text of a mode, or none for the soft one.
    pub fn note(m: Mode) -> Option<String> {
        (m == Mode::Hard).then(|| String::from("hard"))
    }

    pub fn modes() -> Vec<Mode> {
        vec![Mode::Hard, Mode::Soft]
    }

    /// Pokes an item.
    /// level: How hard to poke.
    /// loud: Whether it makes a sound.
    #[options(name = "PokeOptions", only)]
    #[optional(level = 1, loud, mode)]
    pub fn poke(item: Item, level: f64, loud: bool, mode: Option<Mode>) -> f64 {
        f64::from(item.0) * level
            + if loud { 0.5 } else { 0.0 }
            + f64::from(u8::from(mode == Some(Mode::Hard)))
    }

    /// Sets the look (`null` for none).
    pub fn set_look(v: Option<LookPatch>) {
        LOOK.with(|c| *c.borrow_mut() = v);
    }

    #[property]
    pub fn look() -> Option<LookPatch> {
        LOOK.with(|c| c.borrow().clone())
    }

    /// A record with a string field.
    pub fn label(n: f64) -> Tag {
        Tag {
            text: format!("n{n}"),
            size: n,
        }
    }

    pub fn labels() -> Vec<Tag> {
        vec![
            Tag {
                text: "a".into(),
                size: 1.0,
            },
            Tag {
                text: "bb".into(),
                size: 2.0,
            },
        ]
    }

    pub fn loud(l: Loud) -> Loud {
        match l {
            Loud::Quiet => Loud::Boomy,
            Loud::Boomy => Loud::Quiet,
        }
    }

    /// The item, or none when the id is 0.
    #[nullable(undefined)]
    pub fn item(id: f64) -> Item {
        Item(id as u32)
    }

    impl Item {
        #[property]
        #[nullable(undefined)]
        pub fn next(self) -> Item {
            Item(self.0 + 1).max_or_none()
        }
    }
}

impl Item {
    fn max_or_none(self) -> Item {
        if self.0 > 3 { Item(0) } else { self }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LookPatch {
    pub width: f64,
    pub tint: Option<Rgb>,
    pub sun: Option<Option<Rgb>>,
    pub mode: Option<Mode>,
    pub curve: Option<Vec<(f64, f64)>>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub text: String,
    pub size: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loud {
    Quiet,
    Boomy,
}

// M9-API-1622
#[test]
fn an_extern_enum_adds_no_second_set_of_symbols_and_the_manifest_lists_it_once() {
    let kinds = kinds_api::MANIFEST;
    let things = things_api::MANIFEST;
    // Reading a manifest registers its symbols.
    kinds.to_json();
    assert_eq!(kinds.enums.len(), 1);
    assert_eq!(kinds.enums[0].name, "Hardness");
    assert_eq!(kinds.enums[0].doc, "How hard.");
    // Only `Shout` is declared by `Things`; `Hardness` is borrowed.
    let names: Vec<_> = things.enums.iter().map(|e| e.name).collect();
    assert_eq!(names, ["Shout"]);
    // An explicit name wins over the variant's.
    assert_eq!(things.enums[0].values, ["QUIET", "Boomy"]);
    // The functions still say `enum:Hardness`.
    let f = |n: &str| things.functions.iter().find(|f| f.name == n).unwrap();
    assert_eq!(f("note").params[0].ty, Ty::Enum("Hardness"));
    assert_eq!(f("modes").ret, Ty::List("enum:Hardness", "Hardness[]"));
    assert!(
        things
            .functions
            .iter()
            .any(|f| f.rust_name == "set_mode" && f.params[0].ty == Ty::Enum("Hardness"))
    );
    let json: serde_json::Value = serde_json::from_str(&things.to_json()).unwrap();
    assert_eq!(json["aliases"][0]["name"], "Stuff");
    assert_eq!(json["aliases"][1]["of"], "Hardness");
    let names: Vec<_> = json["enums"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["Shout"]);
    assert!(ts_api::symbol("platypus_Hardness_name").is_some());
    assert!(ts_api::symbol("platypus_Shout_parse").is_some());
}

// M9-API-1623
#[test]
fn nullable_numbers_values_and_strings_use_nan_and_the_none_flag_on_the_abi() {
    // Option<f64>: NaN is none, both ways.
    things_api::platypus_Things_setLevel(f64::NAN);
    assert!(LEVEL.with(|c| c.get()).is_none());
    things_api::platypus_Things_setLevel(2.5);
    assert_eq!(LEVEL.with(|c| c.get()), Some(2.5));
    assert_eq!(things_api::platypus_Things_level(), 2.5);
    things_api::platypus_Things_setLevel(f64::NAN);
    assert!(things_api::platypus_Things_level().is_nan());
    // Option<V>: NaN in the fields is none, both ways.
    things_api::platypus_Things_setTint(0.25, 0.5);
    assert_eq!(TINT.with(|c| c.get()), Some(Rgb { r: 0.25, g: 0.5 }));
    let mut out = [0.0f64; 2];
    unsafe { things_api::platypus_Things_tint(out.as_mut_ptr()) };
    assert_eq!(out, [0.25, 0.5]);
    things_api::platypus_Things_setTint(f64::NAN, f64::NAN);
    assert!(TINT.with(|c| c.get()).is_none());
    unsafe { things_api::platypus_Things_tint(out.as_mut_ptr()) };
    assert!(out[0].is_nan() && out[1].is_nan());
    // Option<String>: an empty text and the flag.
    let mut len = 0i64;
    let p = unsafe { things_api::platypus_Things_note(1, &mut len) };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(p, len as usize) },
        b"hard"
    );
    assert!(!ts_api::ts_api_result_none());
    let p = unsafe { things_api::platypus_Things_note(0, &mut len) };
    assert_eq!(
        (unsafe { std::slice::from_raw_parts(p, len as usize) }, len),
        (&b""[..], 0)
    );
    assert!(ts_api::ts_api_result_none());
    assert!(ts_api::symbol("ts_api_result_none").is_some());
    // The manifest and the declarations.
    let json = things_api::MANIFEST.to_json();
    assert!(json.contains(r#""returns":"opt:f64""#), "{json}");
    assert!(json.contains(r#""returns":"opt:value:Rgb""#), "{json}");
    assert!(json.contains(r#""returns":"opt:string""#), "{json}");
}

// M9-API-1624
#[test]
fn an_optional_enum_parameter_is_none_for_a_left_out_or_unknown_name_and_a_list_of_enums_is_indexes()
 {
    // The shim reads -1 (an unknown or left-out name) as none: nothing changes.
    things_api::platypus_Things_setMode(0);
    assert_eq!(MODE.with(|c| c.get()), Mode::Soft);
    things_api::platypus_Things_setMode(1);
    assert_eq!(MODE.with(|c| c.get()), Mode::Hard);
    things_api::platypus_Things_setMode(-1);
    assert_eq!(MODE.with(|c| c.get()), Mode::Hard, "-1 changes nothing");
    things_api::platypus_Things_setMode(99);
    assert_eq!(MODE.with(|c| c.get()), Mode::Hard);
    // A list of enums: the count, and the indexes in the result buffer.
    let n = things_api::platypus_Things_modes();
    assert_eq!(n, 2);
    assert_eq!(
        (ts_api::ts_api_result(0), ts_api::ts_api_result(1)),
        (1.0, 0.0)
    );
    // The converters of the declaring namespace and the explicit names.
    assert_eq!(kinds_api::hardness_name(Mode::Hard), "hard");
    assert_eq!(things_api::shout_name(Loud::Quiet), "QUIET");
    assert_eq!(things_api::shout_parse("Boomy"), Some(Loud::Boomy));
    assert_eq!(things_api::shout_parse("boomy"), None);
    assert_eq!(things_api::platypus_Things_loud(0), 1);
}

// M9-API-1626
#[test]
fn nullable_undefined_is_in_the_manifest_json() {
    let json: serde_json::Value = serde_json::from_str(&things_api::MANIFEST.to_json()).unwrap();
    let item = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "item")
        .unwrap();
    assert_eq!(item["nullable"], true);
    assert_eq!(item["undefined"], true);
    let next = json["classes"][0]["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "next")
        .unwrap();
    assert_eq!(next["undefined"], true);
}

// M9-API-1630
#[test]
fn a_record_with_a_string_field_keeps_the_text_in_the_string_list_in_field_order() {
    let r = |i: u32| ts_api::ts_api_result(i);
    let str_at = |i: u32| {
        let mut len = 0i64;
        let p = unsafe { ts_api::ts_api_result_str(i, &mut len) };
        String::from_utf8(unsafe { std::slice::from_raw_parts(p, len as usize) }.to_vec()).unwrap()
    };
    things_api::platypus_Things_label(7.0);
    assert_eq!((r(0), str_at(0)), (7.0, "n7".to_string()));
    // A list: the numbers of every element in a row, the strings in a row.
    assert_eq!(things_api::platypus_Things_labels(), 2);
    assert_eq!((r(0), r(1)), (1.0, 2.0));
    assert_eq!((str_at(0), str_at(1)), ("a".to_string(), "bb".to_string()));
    let json = things_api::MANIFEST.to_json();
    assert!(json.contains(r#"{"name":"text","type":"string""#), "{json}");
}

// M9-API-1631
#[test]
fn a_named_options_object_has_a_type_with_the_parameter_docs() {
    let json: serde_json::Value = serde_json::from_str(&things_api::MANIFEST.to_json()).unwrap();
    let poke = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "poke")
        .unwrap();
    assert_eq!(poke["optionsName"], "PokeOptions");
    assert_eq!(poke["optionsOnly"], true);
    // The `/// name: text` lines are the docs of the keys and leave the function's own doc.
    assert_eq!(poke["doc"], "Pokes an item.");
    assert_eq!(poke["params"][1]["doc"], "How hard to poke.");
    assert_eq!(poke["params"][2]["doc"], "Whether it makes a sound.");
    assert!(poke["params"][0].get("doc").is_none());
}

/// A script string object as the compiled code makes them.
fn script_string(text: &str) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&0xA11A_0003u64.to_le_bytes());
    v.extend_from_slice(&(text.len() as i64).to_le_bytes());
    v.extend_from_slice(&0i64.to_le_bytes());
    v.extend_from_slice(&2u64.to_le_bytes());
    v.extend_from_slice(&[0u8; 16]);
    v.extend_from_slice(text.as_bytes());
    v
}

// M9-API-1632
#[test]
fn an_optional_record_travels_as_json_text_both_ways_with_optional_keys_and_nulls() {
    let set = |text: &str| {
        let s = script_string(text);
        unsafe { things_api::platypus_Things_setLook(s.as_ptr()) }
    };
    let get = || {
        let mut len = 0i64;
        let p = unsafe { things_api::platypus_Things_look(&mut len) };
        String::from_utf8(unsafe { std::slice::from_raw_parts(p, len as usize) }.to_vec()).unwrap()
    };
    set(
        r#"{"width":2,"tint":[0.5,0.25],"sun":null,"mode":"hard","curve":[[1,2],{"0":3,"1":4}],"label":"x"}"#,
    );
    let l = LOOK.with(|c| c.borrow().clone()).unwrap();
    assert_eq!(l.width, 2.0);
    assert_eq!(l.tint, Some(Rgb { r: 0.5, g: 0.25 }));
    assert_eq!(l.sun, Some(None), "null is present and none");
    assert_eq!(l.mode, Some(Mode::Hard));
    assert_eq!(l.curve, Some(vec![(1.0, 2.0), (3.0, 4.0)]));
    assert_eq!(l.name.as_deref(), Some("x"));
    // Left-out keys are none (and a plain object of a value class does too).
    set(r#"{"width":1,"sun":{"r":1,"g":0}}"#);
    let l = LOOK.with(|c| c.borrow().clone()).unwrap();
    assert_eq!((l.tint, l.mode, l.curve, l.name), (None, None, None, None));
    assert_eq!(l.sun, Some(Some(Rgb { r: 1.0, g: 0.0 })));
    // A missing required key, a wrong type, `null` and "" are reported or none: the look is none.
    set(r#"{"tint":[1,2]}"#);
    assert!(LOOK.with(|c| c.borrow().is_none()));
    set(r#"{"width":1}"#);
    set("null");
    assert!(LOOK.with(|c| c.borrow().is_none()));
    set(r#"{"width":1,"mode":"fuzzy"}"#);
    assert!(
        LOOK.with(|c| c.borrow().is_none()),
        "an unknown enum name is an error"
    );
    // The result is the same record as JSON text; none is `null`.
    assert_eq!(get(), "null");
    set(r#"{"width":2,"sun":null,"curve":[[1,2]],"label":"x"}"#);
    let text = get();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["width"].as_f64(), Some(2.0));
    assert!(v["sun"].is_null() && v.get("tint").is_none() && v.get("mode").is_none());
    assert_eq!(v["curve"][0][0].as_f64(), Some(1.0));
    assert_eq!(v["label"], "x");
    // The manifest: a nullable record parameter / result, optional keys, `like` and the script key.
    let json: serde_json::Value = serde_json::from_str(&things_api::MANIFEST.to_json()).unwrap();
    let look = json["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Look")
        .unwrap();
    let types: Vec<(String, String)> = look["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap().to_string(),
                f["type"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        types,
        [
            ("width", "f64"),
            ("tint", "opt:value:Rgb"),
            ("sun", "opt:opt:like:Rgb"),
            ("mode", "opt:enum:Hardness"),
            ("curve", "opt:list:pair"),
            ("label", "opt:string"),
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()))
    );
    let f = json["functions"].as_array().unwrap();
    assert_eq!(
        f.iter()
            .find(|f| f["name"] == "look" && f["kind"] == "get")
            .unwrap()["returns"],
        "opt:record:Look"
    );
}
