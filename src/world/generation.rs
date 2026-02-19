use std::collections::HashMap;

use noise::{NoiseFn, Perlin};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use uuid::Uuid;

use crate::config::generation::GenerationParams;
use crate::world::tile::*;
use crate::world::topology::{generate_flat_hex_grid, grid_dimensions};
use crate::world::World;

/// Generate a new world from the given parameters.
///
/// If `params.seed` is 0, a random seed is chosen. The actual seed used
/// is stored in the returned World's `generation_params` for reproducibility.
pub fn generate_world(params: &GenerationParams) -> World {
    let seed = if params.seed == 0 {
        rand::thread_rng().r#gen()
    } else {
        params.seed
    };
    let resolved_params = GenerationParams {
        seed,
        ..params.clone()
    };
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let (width, height) = grid_dimensions(params.tile_count);
    let mut tiles = generate_flat_hex_grid(width, height);
    let actual_count = tiles.len() as u32;

    generate_elevation(&mut tiles, seed as u32, params.elevation_roughness);
    assign_terrain_types(&mut tiles, params.ocean_ratio, params.mountain_ratio);
    assign_climate(&mut tiles, height, params.climate_bands);
    assign_soil(&mut tiles, seed.wrapping_add(1) as u32);
    assign_initial_biomes(&mut tiles, params.initial_biome_maturity);
    scatter_resources(&mut tiles, &mut rng, params.resource_density);
    initialize_weather(&mut tiles, &mut rng);
    initialize_conditions(&mut tiles);

    let id = Uuid::from_bytes(rng.r#gen());

    World {
        id,
        name: format!("World-{}", seed),
        created_at: format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ),
        tick_count: 0,
        season: Season::Spring,
        season_length: 90,
        tile_count: actual_count,
        topology_type: TopologyType::FlatHex,
        generation_params: resolved_params,
        snapshot_path: None,
        tiles,
    }
}

/// Print a summary of the generated world.
pub fn print_world_summary(world: &World) {
    println!("=== World Summary ===");
    println!("Name: {}", world.name);
    println!("Tiles: {}", world.tile_count);
    println!("Seed: {}", world.generation_params.seed);

    let mut terrain_counts: HashMap<&str, u32> = HashMap::new();
    for tile in &world.tiles {
        let name = match tile.geology.terrain_type {
            TerrainType::Ocean => "Ocean",
            TerrainType::Coast => "Coast",
            TerrainType::Plains => "Plains",
            TerrainType::Hills => "Hills",
            TerrainType::Mountains => "Mountains",
            TerrainType::Cliffs => "Cliffs",
            TerrainType::Wetlands => "Wetlands",
        };
        *terrain_counts.entry(name).or_insert(0) += 1;
    }
    let mut terrain_sorted: Vec<_> = terrain_counts.into_iter().collect();
    terrain_sorted.sort_by_key(|&(name, _)| name);
    println!("\nTerrain:");
    for (name, count) in &terrain_sorted {
        let pct = *count as f32 / world.tile_count as f32 * 100.0;
        println!("  {:<12} {:>5} ({:.1}%)", name, count, pct);
    }

    let mut biome_counts: HashMap<&str, u32> = HashMap::new();
    for tile in &world.tiles {
        let name = match tile.biome.biome_type {
            BiomeType::Ocean => "Ocean",
            BiomeType::Ice => "Ice",
            BiomeType::Tundra => "Tundra",
            BiomeType::BorealForest => "Boreal Forest",
            BiomeType::TemperateForest => "Temperate Forest",
            BiomeType::Grassland => "Grassland",
            BiomeType::Savanna => "Savanna",
            BiomeType::Desert => "Desert",
            BiomeType::TropicalForest => "Tropical Forest",
            BiomeType::Wetland => "Wetland",
            BiomeType::Barren => "Barren",
        };
        *biome_counts.entry(name).or_insert(0) += 1;
    }
    let mut biome_sorted: Vec<_> = biome_counts.into_iter().collect();
    biome_sorted.sort_by_key(|&(name, _)| name);
    println!("\nBiomes:");
    for (name, count) in &biome_sorted {
        let pct = *count as f32 / world.tile_count as f32 * 100.0;
        println!("  {:<18} {:>5} ({:.1}%)", name, count, pct);
    }

    let mut resource_totals: HashMap<&str, (u32, f32)> = HashMap::new();
    for tile in &world.tiles {
        for deposit in &tile.resources.resources {
            let entry = resource_totals
                .entry(deposit.resource_type.as_str())
                .or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += deposit.quantity;
        }
    }
    if !resource_totals.is_empty() {
        let mut resource_sorted: Vec<_> = resource_totals.into_iter().collect();
        resource_sorted.sort_by_key(|&(name, _)| name);
        println!("\nResources:");
        for (name, (count, total)) in &resource_sorted {
            println!("  {:<12} {:>5} deposits, {:.0} total", name, count, total);
        }
    }
}

