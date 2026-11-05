// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Enums, records, lists, rest parameters, option objects after a parameter, doc comments and `like`
//! in `#[ts_api]` (docs/platypus-api.md, "Enums, records and lists"). IDs are in TESTS.md (M9-API-16xx).
use ts_api::{Handle, Ty, ValueType, ts_api};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
}

impl ValueType<2> for P2 {
    fn from_parts(p: [f64; 2]) -> Self {
        P2 { x: p[0], y: p[1] }
    }
    fn to_parts(self) -> [f64; 2] {
        [self.x, self.y]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fill {
    #[default]
    Solid,
    PingPong,
    Dotted,
}

/// Where a thing is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub at: P2,
    pub weight: f64,
    pub hard: bool,
    pub fill: Fill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thing(pub u32);

impl Handle for Thing {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        Thing(raw as u32)
    }
}

/// Things that exist.
#[ts_api(
    namespace = "Rec", like,
    handles(Thing = Thing),
    values(Point = P2 [x: f64, y: f64]),
    enums(
        /// How to fill.
        FillMode = Fill [Solid, PingPong, Dotted]
    ),
    records(
        /// A spot.
        Spot = Spot [
            at: Point,
            /// How heavy.
            weight: f64,
            hard: bool,
            fill: FillMode
        ]
    )
)]
pub mod rec_api {
    use super::*;

    /// The words, one after the other with a separator between them.
    pub fn join_words(words: &[String], sep: &str) -> String {
        words.join(sep)
    }

    /// How many words there are (the function owns the list).
    pub fn count_words(words: Vec<String>) -> u32 {
        words.into_iter().filter(|w| !w.is_empty()).count() as u32
    }

    /// Words that start with a prefix, short ones first.
    pub fn words(prefix: &str) -> Vec<String> {
        ["a", "bb", "ccc"]
            .iter()
            .map(|w| format!("{prefix}{w}"))
            .collect()
    }

    /// Two spots per point.
    pub fn spots(points: &[P2]) -> Vec<Spot> {
        points
            .iter()
            .flat_map(|p| {
                [
                    Spot {
                        at: *p,
                        weight: 1.0,
                        hard: true,
                        fill: Fill::PingPong,
                    },
                    Spot {
                        at: *p,
                        weight: 2.5,
                        hard: false,
                        fill: Fill::Dotted,
                    },
                ]
            })
            .collect()
    }

    pub fn one(w: f64) -> Spot {
        Spot {
            at: P2 { x: w, y: -w },
            weight: w,
            hard: w > 0.0,
            fill: Fill::Solid,
        }
    }

    /// The points as given.
    #[rest(points)]
    pub fn many(points: &[P2]) -> Vec<P2> {
        points.to_vec()
    }

    pub fn next(f: Fill) -> Fill {
        match f {
            Fill::Solid => Fill::PingPong,
            Fill::PingPong => Fill::Dotted,
            Fill::Dotted => Fill::Solid,
        }
    }

    impl Thing {
        /// `new Thing(points?, { weight?, fill? })`.
        #[options(after = points)]
        #[optional(points, weight = 1.5, fill)]
        pub fn create(points: &[P2], weight: f64, fill: Fill) -> Thing {
            Thing(points.len() as u32 * 100 + (weight * 10.0) as u32 + fill as u32)
        }

        /// What `create` made of its arguments.
        #[property]
        pub fn code(self) -> f64 {
            f64::from(self.0)
        }
    }
}

