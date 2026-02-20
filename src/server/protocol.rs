use serde::Serialize;
use std::collections::HashMap;

use crate::simulation::statistics::TickStatistics;
use crate::world::tile::*;
use crate::world::World;

/// Complete world state sent to a client on connect.
#[derive(Debug, Clone, Serialize)]
pub struct WorldSnapshot {
    pub message_type: &'static str,
    pub world_id: String,
    pub name: String,
    pub tick: u64,
    pub season: Season,
    pub season_length: u32,
    pub tile_count: u32,
    pub tiles: Vec<TileSnapshot>,
}

/// A tile's complete state in a snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct TileSnapshot {
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

/// Per-tick diff sent after each simulation tick.
#[derive(Debug, Clone, Serialize)]
pub struct TickDiff {
    pub message_type: &'static str,
    pub tick: u64,
    pub season: Season,
    pub changed_tiles: Vec<TileChange>,
    pub statistics: TickStatSummary,
}

/// Changed fields for a single tile in a diff.
#[derive(Debug, Clone, Serialize)]
pub struct TileChange {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weather: Option<WeatherLayer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditions: Option<ConditionsLayer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biome: Option<BiomeLayer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceLayer>,
}

/// Summary statistics included in tick diffs.
#[derive(Debug, Clone, Serialize)]
pub struct TickStatSummary {
    pub tick: u64,
    pub biome_distribution: HashMap<String, u32>,
    pub avg_temperature: f32,
    pub avg_moisture: f32,
    pub avg_vegetation_health: f32,
    pub diversity_index: f32,
    pub rule_errors: u32,
    pub tick_duration_ms: f32,
}

/// Health endpoint response.
#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    pub tick: u64,
    pub tick_rate: f32,
    pub diversity_index: f32,
    pub rule_errors: u32,
    pub snapshot_age_ticks: u64,
    pub tile_count: u32,
    pub season: Season,
}

impl WorldSnapshot {
    pub fn from_world(world: &World) -> Self {
        WorldSnapshot {
            message_type: "WorldSnapshot",
            world_id: world.id.to_string(),
            name: world.name.clone(),
            tick: world.tick_count,
            season: world.season,
            season_length: world.season_length,
            tile_count: world.tile_count,
            tiles: world.tiles.iter().map(TileSnapshot::from_tile).collect(),
        }
    }
}

impl TileSnapshot {
    pub fn from_tile(tile: &Tile) -> Self {
        TileSnapshot {
            id: tile.id,
            neighbors: tile.neighbors.clone(),
            position: tile.position,
            geology: tile.geology.clone(),
            climate: tile.climate.clone(),
            biome: tile.biome.clone(),
            resources: tile.resources.clone(),
            weather: tile.weather.clone(),
            conditions: tile.conditions.clone(),
        }
    }
}

impl TickStatSummary {
    pub fn from_statistics(stats: &TickStatistics) -> Self {
        TickStatSummary {
            tick: stats.tick,
            biome_distribution: stats
                .biome_distribution
                .iter()
                .map(|(k, v)| (format!("{:?}", k), *v))
                .collect(),
            avg_temperature: stats.avg_temperature,
            avg_moisture: stats.avg_moisture,
            avg_vegetation_health: stats.avg_vegetation_health,
            diversity_index: stats.diversity_index,
            rule_errors: stats.rule_errors,
            tick_duration_ms: stats.tick_duration_ms,
        }
    }
}