// --- Internal generation functions ---

fn generate_elevation(tiles: &mut [Tile], seed: u32, roughness: f32) {
    let perlin = Perlin::new(seed);
    let scale = 0.08;
    for tile in tiles.iter_mut() {
        let nx = tile.position.x * scale;
        let ny = tile.position.y * scale;
        let e = perlin.get([nx, ny]) as f32;
        tile.geology.elevation = (e * roughness).clamp(-1.0, 1.0);
    }
}

fn assign_terrain_types(tiles: &mut [Tile], ocean_ratio: f32, mountain_ratio: f32) {
    // Sort tile indices by elevation to assign types by percentile
    let mut indices: Vec<usize> = (0..tiles.len()).collect();
    indices.sort_by(|&a, &b| {
        tiles[a]
            .geology
            .elevation
            .partial_cmp(&tiles[b].geology.elevation)
            .unwrap()
    });

    let ocean_count = (tiles.len() as f32 * ocean_ratio).round() as usize;

    // Lowest tiles → Ocean
    for &idx in &indices[..ocean_count] {
        tiles[idx].geology.terrain_type = TerrainType::Ocean;
    }

    // Land tiles: assign by elevation rank
    let land_indices: Vec<usize> = indices[ocean_count..].to_vec();
    let land_count = land_indices.len();

    if land_count == 0 {
        return;
    }

    let mountain_count = (land_count as f32 * mountain_ratio).round() as usize;
    let hills_count = mountain_count; // Similar number of hills

    // Assign from highest elevation down
    for (i, &idx) in land_indices.iter().rev().enumerate() {
        if i < mountain_count {
            tiles[idx].geology.terrain_type = TerrainType::Mountains;
        } else if i < mountain_count + hills_count {
            tiles[idx].geology.terrain_type = TerrainType::Hills;
        } else {
            tiles[idx].geology.terrain_type = TerrainType::Plains;
        }
    }

    // Coast and Cliffs: land tiles adjacent to ocean
    let ocean_set: std::collections::HashSet<u32> = tiles
        .iter()
        .filter(|t| t.geology.terrain_type == TerrainType::Ocean)
        .map(|t| t.id)
        .collect();

    let to_update: Vec<(usize, TerrainType)> = tiles
        .iter()
        .enumerate()
        .filter(|(_, t)| t.geology.terrain_type != TerrainType::Ocean)
        .filter(|(_, t)| t.neighbors.iter().any(|n| ocean_set.contains(n)))
        .map(|(i, t)| {
            let new_type = match t.geology.terrain_type {
                TerrainType::Mountains | TerrainType::Hills => TerrainType::Cliffs,
                _ => TerrainType::Coast,
            };
            (i, new_type)
        })
        .collect();

    for (idx, terrain) in to_update {
        tiles[idx].geology.terrain_type = terrain;
    }

    // Wetlands: lowest-elevation plains
    let mut land_elevations: Vec<f32> = tiles
        .iter()
        .filter(|t| t.geology.terrain_type == TerrainType::Plains)
        .map(|t| t.geology.elevation)
        .collect();

    if !land_elevations.is_empty() {
        land_elevations.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let low_threshold = land_elevations[land_elevations.len() / 10];

        for tile in tiles.iter_mut() {
            if tile.geology.terrain_type == TerrainType::Plains
                && tile.geology.elevation <= low_threshold
            {
                tile.geology.terrain_type = TerrainType::Wetlands;
            }
        }
    }
}

