use rayon::prelude::*;
use tracing::warn;

use crate::simulation::engine::{
    apply_mutations, tile_to_rhai_map, Phase, RuleEngine, RuleError, TileMutations,
};
use crate::world::tile::BiomeType;
use crate::world::World;
use rhai::Dynamic;

/// Execute a single phase across all tiles using double buffering and parallel evaluation.
///
/// Reads from a snapshot of current tile state (so all tiles in this phase
/// see the same input), evaluates tiles in parallel via rayon, then writes
/// mutations to the live tiles sequentially.
/// Pre-converts all tiles to Rhai maps once per phase to avoid redundant conversions.
pub fn execute_phase(
    world: &mut World,
    engine: &RuleEngine,
    phase: Phase,
) -> Vec<RuleError> {
    let rules = engine.rules_for_phase(phase);
    if rules.is_empty() {
        return Vec::new();
    }

    // Double buffer: snapshot current state for reading
    let input_tiles = world.tiles.clone();

    // Pre-convert all tiles to Rhai maps once (avoids re-converting neighbors repeatedly)
    let tile_maps: Vec<Dynamic> = input_tiles.iter().map(|t| tile_to_rhai_map(t)).collect();

    // Capture values needed by the parallel closure (avoids borrowing `world` across par_iter)
    let tick_count = world.tick_count;
    let season = world.season;

    // Parallel evaluation: each tile is independently evaluated by a rayon worker thread.
    // Thread-local MUTATIONS and RNG_STATE in engine.rs are per-worker, so this is safe.
    let results: Vec<(usize, Result<TileMutations, RuleError>)> = (0..input_tiles.len())
        .into_par_iter()
        .map(|i| {
            let tile = &input_tiles[i];

            // Gather pre-converted neighbor maps
            let neighbor_maps: Vec<Dynamic> = tile
                .neighbors
                .iter()
                .filter_map(|&nid| tile_maps.get(nid as usize).cloned())
                .collect();

            let rng_seed = compute_rng_seed(tick_count, tile.id, phase);

            let result = engine.evaluate_tile_preconverted(
                phase,
                &tile_maps[i],
                neighbor_maps,
                &season,
                tick_count,
                rng_seed,
                tile.id,
            );

            (i, result)
        })
        .collect();

    // Sequential: apply mutations to live tiles
    let mut errors = Vec::new();
    for (i, result) in results {
        match result {
            Ok(mutations) => {
                let mutations = if phase == Phase::Terrain {
                    filter_invalid_biome_transitions(&input_tiles[i], mutations)
                } else {
                    mutations
                };
                apply_mutations(&mut world.tiles[i], &mutations, phase);
            }
            Err(err) => {
                errors.push(err);
            }
        }
    }

    errors
}

/// Compute a deterministic RNG seed for a tile evaluation.
fn compute_rng_seed(tick: u64, tile_id: u32, phase: Phase) -> u64 {
    let phase_offset: u64 = match phase {
        Phase::Weather => 0,
        Phase::Conditions => 1,
        Phase::Terrain => 2,
        Phase::Resources => 3,
    };
    tick.wrapping_mul(6364136223846793005)
        .wrapping_add(tile_id as u64)
        .wrapping_mul(1442695040888963407)
        .wrapping_add(phase_offset)
}

/// Valid biome transitions — adjacent biomes on the moisture/temperature gradient.
/// Ocean cannot transition. Land biomes transition only to adjacent types.
pub fn valid_transitions(biome: BiomeType) -> &'static [BiomeType] {
    match biome {
        BiomeType::Ocean => &[],
        BiomeType::Ice => &[BiomeType::Tundra],
        BiomeType::Tundra => &[BiomeType::Ice, BiomeType::BorealForest],
        BiomeType::BorealForest => &[BiomeType::Tundra, BiomeType::TemperateForest],
        BiomeType::TemperateForest => &[
            BiomeType::BorealForest,
            BiomeType::Grassland,
            BiomeType::TropicalForest,
        ],
        BiomeType::Grassland => &[
            BiomeType::TemperateForest,
            BiomeType::Savanna,
            BiomeType::Wetland,
        ],
        BiomeType::Savanna => &[BiomeType::Grassland, BiomeType::Desert, BiomeType::TropicalForest],
        BiomeType::Desert => &[BiomeType::Savanna, BiomeType::Barren],
        BiomeType::TropicalForest => &[BiomeType::TemperateForest, BiomeType::Savanna],
        BiomeType::Wetland => &[BiomeType::Grassland],
        BiomeType::Barren => &[BiomeType::Desert],
    }
}

