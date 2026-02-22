/// Macro weather system: pressure systems as first-class entities.
///
/// Evolves pressure systems (spawn, move, intensify, decay, merge) and
/// projects their effects (pressure, wind, humidity) onto tiles.

use rayon::prelude::*;

use crate::simulation::sphere_math;
use crate::world::tile::TerrainType;
use crate::world::weather_systems::{PressureSystem, PressureSystemType};
use crate::world::World;

/// Spatial grid for fast nearest-tile lookup (~10-degree resolution).
/// Bins tiles by lat/lon to avoid O(N) linear scan in intensify_decay.
struct SpatialGrid {
    /// 18 lat bins x 36 lon bins = 648 cells
    cells: Vec<Vec<usize>>,
    lat_bins: usize,
    lon_bins: usize,
}

impl SpatialGrid {
    fn new(tiles: &[(f64, f64, TerrainType, f32)]) -> Self {
        let lat_bins = 18; // 10-degree resolution: -90..90
        let lon_bins = 36; // 10-degree resolution: -180..180
        let mut cells = vec![Vec::new(); lat_bins * lon_bins];

        for (i, &(lat, lon, _, _)) in tiles.iter().enumerate() {
            let cell = Self::cell_index_static(lat, lon, lat_bins, lon_bins);
            cells[cell].push(i);
        }

        SpatialGrid { cells, lat_bins, lon_bins }
    }

    fn cell_index_static(lat: f64, lon: f64, lat_bins: usize, lon_bins: usize) -> usize {
        let lat_bin = ((lat + 90.0) / 180.0 * lat_bins as f64).floor() as usize;
        let lon_bin = ((lon + 180.0) / 360.0 * lon_bins as f64).floor() as usize;
        let lat_bin = lat_bin.min(lat_bins - 1);
        let lon_bin = lon_bin.min(lon_bins - 1);
        lat_bin * lon_bins + lon_bin
    }

    /// Find nearest tile to (lat, lon) by checking the target bin + 8 neighbors.
    fn find_nearest<'a>(
        &self,
        lat: f64,
        lon: f64,
        tiles: &'a [(f64, f64, TerrainType, f32)],
    ) -> (TerrainType, f32) {
        let lat_bin = ((lat + 90.0) / 180.0 * self.lat_bins as f64).floor() as isize;
        let lon_bin = ((lon + 180.0) / 360.0 * self.lon_bins as f64).floor() as isize;

        let mut min_dist = f64::MAX;
        let mut nearest_terrain = TerrainType::Ocean;
        let mut nearest_temp = 288.0_f32;

        // Check 3x3 neighborhood (target + 8 neighbors)
        for dlat in -1..=1 {
            for dlon in -1..=1 {
                let r = lat_bin + dlat;
                let c = lon_bin + dlon;

                // Lat clamping (poles)
                if r < 0 || r >= self.lat_bins as isize {
                    continue;
                }
                // Lon wrapping
                let c = ((c % self.lon_bins as isize) + self.lon_bins as isize) as usize % self.lon_bins;
                let r = r as usize;

                let cell_idx = r * self.lon_bins + c;
                for &tile_idx in &self.cells[cell_idx] {
                    let (tlat, tlon, terrain, temp) = tiles[tile_idx];
                    let dist = sphere_math::angular_distance(lat, lon, tlat, tlon);
                    if dist < min_dist {
                        min_dist = dist;
                        nearest_terrain = terrain;
                        nearest_temp = temp;
                    }
                }
            }
        }

        (nearest_terrain, nearest_temp)
    }
}