fn assign_climate(tiles: &mut [Tile], grid_height: u32, use_bands: bool) {
    let max_y = 1.5 * (grid_height.saturating_sub(1)) as f64;

    for tile in tiles.iter_mut() {
        let latitude = if max_y > 0.0 {
            ((tile.position.y / max_y) * 180.0 - 90.0) as f32
        } else {
            0.0
        };
        tile.climate.latitude = latitude;

        if use_bands {
            let abs_lat = latitude.abs();
            tile.climate.zone = if abs_lat > 60.0 {
                ClimateZone::Polar
            } else if abs_lat > 45.0 {
                ClimateZone::Subpolar
            } else if abs_lat > 30.0 {
                ClimateZone::Temperate
            } else if abs_lat > 15.0 {
                ClimateZone::Subtropical
            } else {
                ClimateZone::Tropical
            };
        }

        let zone_temp = match tile.climate.zone {
            ClimateZone::Polar => 250.0,
            ClimateZone::Subpolar => 265.0,
            ClimateZone::Temperate => 283.0,
            ClimateZone::Subtropical => 295.0,
            ClimateZone::Tropical => 300.0,
        };
        // Elevation lapse: higher = colder
        let elevation_effect = tile.geology.elevation.max(0.0) * 20.0;
        tile.climate.base_temperature = zone_temp - elevation_effect;

        tile.climate.base_precipitation = match tile.climate.zone {
            ClimateZone::Polar => 0.2,
            ClimateZone::Subpolar => 0.3,
            ClimateZone::Temperate => 0.5,
            ClimateZone::Subtropical => 0.4,
            ClimateZone::Tropical => 0.7,
        };
    }
}

fn assign_soil(tiles: &mut [Tile], seed: u32) {
    let perlin = Perlin::new(seed);
    let scale = 0.12;

    for tile in tiles.iter_mut() {
        match tile.geology.terrain_type {
            TerrainType::Ocean => {
                tile.geology.soil_type = SoilType::Sand;
                tile.geology.drainage = 1.0;
                continue;
            }
            TerrainType::Mountains | TerrainType::Cliffs => {
                tile.geology.soil_type = SoilType::Rock;
                tile.geology.drainage = 0.9;
                continue;
            }
            TerrainType::Wetlands => {
                tile.geology.soil_type = SoilType::Silt;
                tile.geology.drainage = 0.1;
                continue;
            }
            _ => {}
        }

        let n = perlin.get([tile.position.x * scale, tile.position.y * scale]) as f32;
        let (soil, drainage) = if n < -0.4 {
            (SoilType::Sand, 0.8)
        } else if n < -0.1 {
            (SoilType::Clay, 0.2)
        } else if n < 0.2 {
            (SoilType::Loam, 0.5)
        } else if n < 0.5 {
            (SoilType::Silt, 0.3)
        } else {
            (SoilType::Rock, 0.7)
        };
        tile.geology.soil_type = soil;
        tile.geology.drainage = drainage;
    }
}

