use std::collections::HashMap;

use crate::world::tile::{BiomeType, PrecipitationType};
use crate::world::World;

/// Per-tick aggregate metrics for introspection and degenerate state detection.
#[derive(Debug, Clone)]
pub struct TickStatistics {
    pub tick: u64,
    pub biome_distribution: HashMap<BiomeType, u32>,
    pub avg_temperature: f32,
    pub avg_moisture: f32,
    pub avg_vegetation_health: f32,
    pub weather_coverage: HashMap<PrecipitationType, u32>,
    pub diversity_index: f32,
    pub rule_errors: u32,
    pub tick_duration_ms: f32,
}

/// Compute statistics for the current world state after a tick.
pub fn compute_statistics(
    world: &World,
    rule_errors: u32,
    tick_duration_ms: f32,
) -> TickStatistics {
    let total = world.tiles.len() as f64;
    if total == 0.0 {
        return TickStatistics {
            tick: world.tick_count,
            biome_distribution: HashMap::new(),
            avg_temperature: 0.0,
            avg_moisture: 0.0,
            avg_vegetation_health: 0.0,
            weather_coverage: HashMap::new(),
            diversity_index: 0.0,
            rule_errors,
            tick_duration_ms,
        };
    }

    let mut biome_dist: HashMap<BiomeType, u32> = HashMap::new();
    let mut weather_cov: HashMap<PrecipitationType, u32> = HashMap::new();
    let mut total_temp = 0.0_f64;
    let mut total_moisture = 0.0_f64;
    let mut total_veg_health = 0.0_f64;

    for tile in &world.tiles {
        *biome_dist.entry(tile.biome.biome_type).or_insert(0) += 1;
        *weather_cov
            .entry(tile.weather.precipitation_type)
            .or_insert(0) += 1;
        total_temp += tile.weather.temperature as f64;
        total_moisture += tile.conditions.soil_moisture as f64;
        total_veg_health += tile.biome.vegetation_health as f64;
    }

    let diversity = shannon_diversity(&biome_dist, world.tiles.len() as u32);

    TickStatistics {
        tick: world.tick_count,
        biome_distribution: biome_dist,
        avg_temperature: (total_temp / total) as f32,
        avg_moisture: (total_moisture / total) as f32,
        avg_vegetation_health: (total_veg_health / total) as f32,
        weather_coverage: weather_cov,
        diversity_index: diversity,
        rule_errors,
        tick_duration_ms,
    }
}

/// Shannon diversity index normalized to [0, 1].
/// 0 = monoculture (all tiles same biome), 1 = maximum diversity (all types equally represented).
fn shannon_diversity(distribution: &HashMap<BiomeType, u32>, total: u32) -> f32 {
    if total == 0 {
        return 0.0;
    }

    let total_f = total as f64;
    let mut entropy = 0.0_f64;
    let mut non_zero_types = 0_u32;

    for &count in distribution.values() {
        if count > 0 {
            non_zero_types += 1;
            let p = count as f64 / total_f;
            entropy -= p * p.ln();
        }
    }

    if non_zero_types <= 1 {
        return 0.0;
    }

    // Normalize by max possible entropy (ln of number of types present)
    let max_entropy = (non_zero_types as f64).ln();
    if max_entropy == 0.0 {
        0.0
    } else {
        (entropy / max_entropy) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::tile::{BiomeType, PrecipitationType, Position, Tile};
    use crate::world::World;
    use crate::config::generation::GenerationParams;
    use uuid::Uuid;

    fn make_test_world(tile_count: usize) -> World {
        let tiles: Vec<Tile> = (0..tile_count)
            .map(|i| Tile::new_default(i as u32, vec![], Position { x: 0.0, y: 0.0 }))
            .collect();
        World {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            created_at: "2026-01-01".to_string(),
            tick_count: 1,
            season: crate::world::tile::Season::Spring,
            season_length: 100,
            tile_count: tile_count as u32,
            topology_type: crate::world::tile::TopologyType::FlatHex,
            generation_params: GenerationParams {
                seed: 42,
                tile_count: tile_count as u32,
                ocean_ratio: 0.6,
                mountain_ratio: 0.1,
                elevation_roughness: 0.5,
                climate_bands: true,
                resource_density: 0.3,
                initial_biome_maturity: 0.5,
            },
            snapshot_path: None,
            tiles,
        }
    }

    #[test]
    fn compute_statistics_basic_averages() {
        let mut world = make_test_world(3);
        world.tiles[0].weather.temperature = 280.0;
        world.tiles[1].weather.temperature = 290.0;
        world.tiles[2].weather.temperature = 300.0;
        world.tiles[0].conditions.soil_moisture = 0.2;
        world.tiles[1].conditions.soil_moisture = 0.4;
        world.tiles[2].conditions.soil_moisture = 0.6;

        let stats = compute_statistics(&world, 0, 10.0);

        assert!((stats.avg_temperature - 290.0).abs() < 0.01);
        assert!((stats.avg_moisture - 0.4).abs() < 0.01);
        assert_eq!(stats.tick, 1);
        assert_eq!(stats.rule_errors, 0);
        assert!((stats.tick_duration_ms - 10.0).abs() < 0.01);
    }

    #[test]
    fn compute_statistics_biome_distribution() {
        let mut world = make_test_world(4);
        world.tiles[0].biome.biome_type = BiomeType::Grassland;
        world.tiles[1].biome.biome_type = BiomeType::Grassland;
        world.tiles[2].biome.biome_type = BiomeType::Desert;
        world.tiles[3].biome.biome_type = BiomeType::Ocean;

        let stats = compute_statistics(&world, 0, 5.0);

        assert_eq!(stats.biome_distribution[&BiomeType::Grassland], 2);
        assert_eq!(stats.biome_distribution[&BiomeType::Desert], 1);
        assert_eq!(stats.biome_distribution[&BiomeType::Ocean], 1);
    }

    #[test]
    fn diversity_index_monoculture_is_zero() {
        let world = make_test_world(10); // All default to Grassland
        let stats = compute_statistics(&world, 0, 1.0);
        assert_eq!(stats.diversity_index, 0.0);
    }

    #[test]
    fn diversity_index_multiple_biomes_positive() {
        let mut world = make_test_world(4);
        world.tiles[0].biome.biome_type = BiomeType::Grassland;
        world.tiles[1].biome.biome_type = BiomeType::Desert;
        world.tiles[2].biome.biome_type = BiomeType::Ocean;
        world.tiles[3].biome.biome_type = BiomeType::Tundra;

        let stats = compute_statistics(&world, 0, 1.0);
        // 4 equal types: Shannon entropy = ln(4), normalized = 1.0
        assert!((stats.diversity_index - 1.0).abs() < 0.01);
    }

    #[test]
    fn weather_coverage_counted() {
        let mut world = make_test_world(3);
        world.tiles[0].weather.precipitation_type = PrecipitationType::Rain;
        world.tiles[1].weather.precipitation_type = PrecipitationType::Rain;
        world.tiles[2].weather.precipitation_type = PrecipitationType::None;

        let stats = compute_statistics(&world, 0, 1.0);
        assert_eq!(stats.weather_coverage[&PrecipitationType::Rain], 2);
        assert_eq!(stats.weather_coverage[&PrecipitationType::None], 1);
    }

    #[test]
    fn empty_world_returns_zeroed_stats() {
        let world = make_test_world(0);
        let stats = compute_statistics(&world, 0, 0.0);
        assert_eq!(stats.diversity_index, 0.0);
        assert_eq!(stats.avg_temperature, 0.0);
    }
}
