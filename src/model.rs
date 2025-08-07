use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceValue {
    pub name: String,
    pub slug: String,
    pub value: u64,
    pub temporary: bool,
}

#[derive(Debug, Clone)]
pub struct Blueprint {
    pub name: String,
    pub slug: String,
    pub version: String,
    pub ticks: u64,
    pub cost: HashMap<String, ResourceValue>,
}

impl Blueprint {
    pub fn new(name: String, slug: String, ticks: u64, cost: HashMap<String, ResourceValue>) -> Self {
        Blueprint {
            name,
            slug,
            version: crate::ASSET_VERSION.into(),
            ticks,
            cost,
        }
    }
}

impl From<Box<dyn crate::library::Blueprint>> for Blueprint {
    fn from(blueprint: Box<dyn crate::library::Blueprint>) -> Self {
        Blueprint {
            name: blueprint.name().to_string(),
            slug: blueprint.slug().to_string(),
            version: blueprint.version().to_string(),
            ticks: blueprint.ticks(),
            cost: blueprint.cost().clone(),
        }
    }   
}
