use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::config::generation::GenerationParams;
use crate::config::simulation::SimulationConfig;
use crate::persistence;
use crate::server::{self, ServerState};
use crate::simulation;
use crate::simulation::engine::RuleEngine;
use crate::world::generation::generate_world;
use crate::world::tile::{WeatherLayer, ConditionsLayer, BiomeLayer, ResourceLayer};
use crate::world::World;

/// How the simulation should obtain its initial world.
pub enum WorldSource {
    /// Load a specific snapshot file.
    Snapshot(String),
    /// Generate a fresh world from a worldgen config file.
    Generate(String),
}

/// Run the simulation: load world, start WebSocket server, run tick loop.
pub async fn run_simulation(
    config: &SimulationConfig,
    source: WorldSource,
) -> Result<(), String> {
    // 1. Load or generate world
    let snapshot_dir = Path::new(&config.snapshot_directory);
    let mut world = match source {
        WorldSource::Snapshot(path) => {
            info!(path = %path, "Loading world from snapshot");
            persistence::load_snapshot(Path::new(&path))
                .map_err(|e| format!("Failed to load snapshot: {}", e))?
        }
        WorldSource::Generate(worldgen_path) => {
            let params = GenerationParams::from_file(Path::new(&worldgen_path))
                .map_err(|e| format!("Failed to load worldgen config: {}", e))?;
            info!(config = %worldgen_path, "Generating fresh world");
            generate_world(&params)
        }
    };

    info!(
        tiles = world.tiles.len(),
        tick = world.tick_count,
        season = ?world.season,
        "World loaded"
    );

    // 2. Load rules
    let rule_dir = Path::new(&config.rule_directory);
    let engine = RuleEngine::new(rule_dir, config.rule_timeout_ms as u64)
        .map_err(|e| format!("Failed to load rules: {}", e))?;
    info!(dir = %config.rule_directory, "Rules loaded");

    // 3. Build initial snapshot JSON and create server state
    let snapshot_json = server::build_snapshot_json(&world);
    let state = Arc::new(ServerState::new(snapshot_json));

    // 4. Start WebSocket server in background
    let addr: SocketAddr = format!("{}:{}", config.websocket_bind, config.websocket_port)
        .parse()
        .map_err(|e| format!("Invalid bind address: {}", e))?;

    let server_state = Arc::clone(&state);
    tokio::spawn(async move {
        if let Err(e) = server::start_server(server_state, addr).await {
            error!("Server error: {}", e);
        }
    });

    // 5. Set up shutdown signal
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    // 6. Run tick loop
    let tick_interval_ms = (1000.0 / config.tick_rate_hz) as u64;
    let mut last_snapshot_tick = world.tick_count;
    let mut ticks_since_snapshot: u32 = 0;

    info!(
        tick_rate_hz = config.tick_rate_hz,
        snapshot_interval = config.snapshot_interval,
        "Simulation running"
    );

    loop {
        let tick_start = std::time::Instant::now();

        // Lightweight snapshot: only capture mutable layers for diff computation
        let before_layers: Vec<(WeatherLayer, ConditionsLayer, BiomeLayer, ResourceLayer)> =
            world.tiles.iter().map(|t| {
                (t.weather.clone(), t.conditions.clone(), t.biome.clone(), t.resources.clone())
            }).collect();

        // Execute tick
        let result = simulation::execute_tick(&mut world, &engine, config.season_length);

        // Build diff from lightweight layer snapshots (avoids full tile clone)
        let diff_json = server::build_diff_json_from_layers(
            &before_layers,
            &world.tiles,
            world.tick_count,
            world.season,
            &result.statistics,
        );

        // Rebuild full snapshot JSON periodically (every 10 ticks) instead of every tick.
        // This avoids serializing all tiles to JSON on every tick at large tile counts.
        let new_snapshot_json = if world.tick_count % 10 == 0 {
            Some(server::build_snapshot_json(&world))
        } else {
            None
        };

        // Update server state (broadcasts diff to clients)
        state
            .on_tick(
                new_snapshot_json,
                diff_json,
                &result.statistics,
                world.tick_count,
                world.season,
                world.tile_count,
                last_snapshot_tick,
            )
            .await;

        // Log errors
        if !result.rule_errors.is_empty() {
            warn!(
                tick = world.tick_count,
                error_count = result.rule_errors.len(),
                "Rule errors in tick"
            );
        }

        // Periodic auto-save
        ticks_since_snapshot += 1;
        if ticks_since_snapshot >= config.snapshot_interval {
            match persistence::save_snapshot(&world, snapshot_dir) {
                Ok(path) => {
                    last_snapshot_tick = world.tick_count;
                    ticks_since_snapshot = 0;
                    info!(path = %path.display(), "Snapshot saved");

                    // Prune old snapshots
                    if let Err(e) =
                        persistence::prune_snapshots(snapshot_dir, config.max_snapshots as usize)
                    {
                        warn!("Snapshot pruning failed: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Snapshot save failed: {}", e);
                }
            }
        }

        // Tick milestone logging
        if world.tick_count % 1000 == 0 {
            info!(
                tick = world.tick_count,
                season = ?world.season,
                diversity = result.statistics.diversity_index,
                rule_errors = result.statistics.rule_errors,
                "Tick milestone"
            );
        }

        // Rate limiting: sleep remaining time to hit target tick rate
        let elapsed = tick_start.elapsed();
        let target = std::time::Duration::from_millis(tick_interval_ms);
        if elapsed < target {
            let sleep_duration = target - elapsed;
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {}
                _ = &mut shutdown => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        } else {
            // Tick took longer than target — yield to tokio runtime and check for shutdown
            tokio::select! {
                biased;
                _ = &mut shutdown => {
                    info!("Shutdown signal received");
                    break;
                }
                _ = tokio::task::yield_now() => {}
            }
        }
    }

    // Graceful shutdown: save final snapshot
    info!("Saving final snapshot...");
    match persistence::save_snapshot(&world, snapshot_dir) {
        Ok(path) => info!(path = %path.display(), "Final snapshot saved"),
        Err(e) => warn!("Final snapshot save failed: {}", e),
    }

    info!(tick = world.tick_count, "Simulation stopped");
    Ok(())
}

/// Inspect a tile or world summary from the latest snapshot.
pub fn inspect(
    config: &SimulationConfig,
    tile_id: Option<u32>,
    show_world: bool,
) -> Result<(), String> {
    let snapshot_dir = Path::new(&config.snapshot_directory);
    let world = persistence::load_latest_valid_snapshot(snapshot_dir)
        .map_err(|e| format!("Failed to load snapshot: {}", e))?;

    if let Some(id) = tile_id {
        inspect_tile(&world, id)
    } else if show_world {
        inspect_world(&world);
        Ok(())
    } else {
        Err("Specify --tile <ID> or --world".to_string())
    }
}

fn inspect_tile(world: &World, tile_id: u32) -> Result<(), String> {
    let tile = world
        .tiles
        .get(tile_id as usize)
        .ok_or_else(|| format!("Tile {} not found (world has {} tiles)", tile_id, world.tiles.len()))?;

    if tile.id != tile_id {
        return Err(format!(
            "Tile at index {} has id {} (expected {})",
            tile_id, tile.id, tile_id
        ));
    }

    println!("=== Tile {} ===", tile.id);
    println!("Neighbors: {:?}", tile.neighbors);
    println!("Position: ({:.2}, {:.2})", tile.position.x, tile.position.y);
    println!();
    println!("--- Geology ---");
    println!("  Terrain: {:?}", tile.geology.terrain_type);
    println!("  Elevation: {:.3}", tile.geology.elevation);
    println!("  Soil: {:?}", tile.geology.soil_type);
    println!("  Drainage: {:.3}", tile.geology.drainage);
    println!();
    println!("--- Climate ---");
    println!("  Zone: {:?}", tile.climate.zone);
    println!("  Base temp: {:.1}K ({:.1}°C)", tile.climate.base_temperature, tile.climate.base_temperature - 273.15);
    println!("  Latitude: {:.1}°", tile.climate.latitude);
    println!();
    println!("--- Biome ---");
    println!("  Type: {:?}", tile.biome.biome_type);
    println!("  Vegetation density: {:.3}", tile.biome.vegetation_density);
    println!("  Vegetation health: {:.3}", tile.biome.vegetation_health);
    println!("  Transition pressure: {:.3}", tile.biome.transition_pressure);
    println!("  Ticks in current biome: {}", tile.biome.ticks_in_current_biome);
    println!();
    println!("--- Weather ---");
    println!("  Temperature: {:.1}K ({:.1}°C)", tile.weather.temperature, tile.weather.temperature - 273.15);
    println!("  Precipitation: {:.3} ({:?})", tile.weather.precipitation, tile.weather.precipitation_type);
    println!("  Wind: {:.1} @ {:.0}°", tile.weather.wind_speed, tile.weather.wind_direction);
    println!("  Cloud cover: {:.3}", tile.weather.cloud_cover);
    println!("  Storm intensity: {:.3}", tile.weather.storm_intensity);
    println!();
    println!("--- Conditions ---");
    println!("  Soil moisture: {:.3}", tile.conditions.soil_moisture);
    println!("  Snow depth: {:.3}", tile.conditions.snow_depth);
    println!("  Mud level: {:.3}", tile.conditions.mud_level);
    println!("  Flood level: {:.3}", tile.conditions.flood_level);
    println!("  Frost days: {}", tile.conditions.frost_days);
    println!("  Drought days: {}", tile.conditions.drought_days);
    println!("  Fire risk: {:.3}", tile.conditions.fire_risk);
    println!();
    println!("--- Resources ---");
    if tile.resources.resources.is_empty() {
        println!("  (none)");
    } else {
        for r in &tile.resources.resources {
            println!(
                "  {}: {:.1}/{:.1} (renewal: {:.2}/tick)",
                r.resource_type, r.quantity, r.max_quantity, r.renewal_rate
            );
        }
    }

    Ok(())
}

fn inspect_world(world: &World) {
    use std::collections::HashMap;

    println!("=== World: {} ===", world.name);
    println!("ID: {}", world.id);
    println!("Tick: {}", world.tick_count);
    println!("Season: {:?}", world.season);
    println!("Tiles: {}", world.tiles.len());
    println!("Topology: {:?}", world.topology_type);
    println!();

    // Biome distribution
    let mut biome_counts: HashMap<_, u32> = HashMap::new();
    let mut total_temp = 0.0_f64;
    let mut total_moisture = 0.0_f64;
    let mut total_veg_health = 0.0_f64;

    for tile in &world.tiles {
        *biome_counts.entry(tile.biome.biome_type).or_default() += 1;
        total_temp += tile.weather.temperature as f64;
        total_moisture += tile.conditions.soil_moisture as f64;
        total_veg_health += tile.biome.vegetation_health as f64;
    }

    let n = world.tiles.len() as f64;
    println!("--- Averages ---");
    println!("  Temperature: {:.1}K ({:.1}°C)", total_temp / n, total_temp / n - 273.15);
    println!("  Soil moisture: {:.3}", total_moisture / n);
    println!("  Vegetation health: {:.3}", total_veg_health / n);
    println!();

    println!("--- Biome Distribution ---");
    let mut sorted: Vec<_> = biome_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for (biome, count) in &sorted {
        let pct = (*count as f64 / n) * 100.0;
        println!("  {:?}: {} ({:.1}%)", biome, count, pct);
    }
}