/// Compute tile-level diffs between two world states.
/// Returns only tiles where weather, conditions, biome, or resources changed.
pub fn compute_tile_diffs(before: &[Tile], after: &[Tile]) -> Vec<TileChange> {
    let mut changes = Vec::new();

    for (old, new) in before.iter().zip(after.iter()) {
        let weather_changed = old.weather != new.weather;
        let conditions_changed = old.conditions != new.conditions;
        let biome_changed = old.biome != new.biome;
        let resources_changed = old.resources != new.resources;

        if weather_changed || conditions_changed || biome_changed || resources_changed {
            changes.push(TileChange {
                id: new.id,
                weather: if weather_changed {
                    Some(new.weather.clone())
                } else {
                    None
                },
                conditions: if conditions_changed {
                    Some(new.conditions.clone())
                } else {
                    None
                },
                biome: if biome_changed {
                    Some(new.biome.clone())
                } else {
                    None
                },
                resources: if resources_changed {
                    Some(new.resources.clone())
                } else {
                    None
                },
            });
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::tile::{Position, Tile};

    fn make_tile(id: u32) -> Tile {
        Tile::new_default(id, vec![], Position { x: 0.0, y: 0.0 })
    }

    #[test]
    fn world_snapshot_contains_all_tiles() {
        let world = crate::world::World {
            id: uuid::Uuid::new_v4(),
            name: "test".to_string(),
            created_at: "2026-01-01".to_string(),
            tick_count: 42,
            season: Season::Summer,
            season_length: 100,
            tile_count: 3,
            topology_type: TopologyType::FlatHex,
            generation_params: crate::config::generation::GenerationParams {
                seed: 1,
                tile_count: 3,
                ocean_ratio: 0.6,
                mountain_ratio: 0.1,
                elevation_roughness: 0.5,
                climate_bands: true,
                resource_density: 0.3,
                initial_biome_maturity: 0.5,
            },
            snapshot_path: None,
            tiles: vec![make_tile(0), make_tile(1), make_tile(2)],
        };

        let snapshot = WorldSnapshot::from_world(&world);
        assert_eq!(snapshot.message_type, "WorldSnapshot");
        assert_eq!(snapshot.tick, 42);
        assert_eq!(snapshot.season, Season::Summer);
        assert_eq!(snapshot.tiles.len(), 3);
        assert_eq!(snapshot.tiles[0].id, 0);
        assert_eq!(snapshot.tiles[2].id, 2);
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let world = crate::world::World {
            id: uuid::Uuid::new_v4(),
            name: "json_test".to_string(),
            created_at: "2026-01-01".to_string(),
            tick_count: 0,
            season: Season::Spring,
            season_length: 100,
            tile_count: 1,
            topology_type: TopologyType::FlatHex,
            generation_params: crate::config::generation::GenerationParams {
                seed: 1,
                tile_count: 1,
                ocean_ratio: 0.6,
                mountain_ratio: 0.1,
                elevation_roughness: 0.5,
                climate_bands: true,
                resource_density: 0.3,
                initial_biome_maturity: 0.5,
            },
            snapshot_path: None,
            tiles: vec![make_tile(0)],
        };

        let snapshot = WorldSnapshot::from_world(&world);
        let json = serde_json::to_string(&snapshot).expect("serialization should succeed");
        assert!(json.contains("\"message_type\":\"WorldSnapshot\""));
        assert!(json.contains("\"name\":\"json_test\""));
    }

    #[test]
    fn diff_detects_weather_change() {
        let before = vec![make_tile(0), make_tile(1)];
        let mut after = before.clone();
        after[0].weather.temperature = 999.0;

        let diffs = compute_tile_diffs(&before, &after);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].id, 0);
        assert!(diffs[0].weather.is_some());
        assert!(diffs[0].conditions.is_none());
        assert!(diffs[0].biome.is_none());
    }

    #[test]
    fn diff_detects_multiple_layer_changes() {
        let before = vec![make_tile(0)];
        let mut after = before.clone();
        after[0].weather.temperature = 300.0;
        after[0].conditions.soil_moisture = 0.9;
        after[0].biome.vegetation_health = 0.1;

        let diffs = compute_tile_diffs(&before, &after);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].weather.is_some());
        assert!(diffs[0].conditions.is_some());
        assert!(diffs[0].biome.is_some());
        assert!(diffs[0].resources.is_none());
    }

    #[test]
    fn diff_empty_when_no_changes() {
        let tiles = vec![make_tile(0), make_tile(1), make_tile(2)];
        let diffs = compute_tile_diffs(&tiles, &tiles);
        assert!(diffs.is_empty());
    }

    #[test]
    fn diff_only_includes_changed_tiles() {
        let before = vec![make_tile(0), make_tile(1), make_tile(2)];
        let mut after = before.clone();
        after[1].weather.precipitation = 0.8;

        let diffs = compute_tile_diffs(&before, &after);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].id, 1);
    }

    #[test]
    fn tick_diff_serializes_to_json() {
        let diff = TickDiff {
            message_type: "TickDiff",
            tick: 5,
            season: Season::Winter,
            changed_tiles: vec![TileChange {
                id: 42,
                weather: Some(WeatherLayer {
                    temperature: 260.0,
                    precipitation: 0.5,
                    precipitation_type: PrecipitationType::Snow,
                    wind_speed: 10.0,
                    wind_direction: 180.0,
                    cloud_cover: 0.9,
                    humidity: 0.7,
                    storm_intensity: 0.3,
                }),
                conditions: None,
                biome: None,
                resources: None,
            }],
            statistics: TickStatSummary {
                tick: 5,
                biome_distribution: HashMap::new(),
                avg_temperature: 270.0,
                avg_moisture: 0.4,
                avg_vegetation_health: 0.6,
                diversity_index: 0.8,
                rule_errors: 0,
                tick_duration_ms: 50.0,
            },
        };

        let json = serde_json::to_string(&diff).expect("serialization should succeed");
        assert!(json.contains("\"message_type\":\"TickDiff\""));
        assert!(json.contains("\"tick\":5"));
        // Null layers should not appear in JSON (skip_serializing_if)
        assert!(!json.contains("\"conditions\":null"));
        assert!(!json.contains("\"biome\":null"));
    }

    #[test]
    fn health_status_serializes() {
        let health = HealthStatus {
            tick: 100,
            tick_rate: 1.0,
            diversity_index: 0.7,
            rule_errors: 0,
            snapshot_age_ticks: 5,
            tile_count: 1000,
            season: Season::Autumn,
        };

        let json = serde_json::to_string(&health).expect("serialization should succeed");
        assert!(json.contains("\"tick\":100"));
        assert!(json.contains("\"tick_rate\":1.0"));
    }
}