/// Simple xorshift64 PRNG for deterministic macro weather.
fn xorshift64(state: &mut u64) -> u64 {
    if *state == 0 {
        *state = 1;
    }
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Returns a deterministic f64 in [0, 1) from the RNG state.
fn rand_f64(state: &mut u64) -> f64 {
    xorshift64(state) as f64 / u64::MAX as f64
}

/// Returns a deterministic f64 in [min, max) from the RNG state.
fn rand_range(state: &mut u64, min: f64, max: f64) -> f64 {
    min + rand_f64(state) * (max - min)
}

/// Run the full macro weather step: evolve systems, then project onto tiles.
pub fn macro_weather_step(world: &mut World) {
    evolve_systems(world);
    project_macro_to_tiles(world);
}

/// Evolve pressure systems: spawn new ones, move existing, intensify/decay, merge.
fn evolve_systems(world: &mut World) {
    let tile_count = world.tiles.len();
    let max_systems = (tile_count / 100).max(5).min(80);

    // === SPAWN ===
    if world.macro_weather.systems.len() < max_systems {
        // Attempt spawns based on world conditions
        spawn_systems(world, max_systems);
    }

    // === MOVE ===
    for system in &mut world.macro_weather.systems {
        move_system(system);
    }

    // === INTENSIFY / DECAY ===
    let tiles_snapshot: Vec<(f64, f64, TerrainType, f32)> = world
        .tiles
        .iter()
        .map(|t| (t.position.lat, t.position.lon, t.geology.terrain_type, t.climate.base_temperature))
        .collect();

    let grid = SpatialGrid::new(&tiles_snapshot);

    let rng = &mut world.macro_weather.rng_state;
    for system in &mut world.macro_weather.systems {
        intensify_decay(system, &tiles_snapshot, &grid, rng);
    }

    // === MERGE ===
    merge_systems(&mut world.macro_weather.systems);

    // === REMOVE DEAD ===
    world.macro_weather.systems.retain(|s| s.pressure_anomaly.abs() >= 2.0 && s.age <= s.max_age);
}

fn spawn_systems(world: &mut World, max_systems: usize) {
    let rng = &mut world.macro_weather.rng_state;
    let current_count = world.macro_weather.systems.len();
    if current_count >= max_systems {
        return;
    }

    // Spawn probability per tick: roughly 1 system every 5-10 ticks
    let spawn_chance = 0.15;
    if rand_f64(rng) > spawn_chance {
        return;
    }

    // Pick a random tile to seed a system near
    let tile_idx = (xorshift64(rng) as usize) % world.tiles.len();
    let tile = &world.tiles[tile_idx];
    let lat = tile.position.lat;
    let lon = tile.position.lon;
    let abs_lat = lat.abs();
    let terrain = tile.geology.terrain_type;
    let base_temp = tile.climate.base_temperature;

    // Determine what kind of system can spawn here
    let system_type = if abs_lat > 60.0 && terrain != TerrainType::Ocean {
        // Polar high over land at high latitudes
        Some(PressureSystemType::PolarHigh)
    } else if abs_lat > 40.0 && abs_lat < 65.0 {
        // Mid-latitude cyclone at polar front
        if rand_f64(rng) < 0.6 {
            Some(PressureSystemType::MidLatCyclone)
        } else {
            None
        }
    } else if abs_lat > 20.0 && abs_lat < 40.0 && terrain == TerrainType::Ocean {
        // Subtropical high over ocean
        if rand_f64(rng) < 0.3 {
            Some(PressureSystemType::SubtropicalHigh)
        } else {
            None
        }
    } else if abs_lat < 25.0 && terrain == TerrainType::Ocean && base_temp > 299.0 {
        // Tropical low over warm ocean
        if rand_f64(rng) < 0.2 {
            Some(PressureSystemType::TropicalLow)
        } else {
            None
        }
    } else if abs_lat < 35.0 && terrain != TerrainType::Ocean && base_temp > 295.0 {
        // Thermal low over hot continental interiors
        if rand_f64(rng) < 0.25 {
            Some(PressureSystemType::ThermalLow)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(st) = system_type {
        let (pressure_anomaly, radius, max_age, moisture) = match st {
            PressureSystemType::MidLatCyclone => (
                rand_range(rng, -20.0, -8.0) as f32,
                rand_range(rng, 0.15, 0.35) as f32,
                (rand_range(rng, 80.0, 200.0)) as u32,
                rand_range(rng, 0.4, 0.8) as f32,
            ),
            PressureSystemType::SubtropicalHigh => (
                rand_range(rng, 8.0, 18.0) as f32,
                rand_range(rng, 0.25, 0.45) as f32,
                (rand_range(rng, 200.0, 500.0)) as u32,
                rand_range(rng, 0.1, 0.3) as f32,
            ),
            PressureSystemType::TropicalLow => (
                rand_range(rng, -25.0, -10.0) as f32,
                rand_range(rng, 0.1, 0.25) as f32,
                (rand_range(rng, 60.0, 150.0)) as u32,
                rand_range(rng, 0.6, 0.95) as f32,
            ),
            PressureSystemType::PolarHigh => (
                rand_range(rng, 10.0, 25.0) as f32,
                rand_range(rng, 0.2, 0.4) as f32,
                (rand_range(rng, 300.0, 600.0)) as u32,
                rand_range(rng, 0.05, 0.2) as f32,
            ),
            PressureSystemType::ThermalLow => (
                rand_range(rng, -12.0, -5.0) as f32,
                rand_range(rng, 0.1, 0.2) as f32,
                (rand_range(rng, 40.0, 100.0)) as u32,
                rand_range(rng, 0.1, 0.3) as f32,
            ),
        };

        let (x, y, z) = sphere_math::lat_lon_to_xyz(lat, lon);
        let id = world.macro_weather.next_id;
        world.macro_weather.next_id += 1;

        world.macro_weather.systems.push(PressureSystem {
            id,
            lat,
            lon,
            x,
            y,
            z,
            pressure_anomaly,
            radius,
            velocity_east: 0.0,
            velocity_north: 0.0,
            age: 0,
            max_age,
            system_type: st,
            moisture,
        });
    }
}

/// Move a pressure system based on its type and latitude.
fn move_system(system: &mut PressureSystem) {
    let abs_lat = system.lat.abs();

    // Steering flow by latitude band
    let (base_east, base_north) = match system.system_type {
        PressureSystemType::MidLatCyclone => {
            // Westerlies: eastward, speed ~ cos(lat)
            let speed = 0.008 * abs_lat.to_radians().cos() as f32;
            (speed, 0.001_f32) // slight poleward drift
        }
        PressureSystemType::SubtropicalHigh => {
            // Nearly stationary
            (0.0005_f32, 0.0_f32)
        }
        PressureSystemType::TropicalLow => {
            // Trade winds: westward
            (-0.005_f32, 0.001_f32) // westward + slight poleward
        }
        PressureSystemType::PolarHigh => {
            // Slow equatorward drift
            let drift = if system.lat > 0.0 { -0.001_f32 } else { 0.001_f32 };
            (0.001_f32, drift)
        }
        PressureSystemType::ThermalLow => {
            // Nearly stationary (tied to land heating)
            (0.0003_f32, 0.0_f32)
        }
    };

    // Blend current velocity toward steering flow
    system.velocity_east = system.velocity_east * 0.8 + base_east * 0.2;
    system.velocity_north = system.velocity_north * 0.8 + base_north * 0.2;

    // Advance position using Rodrigues' rotation
    let (new_lat, new_lon) = sphere_math::advance_position(
        system.lat,
        system.lon,
        system.velocity_east as f64,
        system.velocity_north as f64,
        1.0,
    );

    system.lat = new_lat;
    system.lon = new_lon;
    let (x, y, z) = sphere_math::lat_lon_to_xyz(new_lat, new_lon);
    system.x = x;
    system.y = y;
    system.z = z;
    system.age += 1;
}

/// Intensify or decay a system based on underlying surface conditions.
fn intensify_decay(
    system: &mut PressureSystem,
    tiles: &[(f64, f64, TerrainType, f32)],
    grid: &SpatialGrid,
    rng: &mut u64,
) {
    // Find the nearest tile via spatial grid (O(1) amortized vs O(N) linear scan)
    let (nearest_terrain, nearest_temp) = grid.find_nearest(system.lat, system.lon, tiles);

    let over_ocean = nearest_terrain == TerrainType::Ocean;
    let warm_ocean = over_ocean && nearest_temp > 299.0;

    // Intensification/weakening based on surface
    let surface_factor = match system.system_type {
        PressureSystemType::MidLatCyclone => {
            if over_ocean { 1.02 } else { 0.97 } // weaken over land (friction)
        }
        PressureSystemType::TropicalLow => {
            if warm_ocean { 1.04 } else if over_ocean { 1.0 } else { 0.92 }
        }
        PressureSystemType::SubtropicalHigh | PressureSystemType::PolarHigh => {
            1.0 // stable
        }
        PressureSystemType::ThermalLow => {
            if !over_ocean && nearest_temp > 295.0 { 1.01 } else { 0.95 }
        }
    };

    // Age decay: intensity fades as system ages
    let age_factor = 1.0 - (system.age as f32 / system.max_age as f32) * 0.02;

    system.pressure_anomaly *= surface_factor as f32 * age_factor;

    // Small random perturbation
    system.pressure_anomaly += rand_range(rng, -0.5, 0.5) as f32;

    // Moisture update: slower land loss lets systems carry moisture deeper inland
    if over_ocean {
        system.moisture = (system.moisture + 0.012).min(1.0);
    } else {
        system.moisture = (system.moisture - 0.002).max(0.0);
    }
}

/// Merge same-type systems that are too close together.
fn merge_systems(systems: &mut Vec<PressureSystem>) {
    let mut to_remove: Vec<u32> = Vec::new();

    let len = systems.len();
    for i in 0..len {
        if to_remove.contains(&systems[i].id) {
            continue;
        }
        for j in (i + 1)..len {
            if to_remove.contains(&systems[j].id) {
                continue;
            }
            if systems[i].system_type != systems[j].system_type {
                continue;
            }

            let dist = sphere_math::angular_distance(
                systems[i].lat,
                systems[i].lon,
                systems[j].lat,
                systems[j].lon,
            );

            let merge_dist = (systems[i].radius.min(systems[j].radius) * 0.5) as f64;
            if dist < merge_dist {
                // Weaker system gets absorbed
                if systems[i].pressure_anomaly.abs() >= systems[j].pressure_anomaly.abs() {
                    to_remove.push(systems[j].id);
                } else {
                    to_remove.push(systems[i].id);
                    break;
                }
            }
        }
    }

    systems.retain(|s| !to_remove.contains(&s.id));
}

/// Project macro weather effects (pressure, wind, humidity) from all pressure systems
/// onto every tile, using parallel evaluation.
fn project_macro_to_tiles(world: &mut World) {
    let systems = &world.macro_weather.systems;
    if systems.is_empty() {
        // Reset macro fields to defaults when no systems exist
        for tile in &mut world.tiles {
            tile.weather.pressure = 1013.25;
            tile.weather.macro_wind_speed = 0.0;
            tile.weather.macro_wind_direction = 0.0;
            tile.weather.macro_humidity = 0.0;
        }
        return;
    }

    // Pre-compute system data for parallel access
    let system_data: Vec<_> = systems
        .iter()
        .map(|s| {
            (
                s.lat,
                s.lon,
                s.pressure_anomaly,
                s.radius,
                s.moisture,
                s.system_type,
            )
        })
        .collect();

    // Compute macro fields for each tile in parallel
    let macro_fields: Vec<(f32, f32, f32, f32)> = world
        .tiles
        .par_iter()
        .map(|tile| {
            compute_tile_macro_fields(
                tile.position.lat,
                tile.position.lon,
                &system_data,
            )
        })
        .collect();

    // Apply computed fields to tiles
    for (i, (pressure, wind_speed, wind_dir, humidity)) in macro_fields.into_iter().enumerate() {
        world.tiles[i].weather.pressure = pressure;
        world.tiles[i].weather.macro_wind_speed = wind_speed;
        world.tiles[i].weather.macro_wind_direction = wind_dir;
        world.tiles[i].weather.macro_humidity = humidity;
    }
}

/// Compute macro weather fields for a single tile from all pressure systems.
fn compute_tile_macro_fields(
    tile_lat: f64,
    tile_lon: f64,
    systems: &[(f64, f64, f32, f32, f32, PressureSystemType)],
) -> (f32, f32, f32, f32) {
    let mut pressure_sum = 0.0_f32;
    let mut wind_east_sum = 0.0_f64;
    let mut wind_north_sum = 0.0_f64;
    let mut humidity_sum = 0.0_f32;
    let mut total_weight = 0.0_f32;

    for &(sys_lat, sys_lon, anomaly, radius, moisture, _sys_type) in systems {
        let dist = sphere_math::angular_distance(tile_lat, tile_lon, sys_lat, sys_lon);
        let radius_f64 = radius as f64;

        if dist > radius_f64 * 2.5 {
            continue; // Too far, no influence
        }

        // Gaussian falloff: anomaly * exp(-3 * (dist/radius)^2)
        let normalized_dist = dist / radius_f64;
        let weight = (-3.0 * normalized_dist * normalized_dist).exp() as f32;

        if weight < 0.01 {
            continue;
        }

        // 1. Pressure contribution
        pressure_sum += anomaly * weight;

        // 2. Wind: pressure gradient direction from system center to tile
        let (dir_east, dir_north) =
            sphere_math::direction_on_sphere(sys_lat, sys_lon, tile_lat, tile_lon);

        if dir_east.abs() > 1e-10 || dir_north.abs() > 1e-10 {
            // Gradient wind direction: outward from center
            // For low pressure: inward spiral (reverse direction)
            let inward = if anomaly < 0.0 { -1.0 } else { 1.0 };

            let grad_east = dir_east * inward;
            let grad_north = dir_north * inward;

            // Coriolis deflection: ~65 degrees from gradient direction
            // NH: deflect right (clockwise for highs, CCW for lows)
            // SH: deflect left (opposite)
            let hemisphere_sign = if tile_lat >= 0.0 { 1.0 } else { -1.0 };

            // For low pressure (inward): deflection creates cyclonic rotation
            // For high pressure (outward): deflection creates anticyclonic rotation
            let deflection_angle = hemisphere_sign * 65.0_f64.to_radians();

            let (wind_e, wind_n) =
                sphere_math::rotate_tangent_vector(grad_east, grad_north, deflection_angle);

            // Wind speed proportional to pressure gradient magnitude
            // Gradient is steeper near the center (higher weight = closer)
            let gradient_strength = anomaly.abs() as f64 * weight as f64;
            let speed_scale = gradient_strength * 0.15; // tuning factor

            wind_east_sum += wind_e * speed_scale;
            wind_north_sum += wind_n * speed_scale;
        }

        // 3. Humidity
        humidity_sum += moisture * weight;
        total_weight += weight;
    }

    // Final pressure
    let pressure = 1013.25 + pressure_sum;

    // Final wind
    let wind_speed = ((wind_east_sum * wind_east_sum + wind_north_sum * wind_north_sum).sqrt()) as f32;
    let wind_direction = if wind_speed > 0.01 {
        sphere_math::tangent_to_bearing(wind_east_sum, wind_north_sum) as f32
    } else {
        0.0
    };

    // Final humidity (weighted average)
    let humidity = if total_weight > 0.01 {
        (humidity_sum / total_weight).clamp(0.0, 1.0)
    } else {
        0.0
    };

    (pressure, wind_speed, wind_direction, humidity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::generation::GenerationParams;
    use crate::world::generation::generate_world;

    fn default_gen_params(tile_count: u32) -> GenerationParams {
        GenerationParams {
            seed: 42,
            tile_count,
            ocean_ratio: 0.3,
            mountain_ratio: 0.1,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
            topology: crate::config::generation::TopologyConfig::default(),
        }
    }

    fn geodesic_gen_params(level: u32) -> GenerationParams {
        GenerationParams {
            seed: 42,
            tile_count: 1000,
            ocean_ratio: 0.3,
            mountain_ratio: 0.1,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
            topology: crate::config::generation::TopologyConfig {
                mode: "geodesic".to_string(),
                subdivision_level: level,
            },
        }
    }

    #[test]
    fn macro_weather_step_deterministic() {
        let mut world_a = generate_world(&default_gen_params(200));
        let mut world_b = generate_world(&default_gen_params(200));

        // Run 50 ticks of macro weather
        for _ in 0..50 {
            macro_weather_step(&mut world_a);
            macro_weather_step(&mut world_b);
        }

        assert_eq!(
            world_a.macro_weather.systems.len(),
            world_b.macro_weather.systems.len(),
            "System count should be deterministic"
        );

        for (a, b) in world_a
            .macro_weather
            .systems
            .iter()
            .zip(world_b.macro_weather.systems.iter())
        {
            assert_eq!(a.id, b.id);
            assert_eq!(a.lat, b.lat);
            assert_eq!(a.pressure_anomaly, b.pressure_anomaly);
        }
    }

    #[test]
    fn systems_spawn_and_evolve() {
        let mut world = generate_world(&default_gen_params(500));

        // Run enough ticks for systems to spawn
        for _ in 0..100 {
            macro_weather_step(&mut world);
        }

        assert!(
            !world.macro_weather.systems.is_empty(),
            "Should have spawned at least one system after 100 ticks"
        );

        // All systems should have valid positions
        for system in &world.macro_weather.systems {
            assert!(
                system.lat >= -90.0 && system.lat <= 90.0,
                "System {} has invalid lat: {}",
                system.id,
                system.lat
            );
            assert!(
                system.lat.is_finite() && system.lon.is_finite(),
                "System {} has NaN/Inf position",
                system.id
            );
            assert!(
                system.pressure_anomaly.abs() >= 2.0,
                "Dead systems should be removed, got anomaly {}",
                system.pressure_anomaly
            );
        }
    }

    #[test]
    fn projection_sets_tile_fields() {
        let mut world = generate_world(&default_gen_params(200));

        // Manually add a strong low-pressure system
        let (x, y, z) = sphere_math::lat_lon_to_xyz(45.0, 0.0);
        world.macro_weather.systems.push(PressureSystem {
            id: 999,
            lat: 45.0,
            lon: 0.0,
            x,
            y,
            z,
            pressure_anomaly: -20.0,
            radius: 0.5, // large radius to cover many tiles
            velocity_east: 0.0,
            velocity_north: 0.0,
            age: 0,
            max_age: 1000,
            system_type: PressureSystemType::MidLatCyclone,
            moisture: 0.8,
        });

        project_macro_to_tiles(&mut world);

        // At least some tiles should have non-default pressure
        let non_default_pressure = world
            .tiles
            .iter()
            .filter(|t| (t.weather.pressure - 1013.25).abs() > 0.1)
            .count();

        assert!(
            non_default_pressure > 0,
            "Some tiles should have non-default pressure from the system"
        );

        // Some tiles should have macro wind
        let has_wind = world
            .tiles
            .iter()
            .filter(|t| t.weather.macro_wind_speed > 0.01)
            .count();

        assert!(
            has_wind > 0,
            "Some tiles should have macro wind from the system"
        );
    }

    #[test]
    fn gaussian_falloff_correct() {
        // Tile at system center should get full anomaly
        let systems = vec![(45.0, 0.0, -20.0_f32, 0.3_f32, 0.8_f32, PressureSystemType::MidLatCyclone)];

        let (pressure, _, _, _) = compute_tile_macro_fields(45.0, 0.0, &systems);
        // At center, weight = exp(0) = 1.0, so pressure = 1013.25 + (-20) = 993.25
        assert!(
            (pressure - 993.25).abs() < 0.5,
            "Pressure at center should be ~993.25, got {}",
            pressure
        );

        // Tile at radius distance should get reduced anomaly
        // At dist=radius, weight = exp(-3) ≈ 0.05
        let far_lat = 45.0 + (0.3_f64 * 180.0 / std::f64::consts::PI); // ~17 degrees
        let (pressure_far, _, _, _) = compute_tile_macro_fields(far_lat, 0.0, &systems);
        assert!(
            pressure_far > pressure,
            "Pressure farther away should be higher (less negative anomaly)"
        );
    }

    #[test]
    fn coriolis_direction_nh_low() {
        // NH low pressure: winds should spiral counterclockwise inward
        let systems = vec![(45.0, 0.0, -20.0_f32, 0.5_f32, 0.8_f32, PressureSystemType::MidLatCyclone)];

        // Check a tile east of the system center
        let (_, wind_speed, _wind_dir, _) = compute_tile_macro_fields(45.0, 5.0, &systems);

        assert!(
            wind_speed > 0.01,
            "Should have non-zero wind east of NH low"
        );
        // East of a NH low, CCW rotation means wind should have a significant northward component
        // (wind direction should be roughly southerly to northerly flow on east side)
    }

    #[test]
    fn geodesic_world_macro_weather() {
        let mut world = generate_world(&geodesic_gen_params(2));

        for _ in 0..50 {
            macro_weather_step(&mut world);
        }

        // Should work without errors on geodesic worlds
        for tile in &world.tiles {
            assert!(
                tile.weather.pressure.is_finite(),
                "Tile {} has non-finite pressure: {}",
                tile.id,
                tile.weather.pressure
            );
            assert!(
                tile.weather.macro_wind_speed.is_finite(),
                "Tile {} has non-finite macro wind speed",
                tile.id
            );
        }
    }

    #[test]
    fn systems_capped_at_max() {
        let mut world = generate_world(&default_gen_params(200));
        let max_systems = (world.tiles.len() / 100).max(5).min(80);

        // Run many ticks to ensure spawning is capped
        for _ in 0..500 {
            macro_weather_step(&mut world);
        }

        assert!(
            world.macro_weather.systems.len() <= max_systems,
            "Systems should be capped at {}, got {}",
            max_systems,
            world.macro_weather.systems.len()
        );
    }

    #[test]
    fn spatial_grid_matches_linear_scan() {
        let world = generate_world(&geodesic_gen_params(2));
        let tiles_snapshot: Vec<(f64, f64, TerrainType, f32)> = world
            .tiles
            .iter()
            .map(|t| (t.position.lat, t.position.lon, t.geology.terrain_type, t.climate.base_temperature))
            .collect();

        let grid = SpatialGrid::new(&tiles_snapshot);

        // Test several query points, including poles and date line
        let test_points = vec![
            (0.0, 0.0), (45.0, 90.0), (-30.0, -120.0),
            (85.0, 0.0), (-85.0, 0.0), (0.0, 179.0), (0.0, -179.0),
        ];

        for (lat, lon) in test_points {
            let (grid_terrain, grid_temp) = grid.find_nearest(lat, lon, &tiles_snapshot);

            // Linear scan for comparison
            let mut min_dist = f64::MAX;
            let mut linear_terrain = TerrainType::Ocean;
            let mut linear_temp = 288.0_f32;
            for &(tlat, tlon, terrain, temp) in &tiles_snapshot {
                let dist = sphere_math::angular_distance(lat, lon, tlat, tlon);
                if dist < min_dist {
                    min_dist = dist;
                    linear_terrain = terrain;
                    linear_temp = temp;
                }
            }

            assert_eq!(
                grid_terrain, linear_terrain,
                "Grid terrain mismatch at ({}, {}): {:?} vs {:?}",
                lat, lon, grid_terrain, linear_terrain
            );
            assert_eq!(
                grid_temp, linear_temp,
                "Grid temp mismatch at ({}, {})",
                lat, lon
            );
        }
    }

    #[test]
    fn spatial_grid_wrapping_and_poles() {
        // Verify grid handles edge cases: exact boundary values
        let tiles = vec![
            (89.0, 0.0, TerrainType::Plains, 260.0_f32),
            (-89.0, 0.0, TerrainType::Ocean, 270.0),
            (0.0, 179.0, TerrainType::Coast, 295.0),
            (0.0, -179.0, TerrainType::Hills, 290.0),
        ];

        let grid = SpatialGrid::new(&tiles);

        // Near north pole
        let (terrain, _) = grid.find_nearest(88.0, 10.0, &tiles);
        assert_eq!(terrain, TerrainType::Plains);

        // Near south pole
        let (terrain, _) = grid.find_nearest(-88.0, 10.0, &tiles);
        assert_eq!(terrain, TerrainType::Ocean);

        // Near dateline from east
        let (terrain, _) = grid.find_nearest(0.0, 178.0, &tiles);
        assert_eq!(terrain, TerrainType::Coast);
    }

    #[test]
    fn empty_systems_resets_tile_fields() {
        let mut world = generate_world(&default_gen_params(100));

        // Set non-default values
        world.tiles[0].weather.pressure = 990.0;
        world.tiles[0].weather.macro_wind_speed = 5.0;

        // Ensure no systems
        world.macro_weather.systems.clear();
        project_macro_to_tiles(&mut world);

        assert_eq!(world.tiles[0].weather.pressure, 1013.25);
        assert_eq!(world.tiles[0].weather.macro_wind_speed, 0.0);
    }
}
