use std::collections::HashMap;

use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};

pub static PROJECT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/blueprints");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceValue {
    pub name: String,
    pub slug: String,
    pub value: u64,
    pub temporary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Building {
    name: String,
    slug: String,
    ticks: u64,
    cost: HashMap<String, ResourceValue>,
    // attributes: HashMap<String, String>,
}

impl Blueprint for Building {
    fn name(&self) -> &str {
        &self.name
    }

    fn slug(&self) -> &str {
        &self.slug
    }

    fn ticks(&self) -> u64 {
        self.ticks
    }

    fn cost(&self) -> &HashMap<String, ResourceValue> {
        &self.cost
    }
}

pub trait Blueprint {
    fn name(&self) -> &str;
    fn slug(&self) -> &str;
    fn ticks(&self) -> u64;
    fn cost(&self) -> &HashMap<String, ResourceValue>;
}

pub struct BlueprintCollection {
    blueprints: HashMap<String, Box<dyn Blueprint>>,
}

impl BlueprintCollection {
    pub fn new() -> Self {
        BlueprintCollection {
            blueprints: HashMap::new(),
        }
    }

    pub fn load() -> Result<Self, anyhow::Error> {
        let mut collection = BlueprintCollection::new();
        for entry in PROJECT_DIR.entries() {
            if let Some(dir) = entry.as_dir() {
                let kind = match dir.path().file_name().unwrap_or_default().to_str() {
                    Some("") => {
                        println!("Skipping empty directory: {:?}", dir.path());
                        continue;
                    }
                    Some(kind) => kind,
                    None => {
                        println!("Skipping empty directory: {:?}", dir.path());
                        continue;
                    }
                };
                for file in dir.files() {
                    if file.path().extension().and_then(|s| s.to_str()) == Some("json") {
                        println!("Loading blueprint from file: {}", file.path().display());
                        let result = match kind {
                            "building" => {
                                let json = file.contents_utf8().unwrap_or_default();
                                let building: Building = deserialize_building(json)?;
                                Box::new(building) as Box<dyn Blueprint>
                            }
                            _ => {
                                println!("Unsupported blueprint kind: {}", kind);
                                continue;
                            }
                        };

                        println!("{}", result.slug());

                        collection
                            .blueprints
                            .insert(result.slug().to_string(), result);
                    }
                }
            } else {
                continue;
            }
        }

        println!("{:?}", collection.blueprints.keys());

        Ok(collection)
    }

    pub fn blueprints(&self) -> &HashMap<String, Box<dyn Blueprint>> {
        &self.blueprints
    }
}

impl Default for BlueprintCollection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn deserialize_building(json: &str) -> Result<Building, serde_json::Error> {
    serde_json::from_str(json)
}
