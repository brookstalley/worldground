use serde::{Deserialize, Serialize};

// === Enums ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainType {
    Ocean,
    Coast,
    Plains,
    Hills,
    Mountains,
    Cliffs,
    Wetlands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SoilType {
    Sand,
    Clay,
    Loam,
    Rock,
    Silt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClimateZone {
    Polar,
    Subpolar,
    Temperate,
    Subtropical,
    Tropical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BiomeType {
    Ocean,
    Ice,
    Tundra,
    BorealForest,
    TemperateForest,
    Grassland,
    Savanna,
    Desert,
    TropicalForest,
    Wetland,
    Barren,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrecipitationType {
    None,
    Rain,
    Snow,
    Hail,
    Sleet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn next(self) -> Season {
        match self {
            Season::Spring => Season::Summer,
            Season::Summer => Season::Autumn,
            Season::Autumn => Season::Winter,
            Season::Winter => Season::Spring,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TopologyType {
    FlatHex,
    Geodesic,
}

// === Position ===

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

// === Layer Structs ===

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeologyLayer {
    pub terrain_type: TerrainType,
    pub elevation: f32,
    pub soil_type: SoilType,
    pub drainage: f32,
    pub tectonic_stress: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClimateLayer {
    pub zone: ClimateZone,
    pub base_temperature: f32,
    pub base_precipitation: f32,
    pub latitude: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiomeLayer {
    pub biome_type: BiomeType,
    pub vegetation_density: f32,
    pub vegetation_health: f32,
    pub transition_pressure: f32,
    pub ticks_in_current_biome: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceDeposit {
    pub resource_type: String,
    pub quantity: f32,
    pub max_quantity: f32,
    pub renewal_rate: f32,
    pub requires_biome: Option<Vec<BiomeType>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceLayer {
    pub resources: Vec<ResourceDeposit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherLayer {
    pub temperature: f32,
    pub precipitation: f32,
    pub precipitation_type: PrecipitationType,
    pub wind_speed: f32,
    pub wind_direction: f32,
    pub cloud_cover: f32,
    pub storm_intensity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionsLayer {
    pub soil_moisture: f32,
    pub snow_depth: f32,
    pub mud_level: f32,
    pub flood_level: f32,
    pub frost_days: u32,
    pub drought_days: u32,
    pub fire_risk: f32,
}

// === Tile ===

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    pub id: u32,
    pub neighbors: Vec<u32>,
    pub position: Position,
    pub geology: GeologyLayer,
    pub climate: ClimateLayer,
    pub biome: BiomeLayer,
    pub resources: ResourceLayer,
    pub weather: WeatherLayer,
    pub conditions: ConditionsLayer,
}

impl Tile {
    /// Create a tile with neutral default values for all layers.
    /// Used during topology generation; world generation overwrites all layer data.
    pub fn new_default(id: u32, neighbors: Vec<u32>, position: Position) -> Self {
        Self {
            id,
            neighbors,
            position,
            geology: GeologyLayer {
                terrain_type: TerrainType::Plains,
                elevation: 0.0,
                soil_type: SoilType::Loam,
                drainage: 0.5,
                tectonic_stress: 0.0,
            },
            climate: ClimateLayer {
                zone: ClimateZone::Temperate,
                base_temperature: 288.15,
                base_precipitation: 0.5,
                latitude: 0.0,
            },
            biome: BiomeLayer {
                biome_type: BiomeType::Grassland,
                vegetation_density: 0.5,
                vegetation_health: 1.0,
                transition_pressure: 0.0,
                ticks_in_current_biome: 0,
            },
            resources: ResourceLayer {
                resources: Vec::new(),
            },
            weather: WeatherLayer {
                temperature: 288.15,
                precipitation: 0.0,
                precipitation_type: PrecipitationType::None,
                wind_speed: 0.0,
                wind_direction: 0.0,
                cloud_cover: 0.3,
                storm_intensity: 0.0,
            },
            conditions: ConditionsLayer {
                soil_moisture: 0.3,
                snow_depth: 0.0,
                mud_level: 0.0,
                flood_level: 0.0,
                frost_days: 0,
                drought_days: 0,
                fire_risk: 0.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_creation_has_all_layers() {
        let tile = Tile::new_default(0, vec![1, 2, 3, 4, 5, 6], Position { x: 0.0, y: 0.0 });
        assert_eq!(tile.id, 0);
        assert_eq!(tile.neighbors.len(), 6);
        assert_eq!(tile.geology.terrain_type, TerrainType::Plains);
        assert_eq!(tile.geology.elevation, 0.0);
        assert_eq!(tile.geology.soil_type, SoilType::Loam);
        assert_eq!(tile.climate.zone, ClimateZone::Temperate);
        assert_eq!(tile.climate.base_temperature, 288.15);
        assert_eq!(tile.biome.biome_type, BiomeType::Grassland);
        assert_eq!(tile.biome.vegetation_health, 1.0);
        assert!(tile.resources.resources.is_empty());
        assert_eq!(tile.weather.precipitation_type, PrecipitationType::None);
        assert_eq!(tile.weather.storm_intensity, 0.0);
        assert_eq!(tile.conditions.frost_days, 0);
        assert_eq!(tile.conditions.drought_days, 0);
    }

    #[test]
    fn tile_serde_round_trip() {
        let mut tile =
            Tile::new_default(42, vec![1, 2, 3, 4, 5, 6], Position { x: 10.5, y: 20.3 });
        tile.resources.resources.push(ResourceDeposit {
            resource_type: "iron".to_string(),
            quantity: 50.0,
            max_quantity: 100.0,
            renewal_rate: 0.0,
            requires_biome: Some(vec![BiomeType::Grassland, BiomeType::BorealForest]),
        });
        let encoded = bincode::serialize(&tile).expect("serialize");
        let decoded: Tile = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(tile, decoded);
    }

    #[test]
    fn season_cycles_correctly() {
        assert_eq!(Season::Spring.next(), Season::Summer);
        assert_eq!(Season::Summer.next(), Season::Autumn);
        assert_eq!(Season::Autumn.next(), Season::Winter);
        assert_eq!(Season::Winter.next(), Season::Spring);
    }

    #[test]
    fn all_terrain_types_serialize() {
        let types = [
            TerrainType::Ocean,
            TerrainType::Coast,
            TerrainType::Plains,
            TerrainType::Hills,
            TerrainType::Mountains,
            TerrainType::Cliffs,
            TerrainType::Wetlands,
        ];
        for t in &types {
            let encoded = bincode::serialize(t).expect("serialize");
            let decoded: TerrainType = bincode::deserialize(&encoded).expect("deserialize");
            assert_eq!(*t, decoded);
        }
    }

    #[test]
    fn all_biome_types_serialize() {
        let types = [
            BiomeType::Ocean,
            BiomeType::Ice,
            BiomeType::Tundra,
            BiomeType::BorealForest,
            BiomeType::TemperateForest,
            BiomeType::Grassland,
            BiomeType::Savanna,
            BiomeType::Desert,
            BiomeType::TropicalForest,
            BiomeType::Wetland,
            BiomeType::Barren,
        ];
        for b in &types {
            let encoded = bincode::serialize(b).expect("serialize");
            let decoded: BiomeType = bincode::deserialize(&encoded).expect("deserialize");
            assert_eq!(*b, decoded);
        }
    }
}
