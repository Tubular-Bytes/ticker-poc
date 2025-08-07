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
    pub slug: String, // unique identifier for the building
    pub name: String,
    pub version: String,
    pub ticks: u64,
    pub cost: HashMap<String, ResourceValue>,
    #[serde(default)]
    pub attributes: HashMap<String, AttributeType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttributeType {
    String(Attribute<String>),
    Number(Attribute<u64>),
    Boolean(Attribute<bool>),
    Float(Attribute<f64>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute<T> {
    pub name: String,
    pub value: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Blueprint {
    Building(Building),
}

impl Blueprint {
    pub fn name(&self) -> String {
        match self {
            Blueprint::Building(b) => b.name.clone(),
        }
    }

    pub fn slug(&self) -> String {
        match self {
            Blueprint::Building(b) => b.slug.clone(),
        }
    }

    pub fn version(&self) -> String {
        match self {
            Blueprint::Building(b) => b.version.clone(),
        }
    }

    pub fn ticks(&self) -> u64 {
        match self {
            Blueprint::Building(b) => b.ticks,
        }
    }

    pub fn cost(&self) -> &HashMap<String, ResourceValue> {
        match self {
            Blueprint::Building(b) => &b.cost,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    blueprints: HashMap<String, Blueprint>,
}

impl Collection {
    pub fn new() -> Self {
        Collection {
            blueprints: HashMap::new(),
        }
    }

    pub fn load() -> Result<Self, anyhow::Error> {
        let mut collection = Collection::new();
        for entry in PROJECT_DIR.entries() {
            if let Some(dir) = entry.as_dir() {
                let kind = match dir.path().file_name().unwrap_or_default().to_str() {
                    Some("") => {
                        tracing::warn!(
                            path = format!("{:?}", dir.path()),
                            "skipping empty directory"
                        );
                        continue;
                    }
                    Some(kind) => kind,
                    None => {
                        tracing::warn!(
                            path = format!("{:?}", dir.path()),
                            "skipping empty directory"
                        );
                        continue;
                    }
                };
                for file in dir.files() {
                    if file.path().extension().and_then(|s| s.to_str()) == Some("json") {
                        let display_name = file.path().display();
                        tracing::debug!(file = format!("{display_name}"), "loading blueprint");
                        let result = match kind {
                            "building" => {
                                let json = file.contents_utf8().unwrap_or_default();
                                let building: Building = serde_json::from_str(json)?;
                                building
                            }
                            _ => {
                                tracing::error!(kind, "unsupported blueprint kind");
                                continue;
                            }
                        };

                        collection
                            .blueprints
                            .insert(result.slug.to_string(), Blueprint::Building(result));
                    }
                }
            } else {
                continue;
            }
        }

        Ok(collection)
    }

    pub fn get_blueprint(&self, slug: &str) -> Option<Blueprint> {
        self.blueprints.get(slug).cloned()
    }

    pub fn keys(&self) -> Vec<&String> {
        self.blueprints.keys().collect()
    }
}

impl Default for Collection {
    fn default() -> Self {
        Self::new()
    }
}