fn assign_initial_biomes(tiles: &mut [Tile], maturity: f32) {
    for tile in tiles.iter_mut() {
        let biome = if tile.geology.terrain_type == TerrainType::Wetlands {
            BiomeType::Wetland
        } else {
            match tile.geology.terrain_type {
                TerrainType::Ocean => BiomeType::Ocean,
                TerrainType::Coast => match tile.climate.zone {
                    ClimateZone::Polar => BiomeType::Ice,
                    _ => BiomeType::Grassland,
                },
                _ => match tile.climate.zone {
                    ClimateZone::Polar => {
                        if tile.geology.elevation > 0.3 {
                            BiomeType::Ice
                        } else {
                            BiomeType::Tundra
                        }
                    }
                    ClimateZone::Subpolar => BiomeType::BorealForest,
                    ClimateZone::Temperate => {
                        if tile.climate.base_precipitation > 0.4 {
                            BiomeType::TemperateForest
                        } else {
                            BiomeType::Grassland
                        }
                    }
                    ClimateZone::Subtropical => {
                        if tile.climate.base_precipitation > 0.5 {
                            BiomeType::Savanna
                        } else if tile.climate.base_precipitation < 0.2 {
                            BiomeType::Desert
                        } else {
                            BiomeType::Grassland
                        }
                    }
                    ClimateZone::Tropical => {
                        if tile.climate.base_precipitation > 0.5 {
                            BiomeType::TropicalForest
                        } else {
                            BiomeType::Savanna
                        }
                    }
                },
            }
        };

        tile.biome.biome_type = biome;

        tile.biome.vegetation_density = match biome {
            BiomeType::Ocean | BiomeType::Ice | BiomeType::Barren => 0.0,
            BiomeType::Desert => 0.05,
            BiomeType::Tundra => 0.15,
            BiomeType::Grassland | BiomeType::Savanna => 0.4,
            BiomeType::BorealForest | BiomeType::TemperateForest => 0.7,
            BiomeType::TropicalForest => 0.9,
            BiomeType::Wetland => 0.5,
        };

        tile.biome.vegetation_health = match biome {
            BiomeType::Ocean | BiomeType::Ice => 0.0,
            _ => 0.8,
        };

        tile.biome.ticks_in_current_biome = (maturity * 100.0) as u32;
    }
}

fn scatter_resources(tiles: &mut [Tile], rng: &mut impl Rng, density: f32) {
    for tile in tiles.iter_mut() {
        tile.resources.resources.clear();

        if tile.geology.terrain_type == TerrainType::Ocean {
            continue;
        }

        if matches!(
            tile.geology.terrain_type,
            TerrainType::Mountains | TerrainType::Hills
        ) {
            if rng.r#gen::<f32>() < density * 0.5 {
                tile.resources.resources.push(ResourceDeposit {
                    resource_type: "iron".to_string(),
                    quantity: rng.gen_range(20.0..100.0),
                    max_quantity: 100.0,
                    renewal_rate: 0.0,
                    requires_biome: None,
                });
            }
            if rng.r#gen::<f32>() < density * 0.3 {
                tile.resources.resources.push(ResourceDeposit {
                    resource_type: "stone".to_string(),
                    quantity: rng.gen_range(50.0..200.0),
                    max_quantity: 200.0,
                    renewal_rate: 0.0,
                    requires_biome: None,
                });
            }
        }

        if matches!(
            tile.biome.biome_type,
            BiomeType::BorealForest | BiomeType::TemperateForest | BiomeType::TropicalForest
        ) {
            if rng.r#gen::<f32>() < density * 0.7 {
                tile.resources.resources.push(ResourceDeposit {
                    resource_type: "timber".to_string(),
                    quantity: rng.gen_range(30.0..80.0),
                    max_quantity: 80.0,
                    renewal_rate: 0.1,
                    requires_biome: Some(vec![
                        BiomeType::BorealForest,
                        BiomeType::TemperateForest,
                        BiomeType::TropicalForest,
                    ]),
                });
            }
        }

        if matches!(
            tile.biome.biome_type,
            BiomeType::Grassland | BiomeType::Savanna
        ) && matches!(tile.geology.soil_type, SoilType::Loam | SoilType::Silt)
        {
            if rng.r#gen::<f32>() < density * 0.6 {
                tile.resources.resources.push(ResourceDeposit {
                    resource_type: "grain".to_string(),
                    quantity: rng.gen_range(10.0..50.0),
                    max_quantity: 50.0,
                    renewal_rate: 0.5,
                    requires_biome: Some(vec![BiomeType::Grassland, BiomeType::Savanna]),
                });
            }
        }
    }
}

