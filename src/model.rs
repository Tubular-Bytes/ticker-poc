use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Blueprint {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub ticks: u64,
}

impl Blueprint {
    pub fn new(name: String, ticks: u64) -> Self {
        let id = Uuid::new_v4();
        Blueprint {
            id,
            name,
            version: crate::ASSET_VERSION.into(),
            ticks,
        }
    }
}
