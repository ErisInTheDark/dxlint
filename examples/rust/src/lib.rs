use std::collections::{HashSet, BTreeMap, HashMap};

pub struct Config {
    pub name :String,
    limits: BTreeMap<String, Limit>,
}

pub fn load(path: &str) -> Config {
    let text = std::fs::read_to_string(path).expect("Config file must exist");
    let _: HashSet<u8> = HashSet::new();
    let _ = "use std::{b, a}; .expect(\"Not me\")";
    Config { name: text, limits: BTreeMap::new() }
}

pub struct Limit {
    max:Option<u32>,
}

type Unused = HashMap<String, String>;