fn initialize_weather(tiles: &mut [Tile], rng: &mut impl Rng) {
    for tile in tiles.iter_mut() {
        tile.weather.temperature = tile.climate.base_temperature + rng.gen_range(-2.0..2.0);
        tile.weather.precipitation = if rng.r#gen::<f32>() < tile.climate.base_precipitation {
            rng.gen_range(0.1..0.5)
        } else {
            0.0
        };
        tile.weather.precipitation_type = if tile.weather.precipitation > 0.0 {
            if tile.weather.temperature < 273.15 {
                PrecipitationType::Snow
            } else {
                PrecipitationType::Rain
            }
        } else {
            PrecipitationType::None
        };
        tile.weather.wind_speed = rng.gen_range(0.0..10.0);
        tile.weather.wind_direction = rng.gen_range(0.0..360.0);
        tile.weather.cloud_cover =
            (tile.climate.base_precipitation * 0.8 + rng.gen_range(-0.1..0.1)).clamp(0.0, 1.0);
        tile.weather.storm_intensity = 0.0;
    }
}

fn initialize_conditions(tiles: &mut [Tile]) {
    for tile in tiles.iter_mut() {
        tile.conditions.soil_moisture = match tile.geology.terrain_type {
            TerrainType::Ocean => 1.0,
            TerrainType::Wetlands => 0.8,
            _ => tile.climate.base_precipitation * 0.6,
        };
        tile.conditions.snow_depth =
            if tile.weather.temperature < 273.15 && tile.weather.precipitation > 0.0 {
                tile.weather.precipitation * 2.0
            } else {
                0.0
            };
        tile.conditions.mud_level = 0.0;
        tile.conditions.flood_level = 0.0;
        tile.conditions.frost_days = if tile.weather.temperature < 273.15 {
            1
        } else {
            0
        };
        tile.conditions.drought_days = 0;
        tile.conditions.fire_risk = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> GenerationParams {
        GenerationParams {
            seed: 42,
            tile_count: 1000,
            ocean_ratio: 0.6,
            mountain_ratio: 0.1,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
        }
    }

    #[test]
    fn generate_default_world_correct_tile_count() {
        let world = generate_world(&default_params());
        assert!(
            world.tiles.len() >= 1000,
            "Expected >= 1000 tiles, got {}",
            world.tiles.len()
        );
    }

    #[test]
    fn generate_default_world_ocean_ratio() {
        let world = generate_world(&default_params());
        let ocean_count = world
            .tiles
            .iter()
            .filter(|t| t.geology.terrain_type == TerrainType::Ocean)
            .count();
        let ocean_pct = ocean_count as f32 / world.tiles.len() as f32;
        // Allow tolerance since coast/cliff tiles reduce pure ocean count
        assert!(
            ocean_pct > 0.4 && ocean_pct < 0.8,
            "Expected ~60% ocean, got {:.1}% ({} of {})",
            ocean_pct * 100.0,
            ocean_count,
            world.tiles.len()
        );
    }

    #[test]
    fn generate_default_world_all_layers_populated() {
        let world = generate_world(&default_params());
        for tile in &world.tiles {
            assert!(
                tile.geology.elevation >= -1.0 && tile.geology.elevation <= 1.0,
                "Tile {} elevation out of range: {}",
                tile.id,
                tile.geology.elevation
            );
            assert!(
                tile.climate.latitude >= -90.0 && tile.climate.latitude <= 90.0,
                "Tile {} latitude out of range: {}",
                tile.id,
                tile.climate.latitude
            );
            assert!(
                tile.climate.base_temperature > 200.0,
                "Tile {} temperature too low: {}",
                tile.id,
                tile.climate.base_temperature
            );
            assert!(
                tile.weather.temperature > 200.0,
                "Tile {} weather temp too low: {}",
                tile.id,
                tile.weather.temperature
            );
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let params = default_params();
        let world1 = generate_world(&params);
        let world2 = generate_world(&params);

        assert_eq!(world1.tiles.len(), world2.tiles.len());
        for (t1, t2) in world1.tiles.iter().zip(world2.tiles.iter()) {
            assert_eq!(t1.geology, t2.geology, "Geology mismatch at tile {}", t1.id);
            assert_eq!(t1.climate, t2.climate, "Climate mismatch at tile {}", t1.id);
            assert_eq!(t1.biome, t2.biome, "Biome mismatch at tile {}", t1.id);
            assert_eq!(
                t1.resources, t2.resources,
                "Resources mismatch at tile {}",
                t1.id
            );
            assert_eq!(
                t1.weather, t2.weather,
                "Weather mismatch at tile {}",
                t1.id
            );
            assert_eq!(
                t1.conditions, t2.conditions,
                "Conditions mismatch at tile {}",
                t1.id
            );
        }
    }

    #[test]
    fn custom_params_ocean_ratio() {
        let mut params = default_params();
        params.ocean_ratio = 0.3;
        let world = generate_world(&params);
        let ocean_count = world
            .tiles
            .iter()
            .filter(|t| t.geology.terrain_type == TerrainType::Ocean)
            .count();
        let ocean_pct = ocean_count as f32 / world.tiles.len() as f32;
        assert!(
            ocean_pct < 0.5,
            "Expected ~30% ocean, got {:.1}%",
            ocean_pct * 100.0
        );
    }

    #[test]
    fn custom_params_mountain_ratio() {
        let mut params = default_params();
        params.mountain_ratio = 0.3;
        let world = generate_world(&params);
        let mountain_count = world
            .tiles
            .iter()
            .filter(|t| t.geology.terrain_type == TerrainType::Mountains)
            .count();
        let land_count = world
            .tiles
            .iter()
            .filter(|t| t.geology.terrain_type != TerrainType::Ocean)
            .count();
        if land_count > 0 {
            let mountain_pct = mountain_count as f32 / land_count as f32;
            assert!(
                mountain_pct > 0.1,
                "Expected significant mountains with ratio 0.3, got {:.1}%",
                mountain_pct * 100.0
            );
        }
    }

    #[test]
    fn min_world_100_tiles() {
        let mut params = default_params();
        params.tile_count = 100;
        let world = generate_world(&params);
        assert!(world.tiles.len() >= 100);
    }

    #[test]
    fn all_ocean_world() {
        let mut params = default_params();
        params.ocean_ratio = 1.0;
        let world = generate_world(&params);
        let ocean_count = world
            .tiles
            .iter()
            .filter(|t| t.geology.terrain_type == TerrainType::Ocean)
            .count();
        assert!(
            ocean_count > world.tiles.len() * 90 / 100,
            "Expected >90% ocean, got {} of {}",
            ocean_count,
            world.tiles.len()
        );
    }

    #[test]
    fn no_ocean_world() {
        let mut params = default_params();
        params.ocean_ratio = 0.0;
        let world = generate_world(&params);
        let ocean_count = world
            .tiles
            .iter()
            .filter(|t| t.geology.terrain_type == TerrainType::Ocean)
            .count();
        assert_eq!(ocean_count, 0, "Expected no ocean, got {}", ocean_count);
    }

    #[test]
    fn climate_follows_latitude() {
        let params = default_params();
        let world = generate_world(&params);

        let mut polar_found = false;
        let mut tropical_found = false;

        for tile in &world.tiles {
            if tile.climate.latitude.abs() > 60.0 {
                assert_eq!(
                    tile.climate.zone,
                    ClimateZone::Polar,
                    "Tile at lat {:.1} should be Polar",
                    tile.climate.latitude
                );
                polar_found = true;
            }
            if tile.climate.latitude.abs() < 15.0 {
                assert_eq!(
                    tile.climate.zone,
                    ClimateZone::Tropical,
                    "Tile at lat {:.1} should be Tropical",
                    tile.climate.latitude
                );
                tropical_found = true;
            }
        }
        assert!(polar_found, "Should have polar tiles");
        assert!(tropical_found, "Should have tropical tiles");
    }

    #[test]
    fn resources_distributed() {
        let params = default_params();
        let world = generate_world(&params);
        let total_resources: usize = world
            .tiles
            .iter()
            .map(|t| t.resources.resources.len())
            .sum();
        assert!(total_resources > 0, "Expected some resources");
    }

    #[test]
    fn seed_zero_generates_random() {
        let mut params = default_params();
        params.seed = 0;
        let world = generate_world(&params);
        // The resolved seed should be non-zero (stored in generation_params)
        assert_ne!(
            world.generation_params.seed, 0,
            "Resolved seed should be non-zero"
        );
    }
}