// M9-API-1610
#[test]
fn the_manifest_lists_enums_records_lists_rest_and_options_after() {
    let m = rec_api::MANIFEST;
    assert_eq!(m.doc, "Things that exist.");
    assert!(m.like);
    assert_eq!(m.enums.len(), 1);
    assert_eq!(m.enums[0].name, "FillMode");
    assert_eq!(m.enums[0].doc, "How to fill.");
    assert_eq!(m.enums[0].values, ["solid", "pingPong", "dotted"]);
    assert_eq!(m.records.len(), 1);
    assert_eq!(m.records[0].doc, "A spot.");
    let names: Vec<_> = m.records[0].fields.iter().map(|f| f.name).collect();
    assert_eq!(names, ["at", "weight", "hard", "fill"]);
    assert_eq!(m.records[0].fields[0].ty, Ty::Value("Point"));
    assert_eq!(m.records[0].fields[1].doc, "How heavy.");
    assert_eq!(m.records[0].fields[3].ty, Ty::Enum("FillMode"));
    let f = |n: &str| m.functions.iter().find(|f| f.name == n).unwrap();
    assert_eq!(f("spots").ret, Ty::List("record:Spot", "Spot[]"));
    assert_eq!(f("spots").doc, "Two spots per point.");
    assert_eq!(f("one").ret, Ty::Record("Spot"));
    assert_eq!(f("many").ret, Ty::List("value:Point", "Point[]"));
    assert!(f("many").params[0].rest);
    assert_eq!(f("next").ret, Ty::Enum("FillMode"));
    let create = m.classes[0]
        .functions
        .iter()
        .find(|f| f.name == "create")
        .unwrap();
    assert_eq!(create.options_after, Some(0));
    assert_eq!(m.classes[0].doc, "");

    let json: serde_json::Value = serde_json::from_str(&m.to_json()).unwrap();
    assert_eq!(json["like"], true);
    assert_eq!(json["enums"][0]["values"][1], "pingPong");
    assert_eq!(json["enums"][0]["nameSymbol"], "platypus_FillMode_name");
    assert_eq!(json["enums"][0]["parseSymbol"], "platypus_FillMode_parse");
    assert_eq!(json["records"][0]["fields"][3]["type"], "enum:FillMode");
    assert_eq!(json["records"][0]["fields"][0]["type"], "value:Point");
    let fns = json["functions"].as_array().unwrap();
    let spots = fns.iter().find(|f| f["name"] == "spots").unwrap();
    assert_eq!(spots["returns"], "list:record:Spot");
    assert_eq!(spots["doc"], "Two spots per point.");
    let many = fns.iter().find(|f| f["name"] == "many").unwrap();
    assert_eq!(many["returns"], "list:value:Point");
    assert_eq!(many["params"][0]["rest"], true);
    assert_eq!(json["classes"][0]["functions"][0]["optionsAfter"], 0);
}

// M9-API-1611
#[test]
fn enum_helpers_convert_names_and_indexes_both_ways() {
    use rec_api::*;
    assert_eq!(fill_mode_name(Fill::PingPong), "pingPong");
    assert_eq!(fill_mode_parse("dotted"), Some(Fill::Dotted));
    assert_eq!(fill_mode_parse("Dotted"), None);
    assert_eq!(fill_mode_index(Fill::Dotted), 2);
    assert_eq!(fill_mode_from_index(1), Some(Fill::PingPong));
    assert_eq!(fill_mode_from_index(-1), None);
    assert_eq!(FILL_MODE_NAMES, ["solid", "pingPong", "dotted"]);
}

// M9-API-1612
#[test]
fn list_and_record_results_come_back_flat_through_the_result_buffer() {
    let r = |i: u32| ts_api::ts_api_result(i);
    // Two points in, four spots out: at.x, at.y, weight, hard, fill (an index), per spot.
    let points = [P2 { x: 1.0, y: 2.0 }, P2 { x: 3.0, y: 4.0 }];
    #[repr(C)]
    struct Arr {
        _h: u64,
        len: i64,
        _cap: i64,
        data: *mut u8,
    }
    // Script objects of the value class: a header word and the fields.
    let objs: Vec<[f64; 3]> = points.iter().map(|p| [0.0, p.x, p.y]).collect();
    let ptrs: Vec<*const f64> = objs.iter().map(|o| o.as_ptr()).collect();
    let arr = Arr {
        _h: 0,
        len: 2,
        _cap: 2,
        data: ptrs.as_ptr() as *mut u8,
    };
    let n = unsafe { rec_api::platypus_Rec_spots(&arr as *const Arr as *const u8) };
    assert_eq!(n, 4);
    let first: Vec<f64> = (0..5).map(r).collect();
    assert_eq!(first, [1.0, 2.0, 1.0, 1.0, 1.0]);
    let second: Vec<f64> = (5..10).map(r).collect();
    assert_eq!(second, [1.0, 2.0, 2.5, 0.0, 2.0]);
    assert_eq!((r(15), r(16)), (3.0, 4.0), "the last element's at");
    assert_eq!(r(1000), 0.0, "past the end is 0");
    // One record, no count.
    rec_api::platypus_Rec_one(2.0);
    let one: Vec<f64> = (0..5).map(r).collect();
    assert_eq!(one, [2.0, -2.0, 2.0, 1.0, 0.0]);
    // Enums travel as indexes and unknown ones are the first value.
    assert_eq!(rec_api::platypus_Rec_next(1), 2);
    assert_eq!(rec_api::platypus_Rec_next(-1), 1);
    assert_eq!(rec_api::platypus_Rec_next(99), 1);
    // Their names, for compiled code.
    let mut len = 0i64;
    let p = unsafe { rec_api::platypus_FillMode_name(1, &mut len) };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(p, len as usize) },
        b"pingPong"
    );
    let p = unsafe { rec_api::platypus_FillMode_name(7, &mut len) };
    assert_eq!((p.is_null(), len), (false, 0));
    rec_api::MANIFEST.to_json();
    assert!(ts_api::symbol("platypus_FillMode_name").is_some());
    assert!(ts_api::symbol("platypus_FillMode_parse").is_some());
    assert!(ts_api::symbol("ts_api_result").is_some());
}

