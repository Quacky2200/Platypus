// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Records with lists of numbers and of records, a function called with one object of all its
//! parameters (`#[options(.., object)]`), and a class function declared twice. IDs are in TESTS.md
//! (M9-API-1640..).
use ts_api::{Handle, ts_api};

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: f64,
    pub tags: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub name: String,
    pub rows: Vec<Row>,
    pub spare: Option<Vec<Row>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell(pub u32);

impl Handle for Cell {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        Cell(raw as u32)
    }
}

thread_local! {
    static SUM: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    static LEVEL: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

#[ts_api(
    namespace = "Lists", handles(Cell = Cell),
    records(
        /// A row of tags.
        Row = Row [id: f64, tags: Vec<f64>],
        /// Rows by name.
        Table = Table [name: string, rows: Vec<Row>, spare: Option<Vec<Row>>]
    )
)]
pub mod lists_api {
    use super::*;

    /// The ids and tags of every row, spare ones included.
    pub fn total(t: Option<Table>) -> f64 {
        let Some(t) = t else { return -1.0 };
        t.rows
            .iter()
            .chain(t.spare.iter().flatten())
            .map(|r| r.id + r.tags.iter().sum::<f64>())
            .sum()
    }

    /// The table back, so the JSON of nested lists can be read.
    pub fn echo(t: Option<Table>) -> Option<Table> {
        t
    }

    /// Adds up its parts.
    /// a: The first.
    /// b: The second.
    #[global]
    #[options(name = "SumOptions", only, object)]
    #[optional(b)]
    pub fn sum(a: f64, b: f64) -> f64 {
        a + if b.is_nan() { 100.0 } else { b }
    }

    impl Cell {
        pub fn create() -> Cell {
            Cell(1)
        }

        /// Adds a number.
        #[js_name = "add"]
        pub fn add_number(self, n: f64) {
            SUM.with(|s| s.set(s.get() + n));
        }

        /// Adds a table.
        #[js_name = "add"]
        pub fn add_table(self, t: Option<Table>) {
            let n = total(t);
            SUM.with(|s| s.set(s.get() + n * 10.0));
        }

        #[property]
        pub fn sum(self) -> f64 {
            SUM.with(std::cell::Cell::get)
        }

        #[property]
        pub fn level(self) -> f64 {
            LEVEL.with(std::cell::Cell::get)
        }

        pub fn set_level(self, v: f64) {
            LEVEL.with(|l| l.set(v));
        }

        /// `cell.level = [a, b]`: another version of the setter.
        #[setter(level)]
        pub fn set_level_list(self, v: &[f64]) {
            LEVEL.with(|l| l.set(v.iter().sum()));
        }

        /// `new Cell(a, b)`.
        #[constructor]
        #[optional(b)]
        pub fn construct(a: f64, b: f64) -> Cell {
            LEVEL.with(|l| l.set(a + if b.is_nan() { 0.0 } else { b }));
            Cell(2)
        }
    }
}

// M9-API-1640
#[test]
fn records_hold_lists_of_numbers_and_of_records() {
    let json: serde_json::Value = serde_json::from_str(&lists_api::MANIFEST.to_json()).unwrap();
    let table = json["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Table")
        .unwrap();
    let types: Vec<String> = table["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["type"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(types, ["string", "records:Row", "opt:records:Row"]);
    let row = json["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Row")
        .unwrap();
    assert_eq!(row["fields"][1]["type"], "list:f64");
}

// M9-API-1641
#[test]
fn a_function_called_with_one_object_declares_it() {
    let json: serde_json::Value = serde_json::from_str(&lists_api::MANIFEST.to_json()).unwrap();
    let sum = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "sum" && f["kind"] == "static")
        .unwrap();
    assert_eq!(sum["optionsObject"], true);
    assert_eq!(sum["optionsName"], "SumOptions");
    // `#[global]`: also a global function of the same name.
    assert_eq!(sum["global"], true);
}

// M9-API-1642
#[test]
fn a_class_function_declared_twice_is_in_the_manifest_twice() {
    let json: serde_json::Value = serde_json::from_str(&lists_api::MANIFEST.to_json()).unwrap();
    let cell = json["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Cell")
        .unwrap();
    let adds: Vec<&str> = cell["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["name"] == "add")
        .map(|f| f["params"][1]["type"].as_str().unwrap())
        .collect();
    assert_eq!(adds, ["f64", "opt:record:Table"]);
}

// M9-API-1643
#[test]
fn a_setter_can_have_a_second_version_and_a_class_a_positional_constructor() {
    let json: serde_json::Value = serde_json::from_str(&lists_api::MANIFEST.to_json()).unwrap();
    let cell = json["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Cell")
        .unwrap();
    let fns = cell["functions"].as_array().unwrap();
    let setters: Vec<&str> = fns
        .iter()
        .filter(|f| f["kind"] == "set" && f["name"] == "level")
        .map(|f| f["params"][1]["type"].as_str().unwrap())
        .collect();
    assert_eq!(setters, ["f64", "f64s"]);
    let ctor = fns.iter().find(|f| f["constructor"] == true).unwrap();
    assert_eq!(ctor["name"], "construct");
}