/// Filter out invalid biome transitions from mutations.
/// Invalid transitions are logged as warnings and removed.
fn filter_invalid_biome_transitions(
    current_tile: &crate::world::Tile,
    mut mutations: TileMutations,
) -> TileMutations {
    let current_biome = current_tile.biome.biome_type;
    let valid = valid_transitions(current_biome);

    mutations.mutations.retain(|(field, value)| {
        if field != "biome_type" {
            return true;
        }
        if let Ok(s) = value.clone().into_string() {
            if let Some(target) = parse_biome_type(&s) {
                if target == current_biome {
                    return true; // No-op, keep it
                }
                if !valid.contains(&target) {
                    warn!(
                        tile_id = current_tile.id,
                        from = ?current_biome,
                        to = ?target,
                        "Invalid biome transition rejected"
                    );
                    return false;
                }
            }
        }
        true
    });

    mutations
}

fn parse_biome_type(s: &str) -> Option<BiomeType> {
    match s {
        "Ocean" => Some(BiomeType::Ocean),
        "Ice" => Some(BiomeType::Ice),
        "Tundra" => Some(BiomeType::Tundra),
        "BorealForest" => Some(BiomeType::BorealForest),
        "TemperateForest" => Some(BiomeType::TemperateForest),
        "Grassland" => Some(BiomeType::Grassland),
        "Savanna" => Some(BiomeType::Savanna),
        "Desert" => Some(BiomeType::Desert),
        "TropicalForest" => Some(BiomeType::TropicalForest),
        "Wetland" => Some(BiomeType::Wetland),
        "Barren" => Some(BiomeType::Barren),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::engine::{Phase, RuleEngine, TileMutations};
    use crate::world::tile::*;
    use rhai::Dynamic;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn make_test_tile(id: u32) -> crate::world::Tile {
        crate::world::Tile::new_default(id, vec![], Position { x: 0.0, y: 0.0 })
    }

    fn setup_empty_rule_dirs(dir: &Path) {
        for phase in Phase::all() {
            fs::create_dir_all(dir.join(phase.dir_name())).unwrap();
        }
    }

    fn make_rule_dir(dir: &Path, phase: &str, rules: &[(&str, &str)]) {
        let phase_dir = dir.join(phase);
        fs::create_dir_all(&phase_dir).unwrap();
        for (name, content) in rules {
            fs::write(phase_dir.join(name), content).unwrap();
        }
    }

    #[test]
    fn empty_phase_is_noop() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());

        let engine = RuleEngine::new(dir.path(), 100).unwrap();
        let mut world = crate::world::World {
            id: uuid::Uuid::new_v4(),
            name: "test".to_string(),
            created_at: "2026-01-01".to_string(),
            tick_count: 0,
            season: Season::Spring,
            season_length: 100,
            tile_count: 2,
            topology_type: TopologyType::FlatHex,
            generation_params: crate::config::generation::GenerationParams {
                seed: 42,
                tile_count: 100,
                ocean_ratio: 0.6,
                mountain_ratio: 0.1,
                elevation_roughness: 0.5,
                climate_bands: true,
                resource_density: 0.3,
                initial_biome_maturity: 0.5,
            },
            snapshot_path: None,
            tiles: vec![make_test_tile(0), make_test_tile(1)],
        };

        let original = world.tiles.clone();
        let errors = execute_phase(&mut world, &engine, Phase::Weather);

        assert!(errors.is_empty());
        assert_eq!(world.tiles, original);
    }

    #[test]
    fn double_buffer_within_phase() {
        // Within a phase, tiles should read from the snapshot, not live data.
        // If tile 0 sets temperature to 999, tile 1 should NOT see 999 when reading
        // tile 0 as a neighbor (it should see the pre-phase value).
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[(
                "01-avg-neighbor.rhai",
                r#"
                // Set our temperature to average of neighbors
                let sum = 0.0;
                for n in neighbors {
                    sum += n.weather.temperature;
                }
                if neighbors.len() > 0 {
                    set("temperature", sum / neighbors.len());
                } else {
                    set("temperature", 999.0);
                }
                "#,
            )],
        );

        let engine = RuleEngine::new(dir.path(), 100).unwrap();
        let mut world = crate::world::World {
            id: uuid::Uuid::new_v4(),
            name: "test".to_string(),
            created_at: "2026-01-01".to_string(),
            tick_count: 0,
            season: Season::Spring,
            season_length: 100,
            tile_count: 2,
            topology_type: TopologyType::FlatHex,
            generation_params: crate::config::generation::GenerationParams {
                seed: 42,
                tile_count: 100,
                ocean_ratio: 0.6,
                mountain_ratio: 0.1,
                elevation_roughness: 0.5,
                climate_bands: true,
                resource_density: 0.3,
                initial_biome_maturity: 0.5,
            },
            snapshot_path: None,
            tiles: vec![
                {
                    let mut t = make_test_tile(0);
                    t.neighbors = vec![1];
                    t.weather.temperature = 280.0;
                    t
                },
                {
                    let mut t = make_test_tile(1);
                    t.neighbors = vec![0];
                    t.weather.temperature = 300.0;
                    t
                },
            ],
        };

        execute_phase(&mut world, &engine, Phase::Weather);

        // Tile 0 should see neighbor (tile 1) at 300.0 (pre-phase value)
        assert!((world.tiles[0].weather.temperature - 300.0).abs() < 0.01);
        // Tile 1 should see neighbor (tile 0) at 280.0 (pre-phase value, NOT the 300.0 that tile 0 was updated to)
        assert!((world.tiles[1].weather.temperature - 280.0).abs() < 0.01);
    }

    #[test]
    fn invalid_biome_transition_tundra_to_desert_rejected() {
        let tile = {
            let mut t = make_test_tile(0);
            t.biome.biome_type = BiomeType::Tundra;
            t
        };

        let mutations = TileMutations {
            mutations: vec![("biome_type".to_string(), Dynamic::from("Desert".to_string()))],
        };

        let filtered = filter_invalid_biome_transitions(&tile, mutations);
        // The biome_type mutation should have been removed
        assert!(
            !filtered
                .mutations
                .iter()
                .any(|(f, _)| f == "biome_type"),
            "Tundra → Desert should be rejected"
        );
    }

    #[test]
    fn valid_biome_transition_grassland_to_savanna_accepted() {
        let tile = {
            let mut t = make_test_tile(0);
            t.biome.biome_type = BiomeType::Grassland;
            t
        };

        let mutations = TileMutations {
            mutations: vec![(
                "biome_type".to_string(),
                Dynamic::from("Savanna".to_string()),
            )],
        };

        let filtered = filter_invalid_biome_transitions(&tile, mutations);
        assert!(
            filtered
                .mutations
                .iter()
                .any(|(f, _)| f == "biome_type"),
            "Grassland → Savanna should be accepted"
        );
    }

    #[test]
    fn ocean_biome_transition_always_rejected() {
        let tile = {
            let mut t = make_test_tile(0);
            t.biome.biome_type = BiomeType::Ocean;
            t
        };

        let mutations = TileMutations {
            mutations: vec![(
                "biome_type".to_string(),
                Dynamic::from("Grassland".to_string()),
            )],
        };

        let filtered = filter_invalid_biome_transitions(&tile, mutations);
        assert!(
            !filtered
                .mutations
                .iter()
                .any(|(f, _)| f == "biome_type"),
            "Ocean cannot transition to land biomes"
        );
    }

    #[test]
    fn non_biome_mutations_preserved_when_biome_rejected() {
        let tile = {
            let mut t = make_test_tile(0);
            t.biome.biome_type = BiomeType::Tundra;
            t
        };

        let mutations = TileMutations {
            mutations: vec![
                (
                    "vegetation_health".to_string(),
                    Dynamic::from(0.5_f64),
                ),
                (
                    "biome_type".to_string(),
                    Dynamic::from("Desert".to_string()),
                ),
                (
                    "transition_pressure".to_string(),
                    Dynamic::from(0.0_f64),
                ),
            ],
        };

        let filtered = filter_invalid_biome_transitions(&tile, mutations);
        // biome_type removed, but other mutations preserved
        assert_eq!(filtered.mutations.len(), 2);
        assert!(filtered.mutations.iter().any(|(f, _)| f == "vegetation_health"));
        assert!(filtered.mutations.iter().any(|(f, _)| f == "transition_pressure"));
    }

    #[test]
    fn biome_adjacency_graph_is_bidirectional() {
        // Every biome that A can transition to should also list A as a valid source
        let all_biomes = [
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

        for &biome in &all_biomes {
            for &target in valid_transitions(biome) {
                assert!(
                    valid_transitions(target).contains(&biome),
                    "{:?} -> {:?} exists but {:?} -> {:?} does not (adjacency must be bidirectional)",
                    biome, target, target, biome
                );
            }
        }
    }
}