// M9-API-1614
#[test]
fn creating_with_an_options_object_reads_the_parameters_after_the_named_one() {
    let at = |p: &[P2], w: f64, f: i32| unsafe {
        let objs: Vec<[f64; 3]> = p.iter().map(|p| [0.0, p.x, p.y]).collect();
        let ptrs: Vec<*const f64> = objs.iter().map(|o| o.as_ptr()).collect();
        #[repr(C)]
        struct Arr {
            _h: u64,
            len: i64,
            _cap: i64,
            data: *mut u8,
        }
        let arr = Arr {
            _h: 0,
            len: p.len() as i64,
            _cap: p.len() as i64,
            data: ptrs.as_ptr() as *mut u8,
        };
        rec_api::platypus_Thing_create(&arr as *const Arr as *const u8, w, f)
    };
    let two = [P2 { x: 0.0, y: 0.0 }, P2 { x: 1.0, y: 1.0 }];
    // 2 points, weight 3, `dotted` (index 2): 200 + 30 + 2.
    assert_eq!(at(&two, 3.0, 2), 232);
    assert_eq!(at(&two[..1], 1.5, 0), 115);
}

// M9-API-1620
#[test]
fn a_vec_of_strings_comes_back_through_the_string_list() {
    // The ABI: the count (a null prefix is an empty one), then the strings one by one.
    let n = unsafe { rec_api::platypus_Rec_words(std::ptr::null()) };
    assert_eq!(n, 3);
    let mut len = 0i64;
    let mut got = Vec::new();
    for i in 0..4 {
        let p = unsafe { ts_api::ts_api_result_str(i, &mut len) };
        got.push(
            String::from_utf8(unsafe { std::slice::from_raw_parts(p, len as usize) }.to_vec())
                .unwrap(),
        );
    }
    assert_eq!(
        got,
        ["a", "bb", "ccc", ""],
        "an index past the end is empty"
    );
    // The manifest says `list:string`.
    let json = rec_api::MANIFEST.to_json();
    assert!(json.contains(r#""returns":"list:string""#), "{json}");
    assert!(ts_api::symbol("ts_api_result_str").is_some());
}

/// A script string object as the compiled code makes them: a 48 byte header, then the bytes.
fn script_string(text: &str) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&0xA11A_0003u64.to_le_bytes());
    v.extend_from_slice(&(text.len() as i64).to_le_bytes());
    v.extend_from_slice(&0i64.to_le_bytes());
    v.extend_from_slice(&2u64.to_le_bytes()); // well formed
    v.extend_from_slice(&[0u8; 16]);
    v.extend_from_slice(text.as_bytes());
    v
}

// M9-API-1621
#[test]
fn string_arrays_come_in_as_script_arrays_of_strings() {
    // The ABI: a script array is a header, a length, a capacity and a pointer to string pointers.
    let strings: Vec<Vec<u8>> = ["héllo", "", "wörld"]
        .iter()
        .map(|w| script_string(w))
        .collect();
    let slots: Vec<*const u8> = strings.iter().map(|s| s.as_ptr()).collect();
    let mut array = Vec::new();
    array.extend_from_slice(&0xA11A_0001u64.to_le_bytes());
    array.extend_from_slice(&(slots.len() as i64).to_le_bytes());
    array.extend_from_slice(&(slots.len() as i64).to_le_bytes());
    array.extend_from_slice(&(slots.as_ptr() as usize as u64).to_le_bytes());
    let sep = script_string(", ");
    let mut len = 0i64;
    let p = unsafe { rec_api::platypus_Rec_joinWords(array.as_ptr(), sep.as_ptr(), &mut len) };
    let out = unsafe { std::slice::from_raw_parts(p, len as usize) };
    assert_eq!(std::str::from_utf8(out).unwrap(), "héllo, , wörld");
    assert_eq!(
        unsafe { rec_api::platypus_Rec_countWords(array.as_ptr()) },
        2
    );
    // A null array is an empty list.
    assert_eq!(
        unsafe { rec_api::platypus_Rec_countWords(std::ptr::null()) },
        0
    );
    let p = unsafe { rec_api::platypus_Rec_joinWords(std::ptr::null(), sep.as_ptr(), &mut len) };
    assert_eq!(unsafe { std::slice::from_raw_parts(p, len as usize) }, b"");
    // The manifest says `strings`; the declarations say `string[]`.
    let json = rec_api::MANIFEST.to_json();
    assert!(json.contains(r#""type":"strings""#), "{json}");
    let dts = rec_api::MANIFEST.to_dts();
    assert!(
        dts.contains("function joinWords(words: string[], sep: string): string;"),
        "{dts}"
    );
    assert!(
        dts.contains("function countWords(words: string[]): number;"),
        "{dts}"
    );
}
