pub mod generation;
pub mod tile;
pub mod topology;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::generation::GenerationParams;
pub use tile::{Season, Tile, TopologyType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct World {
    pub id: Uuid,
    pub name: String,
    pub created_at: String,
    pub tick_count: u64,
    pub season: Season,
    pub season_length: u32,
    pub tile_count: u32,
    pub topology_type: TopologyType,
    pub generation_params: GenerationParams,
    pub snapshot_path: Option<String>,
    pub tiles: Vec<Tile>,
}
