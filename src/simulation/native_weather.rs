/// Native Rust implementation of the 4 weather rules.
///
/// Unlike the Rhai scripts (which each read from the pre-phase tile snapshot),
/// the native evaluator chains rule outputs via a WeatherAccum struct. Each
/// rule reads and writes the accumulator's running values, so later rules see
/// earlier rules' computations within the same tick. This fixes mutation
/// conflicts where last-write-wins would discard intermediate results.
///
/// RNG: uses the same xorshift64 PRNG with the same call sequence
/// (rand/rand_range consumed in identical order including conditionals).

use rhai::Dynamic;

use crate::simulation::engine::{Phase, TileMutations};
use crate::simulation::native_eval::NativePhaseEvaluator;
use crate::simulation::sphere_math::{direction_on_sphere, tangent_to_bearing};
use crate::world::tile::{Season, Tile};

/// xorshift64 PRNG matching the engine's implementation.
fn xorshift64(mut state: u64) -> u64 {
    if state == 0 {
        state = 1;
    }
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    #[allow(dead_code)]
    fn rand(&mut self) -> f64 {
        self.state = xorshift64(self.state);
        self.state as f64 / u64::MAX as f64
    }

    fn rand_range(&mut self, min: f64, max: f64) -> f64 {
        self.state = xorshift64(self.state);
        let t = self.state as f64 / u64::MAX as f64;
        min + t * (max - min)
    }
}

/// Running mutable weather values shared across rules within a single tick.
/// Initialized from the tile snapshot; each rule reads/writes these fields
/// so later rules see earlier rules' outputs. Immutable fields (geology,
/// climate, macro_*, pressure) are still read from the tile directly.
struct WeatherAccum {
    wind_direction: f64,
    wind_speed: f64,
    temperature: f64,
    humidity: f64,
    cloud_cover: f64,
    precipitation: f64,
    precipitation_type: String,
    storm_intensity: f64,
}

impl WeatherAccum {
    fn from_tile(tile: &Tile) -> Self {
        Self {
            wind_direction: tile.weather.wind_direction as f64,
            wind_speed: tile.weather.wind_speed as f64,
            temperature: tile.weather.temperature as f64,
            humidity: tile.weather.humidity as f64,
            cloud_cover: tile.weather.cloud_cover as f64,
            precipitation: tile.weather.precipitation as f64,
            precipitation_type: format!("{:?}", tile.weather.precipitation_type),
            storm_intensity: tile.weather.storm_intensity as f64,
        }
    }

    fn into_mutations(self) -> Vec<(String, Dynamic)> {
        vec![
            ("wind_direction".to_string(), Dynamic::from(self.wind_direction)),
            ("wind_speed".to_string(), Dynamic::from(self.wind_speed)),
            ("temperature".to_string(), Dynamic::from(self.temperature)),
            ("humidity".to_string(), Dynamic::from(self.humidity)),
            ("cloud_cover".to_string(), Dynamic::from(self.cloud_cover)),
            ("precipitation".to_string(), Dynamic::from(self.precipitation)),
            ("precipitation_type".to_string(), Dynamic::from(self.precipitation_type)),
            ("storm_intensity".to_string(), Dynamic::from(self.storm_intensity)),
        ]
    }
}

/// Helper: average of a field across neighbors.
fn neighbor_avg_f64(neighbors: &[&Tile], accessor: fn(&Tile) -> f64) -> f64 {
    if neighbors.is_empty() {
        return 0.0;
    }
    let sum: f64 = neighbors.iter().map(|n| accessor(n)).sum();
    sum / neighbors.len() as f64
}

/// Helper: max of a field across neighbors.
fn neighbor_max_f64(neighbors: &[&Tile], accessor: fn(&Tile) -> f64) -> f64 {
    neighbors.iter().map(|n| accessor(n)).reduce(|a, b| a.max(b)).unwrap_or(0.0)
}

/// Helper: terrain type string comparison equivalent.
fn terrain_is(tile: &Tile, name: &str) -> bool {
    use crate::simulation::engine::terrain_type_str;
    terrain_type_str(tile.geology.terrain_type) == name
}

/// Precomputed bearing from each neighbor toward each tile (degrees).
/// `reverse_bearings[tile_id][j]` = bearing (0=N, 90=E) from neighbor j TO tile_id.
/// This tells us what wind direction at neighbor j would blow moisture toward tile_id.
/// Computed once at startup to avoid ~24K trig calls per tick.
pub struct NeighborBearings {
    reverse_bearings: Vec<Vec<f64>>,
    /// True when tiles have real lat/lon (geodesic). False when all lat/lon are 0 (flat hex).
    pub has_geo: bool,
}

impl NeighborBearings {
    /// Build bearings from a tile slice. If all tiles have lat=0 and lon=0
    /// (flat hex topology), sets `has_geo = false` and skips trig.
    pub fn from_tiles(tiles: &[Tile]) -> Self {
        let has_geo = tiles.iter().any(|t| t.position.lat != 0.0 || t.position.lon != 0.0);
        if !has_geo {
            return Self {
                reverse_bearings: Vec::new(),
                has_geo: false,
            };
        }

        let reverse_bearings: Vec<Vec<f64>> = tiles
            .iter()
            .map(|tile| {
                tile.neighbors
                    .iter()
                    .map(|&nid| {
                        if let Some(neighbor) = tiles.get(nid as usize) {
                            let (east, north) = direction_on_sphere(
                                neighbor.position.lat,
                                neighbor.position.lon,
                                tile.position.lat,
                                tile.position.lon,
                            );
                            tangent_to_bearing(east, north)
                        } else {
                            0.0
                        }
                    })
                    .collect()
            })
            .collect();

        Self {
            reverse_bearings,
            has_geo,
        }
    }

    /// Get the bearing from neighbor index `j` toward tile `tile_id`.
    #[inline]
    fn bearing(&self, tile_id: usize, neighbor_idx: usize) -> f64 {
        self.reverse_bearings[tile_id][neighbor_idx]
    }
}

pub struct NativeWeatherEvaluator {
    bearings: NeighborBearings,
}

impl NativeWeatherEvaluator {
    pub fn new(tiles: &[Tile]) -> Self {
        Self {
            bearings: NeighborBearings::from_tiles(tiles),
        }
    }
}

impl NativePhaseEvaluator for NativeWeatherEvaluator {
    fn phase(&self) -> Phase {
        Phase::Weather
    }

    fn evaluate(
        &self,
        tile: &Tile,
        neighbors: &[&Tile],
        season: Season,
        _tick: u64,
        rng_seed: u64,
    ) -> TileMutations {
        let mut rng = Rng::new(rng_seed);
        let mut accum = WeatherAccum::from_tile(tile);

        // ===== Rule 1: Wind & Temperature =====
        rule_wind_temperature(tile, neighbors, season, &mut rng, &mut accum);

        // ===== Rule 2: Humidity =====
        rule_humidity(tile, neighbors, season, &mut rng, &mut accum, &self.bearings);

        // ===== Rule 3: Clouds & Precipitation =====
        rule_clouds_precipitation(tile, neighbors, season, &mut rng, &mut accum, &self.bearings);

        // ===== Rule 4: Storms =====
        rule_storms(tile, neighbors, &mut rng, &mut accum);

        TileMutations { mutations: accum.into_mutations() }
    }
}

/// Rule 1: Wind & Temperature (01-wind-temperature.rhai)
fn rule_wind_temperature(
    tile: &Tile,
    neighbors: &[&Tile],
    season: Season,
    rng: &mut Rng,
    accum: &mut WeatherAccum,
) {
    let lat = tile.climate.latitude as f64;
    let abs_lat = lat.abs();
    let terrain_str = crate::simulation::engine::terrain_type_str(tile.geology.terrain_type);
    let elev = tile.geology.elevation as f64;

    // === WIND (macro-driven) ===
    let macro_speed = tile.weather.macro_wind_speed as f64;
    let macro_dir = tile.weather.macro_wind_direction as f64;

    // Terrain friction factor
    let friction = match terrain_str {
        "Mountains" | "Cliffs" => 0.4,
        "Hills" => 0.7,
        "Ocean" => 1.3,
        "Coast" => 1.15,
        "Wetlands" => 0.9,
        _ => 1.0,
    };

    // Target direction
    let target_dir = if macro_speed > 0.5 {
        macro_dir
    } else {
        if abs_lat < 30.0 {
            if lat >= 0.0 { 45.0 } else { 135.0 }
        } else if abs_lat < 60.0 {
            if lat >= 0.0 { 225.0 } else { 315.0 }
        } else {
            if lat >= 0.0 { 45.0 } else { 135.0 }
        }
    };

    // Seasonal wind shift
    let seasonal_shift = match season {
        Season::Summer => 8.0,
        Season::Winter => -8.0,
        Season::Spring => 4.0,
        Season::Autumn => -4.0,
    };
    let target_dir = target_dir + seasonal_shift;

    let target_speed = if macro_speed > 0.5 {
        macro_speed * friction
    } else {
        let base = if abs_lat < 30.0 { 4.0 }
        else if abs_lat < 60.0 { 6.5 }
        else { 3.5 };
        base * friction
    };

    // Smooth transition — read from accum (initially tile snapshot)
    let current_dir = accum.wind_direction;
    let current_speed = accum.wind_speed;
    let diff = target_dir - current_dir;
    let mut adj_diff = diff;
    if adj_diff > 180.0 { adj_diff -= 360.0; }
    if adj_diff < -180.0 { adj_diff += 360.0; }

    let blend = if macro_speed > 0.5 { 0.35 } else { 0.2 };
    let mut norm_dir = current_dir + adj_diff * blend + rng.rand_range(-10.0, 10.0);
    if norm_dir < 0.0 { norm_dir += 360.0; }
    norm_dir = norm_dir % 360.0;

    let mut new_speed = current_speed * 0.6 + target_speed * 0.4 + rng.rand_range(-0.5, 0.5);

    // === SEA BREEZE ===
    if terrain_str == "Coast" {
        let mut ocean_count = 0;
        for n in neighbors {
            if terrain_is(n, "Ocean") {
                ocean_count += 1;
            }
        }
        if ocean_count > 0 {
            let season_factor = match season {
                Season::Summer => 1.0,
                Season::Spring => 0.6,
                Season::Autumn => 0.4,
                Season::Winter => 0.15,
            };
            new_speed += 0.3 * season_factor;
        }
    }

    // Hard cap
    if new_speed > 20.0 { new_speed = 20.0; }
    if new_speed < 0.3 { new_speed = 0.3; }

    accum.wind_direction = norm_dir;
    accum.wind_speed = new_speed;

    // === TEMPERATURE ===
    let base_temp = tile.climate.base_temperature as f64;
    let elev_adj = if elev > 0.0 { elev * 20.0 } else { 0.0 };

    let season_factor = match season {
        Season::Spring => 0.5,
        Season::Summer => 1.0,
        Season::Autumn => -0.5,
        Season::Winter => -1.0,
    };

    let mut seasonal_amplitude = 6.0 + abs_lat * 0.15;
    if seasonal_amplitude > 18.0 { seasonal_amplitude = 18.0; }

    let seasonal_mod = season_factor * seasonal_amplitude;

    let ocean_damping = match terrain_str {
        "Ocean" => 0.25,
        "Coast" => 0.55,
        _ => 1.0,
    };

    let diffusion_amount = 0.08;
    let local_temp = base_temp - elev_adj + seasonal_mod * ocean_damping + rng.rand_range(-1.5, 1.5);

    if !neighbors.is_empty() {
        let n_avg_temp = neighbor_avg_f64(neighbors, |t| t.weather.temperature as f64);
        let temp = local_temp * (1.0 - diffusion_amount) + n_avg_temp * diffusion_amount;
        accum.temperature = temp;
    } else {
        accum.temperature = local_temp;
    }
}

/// Compute wind-directed advection weight for a single neighbor.
/// Returns how much this neighbor's quantity should contribute to the target tile.
/// `neighbor_wind_dir` is the neighbor's wind direction in degrees (0=N, 90=E).
/// `bearing_to_tile` is the bearing from neighbor toward the target tile in degrees.
/// Positive alignment means the wind at the neighbor is blowing toward us.
#[inline]
fn advection_weight(neighbor_wind_dir: f64, bearing_to_tile: f64, neighbor_wind_speed: f64) -> f64 {
    let diff_rad = (neighbor_wind_dir - bearing_to_tile).to_radians();
    let alignment = diff_rad.cos(); // 1.0 = wind blows directly toward us, -1.0 = away
    if alignment <= 0.0 {
        return 0.0;
    }
    let speed_factor = (neighbor_wind_speed / 10.0).min(1.5);
    alignment * speed_factor
}

/// Compute wind-directed advection of a quantity from neighbors toward a tile.
/// Returns (advected_value, total_weight). If bearings are unavailable (flat hex),
/// falls back to isotropic averaging.
fn compute_advected(
    tile: &Tile,
    neighbors: &[&Tile],
    bearings: &NeighborBearings,
    accessor: fn(&Tile) -> f64,
) -> (f64, f64) {
    if !bearings.has_geo || neighbors.is_empty() {
        // Fallback: isotropic average
        let avg = neighbor_avg_f64(neighbors, accessor);
        return (avg, 1.0);
    }

    let tile_idx = tile.id as usize;
    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;

    for (j, n) in neighbors.iter().enumerate() {
        let bearing = bearings.bearing(tile_idx, j);
        let w = advection_weight(
            n.weather.wind_direction as f64,
            bearing,
            n.weather.wind_speed as f64,
        );
        if w > 0.0 {
            weighted_sum += accessor(n) * w;
            total_weight += w;
        }
    }

    if total_weight > 0.0 {
        (weighted_sum / total_weight, total_weight)
    } else {
        // No upwind neighbors — use isotropic fallback
        let avg = neighbor_avg_f64(neighbors, accessor);
        (avg, 0.0)
    }
}

/// Convergence vertical motion based on latitude and season.
/// Returns a value: positive = rising air (more clouds/humidity), negative = sinking (fewer clouds).
fn convergence_vertical_motion(lat: f64, season: Season) -> f64 {
    let abs_lat = lat.abs();

    // Seasonal ITCZ shift: moves toward summer hemisphere
    let itcz_shift = match season {
        Season::Summer => if lat >= 0.0 { 8.0 } else { -8.0 },
        Season::Winter => if lat >= 0.0 { -8.0 } else { 8.0 },
        Season::Spring => if lat >= 0.0 { 4.0 } else { -4.0 },
        Season::Autumn => if lat >= 0.0 { -4.0 } else { 4.0 },
    };

    // Distance from ITCZ center (shifted equator)
    let itcz_center = itcz_shift; // absolute latitude of ITCZ center
    let dist_from_itcz = (lat - itcz_center).abs();

    // ITCZ: strong rising near equator (0-10° from center)
    if dist_from_itcz < 10.0 {
        let strength = 1.0 - dist_from_itcz / 10.0;
        return 0.15 * strength;
    }

    // Subtropical descent (25-35° abs lat): sinking air
    if abs_lat >= 25.0 && abs_lat <= 35.0 {
        let center_dist = (abs_lat - 30.0).abs();
        let strength = 1.0 - center_dist / 5.0;
        return -0.10 * strength;
    }

    // Polar front (55-65° abs lat): rising air
    if abs_lat >= 55.0 && abs_lat <= 65.0 {
        let center_dist = (abs_lat - 60.0).abs();
        let strength = 1.0 - center_dist / 5.0;
        return 0.08 * strength;
    }

    // Polar cap (>75°): subsidence
    if abs_lat > 75.0 {
        let strength = ((abs_lat - 75.0) / 15.0).min(1.0);
        return -0.05 * strength;
    }

    0.0
}

/// Rule 2: Humidity (02-humidity.rhai)
fn rule_humidity(
    tile: &Tile,
    neighbors: &[&Tile],
    season: Season,
    _rng: &mut Rng,
    accum: &mut WeatherAccum,
    bearings: &NeighborBearings,
) {
    let terrain_str = crate::simulation::engine::terrain_type_str(tile.geology.terrain_type);
    let temp = accum.temperature; // reads Rule 1's output
    let current_humidity = accum.humidity;
    let macro_humidity = tile.weather.macro_humidity as f64;

    // === EVAPORATION ===
    let mut temp_factor = (temp - 250.0) / 60.0;
    if temp_factor < 0.0 { temp_factor = 0.0; }
    if temp_factor > 1.5 { temp_factor = 1.5; }

    let raw_evaporation = match terrain_str {
        "Ocean" => 0.08 + temp_factor * 0.12,
        "Coast" => 0.05 + temp_factor * 0.08,
        "Wetlands" => 0.04 + temp_factor * 0.04,
        _ => {
            let soil_m = tile.conditions.soil_moisture as f64;
            let veg = tile.biome.vegetation_density as f64;
            let veg_h = tile.biome.vegetation_health as f64;
            // Bare soil evaporation (soil moisture + temperature driven)
            let soil_evap = soil_m * 0.04 * temp_factor;
            // Transpiration: healthy vegetation pumps groundwater -> atmosphere
            let transpiration = veg * veg_h * 0.08 * temp_factor * soil_m.sqrt();
            (soil_evap + transpiration).min(0.15)
        }
    };
    // Diminishing returns: saturated air absorbs less moisture
    let evaporation = raw_evaporation * (1.0 - current_humidity).max(0.0);

    let season_evap_mult = match season {
        Season::Summer => 1.3,
        Season::Winter => 0.7,
        _ => 1.0,
    };
    let evaporation = evaporation * season_evap_mult;

    // === WIND-DIRECTED HUMIDITY ADVECTION ===
    let n_count = neighbors.len();
    let (advected_humidity, advection_weight_total) = compute_advected(
        tile, neighbors, bearings, |t| t.weather.humidity as f64,
    );

    // Isotropic fallback (small component for stability)
    let isotropic = if n_count > 0 {
        neighbor_avg_f64(neighbors, |t| t.weather.humidity as f64)
    } else {
        current_humidity
    };

    // === MARITIME MOISTURE BOOST ===
    // Land tiles near ocean receive onshore moisture (sea breeze effect)
    let maritime_boost = if terrain_str != "Ocean" && terrain_str != "Coast" && n_count > 0 {
        let ocean_count = neighbors.iter().filter(|n| {
            let nt = crate::simulation::engine::terrain_type_str(n.geology.terrain_type);
            nt == "Ocean" || nt == "Coast"
        }).count();
        let ocean_frac = ocean_count as f64 / n_count as f64;
        ocean_frac * 0.10 * temp_factor
    } else {
        0.0
    };

    // === CONVERGENCE ZONE HUMIDITY EFFECT ===
    let lat = tile.climate.latitude as f64;
    let vertical_motion = convergence_vertical_motion(lat, season);
    // Subsidence suppresses humidity (drier descending air)
    let convergence_humidity_mod = if vertical_motion < 0.0 {
        vertical_motion * 0.5 // e.g., -0.10 * 0.5 = -0.05 humidity suppression
    } else {
        0.0 // rising air doesn't directly add humidity (it adds clouds in Rule 3)
    };

    // === BLEND ===
    // Macro influence proportional to coverage strength
    let macro_weight = (macro_humidity * 3.5_f64).min(0.35);
    let local_weight = 1.0 - macro_weight;

    // Neighbor contribution: wind-directed advection dominates, small isotropic component
    let neighbor_blend = if bearings.has_geo && advection_weight_total > 0.0 {
        advected_humidity * 0.80 + isotropic * 0.20
    } else {
        isotropic // pure isotropic when no bearing data or no upwind neighbors
    };

    // Local: self-retention + advected neighbors + evaporation + maritime + convergence
    let local_humidity = current_humidity * 0.75 + neighbor_blend * 0.20 + maritime_boost
        + convergence_humidity_mod;
    let mut new_humidity = macro_humidity * macro_weight
        + (local_humidity + evaporation) * local_weight;

    // === OROGRAPHIC STRIPPING ===
    let orographic_loss = match terrain_str {
        "Mountains" | "Cliffs" => {
            let mut strip = 0.4 + tile.geology.elevation as f64 * 0.5;
            if strip > 0.8 { strip = 0.8; }
            strip
        }
        "Hills" => {
            let mut strip = 0.15 + tile.geology.elevation as f64 * 0.3;
            if strip > 0.4 { strip = 0.4; }
            strip
        }
        _ => 0.0,
    };

    // Rain shadow
    let mut shadow_factor = 0.0;
    for n in neighbors {
        let n_terrain = crate::simulation::engine::terrain_type_str(n.geology.terrain_type);
        if n_terrain == "Mountains" || n_terrain == "Cliffs" || n_terrain == "Hills" {
            let strength = if n_terrain == "Mountains" || n_terrain == "Cliffs" {
                n.geology.elevation as f64 * 0.08
            } else {
                n.geology.elevation as f64 * 0.03
            };
            if strength > shadow_factor { shadow_factor = strength; }
        }
    }

    let mut total_loss = orographic_loss + shadow_factor;
    if total_loss > 0.85 { total_loss = 0.85; }
    new_humidity *= 1.0 - total_loss;

    // Moisture-dependent atmospheric decay: humid air is more stable
    let decay = (0.994 + new_humidity * 0.005).min(0.999);
    new_humidity *= decay;

    if new_humidity < 0.0 { new_humidity = 0.0; }
    if new_humidity > 1.0 { new_humidity = 1.0; }

    accum.humidity = new_humidity;
}

/// Rule 3: Clouds & Precipitation (03-clouds-precipitation.rhai)
fn rule_clouds_precipitation(
    tile: &Tile,
    neighbors: &[&Tile],
    season: Season,
    rng: &mut Rng,
    accum: &mut WeatherAccum,
    bearings: &NeighborBearings,
) {
    let temp = accum.temperature; // reads Rule 1's output
    let humidity = accum.humidity; // reads Rule 2's output
    let cloud = accum.cloud_cover;
    let terrain_str = crate::simulation::engine::terrain_type_str(tile.geology.terrain_type);

    // === SATURATION HUMIDITY ===
    // Higher base (0.40) prevents cold air from instantly supersaturating.
    // Quadratic warm-end boost reflects warm air holding more moisture.
    let warm_excess = ((temp - 270.0) / 30.0).max(0.0);
    let mut saturation = 0.40 + (temp - 250.0) * 0.006 + warm_excess * warm_excess * 0.2;
    if saturation < 0.40 { saturation = 0.40; }
    if saturation > 1.2 { saturation = 1.2; }

    let mut relative_humidity = humidity / saturation;
    if relative_humidity > 1.5 { relative_humidity = 1.5; }

    // === CLOUD COVER ===
    // Gentler curve: peaks around 0.75 at full saturation, not 0.91+
    let mut target_cloud = if relative_humidity < 0.30 {
        relative_humidity * 0.1
    } else if relative_humidity < 0.55 {
        0.03 + (relative_humidity - 0.30) * 0.5
    } else if relative_humidity < 0.80 {
        0.155 + (relative_humidity - 0.55) * 1.2
    } else if relative_humidity < 1.1 {
        0.455 + (relative_humidity - 0.80) * 1.0
    } else {
        0.755 + (relative_humidity - 1.1) * 0.2
    };
    if target_cloud > 0.85 { target_cloud = 0.85; }

    // === CONVERGENCE ZONE CLOUD EFFECTS ===
    let lat = tile.climate.latitude as f64;
    let vertical_motion = convergence_vertical_motion(lat, season);
    // Rising air boosts cloud target, sinking air suppresses
    target_cloud += vertical_motion;
    if target_cloud < 0.0 { target_cloud = 0.0; }
    if target_cloud > 0.85 { target_cloud = 0.85; }

    // === WIND-DIRECTED NEIGHBOR CLOUD INFLUENCE ===
    let (advected_cloud, advection_wt) = compute_advected(
        tile, neighbors, bearings, |t| t.weather.cloud_cover as f64,
    );
    let isotropic_cloud = if !neighbors.is_empty() {
        neighbor_avg_f64(neighbors, |t| t.weather.cloud_cover as f64)
    } else {
        cloud
    };

    let neighbor_cloud_blend = if bearings.has_geo && advection_wt > 0.0 {
        advected_cloud * 0.80 + isotropic_cloud * 0.20
    } else {
        isotropic_cloud
    };

    let neighbor_storm_max = neighbor_max_f64(neighbors, |t| t.weather.storm_intensity as f64);

    target_cloud = target_cloud * 0.85 + neighbor_cloud_blend * 0.15;
    if target_cloud > 1.0 { target_cloud = 1.0; }

    // Pre-storm cloud enhancement
    if neighbor_storm_max > 0.2 {
        let storm_cloud_boost = neighbor_storm_max * 0.15;
        target_cloud += storm_cloud_boost;
        if target_cloud > 1.0 { target_cloud = 1.0; }
    }

    // Cloud inertia (formation faster than dissipation, but dissipation not glacial)
    let cloud_speed = if target_cloud > cloud {
        let urgency = target_cloud - cloud;
        if urgency > 0.3 { 0.18 } else { 0.10 }
    } else {
        0.10
    };

    let mut new_cloud = cloud + (target_cloud - cloud) * cloud_speed + rng.rand_range(-0.02, 0.02);
    if new_cloud < 0.0 { new_cloud = 0.0; }
    if new_cloud > 1.0 { new_cloud = 1.0; }
    accum.cloud_cover = new_cloud;

    // === PRECIPITATION ===
    if relative_humidity > 0.70 && new_cloud > 0.35 {
        let excess = relative_humidity - 0.70;
        let mut intensity = excess * new_cloud * 1.2;

        if terrain_str == "Mountains" || terrain_str == "Cliffs" {
            intensity *= 1.8;
        } else if terrain_str == "Hills" {
            intensity *= 1.3;
        }

        if temp > 290.0 && humidity > 0.5 {
            intensity *= 1.2;
        }

        if intensity > 1.0 { intensity = 1.0; }
        if intensity < 0.01 { intensity = 0.0; }

        if intensity > 0.0 {
            accum.precipitation = intensity;

            let precip_type = if temp < 258.0 {
                "Snow"
            } else if temp < 268.0 {
                "Snow"
            } else if temp < 273.0 {
                "Sleet"
            } else {
                "Rain"
            };
            accum.precipitation_type = precip_type.to_string();

            // Precipitation removes moisture — scale with available humidity
            let consumed = intensity * 0.15 * accum.humidity;
            let mut new_h = accum.humidity - consumed;
            if new_h < 0.02 { new_h = 0.02; }
            accum.humidity = new_h;

            // === PRECIPITATION CLOUD CLEARING ===
            // Negative feedback: precipitation removes clouds (rain washes out condensation nuclei).
            // Quadratic: light drizzle barely clears, heavy rain clears strongly.
            let cloud_clearing = intensity * intensity * 0.20;
            accum.cloud_cover = (accum.cloud_cover - cloud_clearing).max(0.0);
        } else {
            accum.precipitation = 0.0;
            accum.precipitation_type = "None".to_string();
        }
    } else {
        accum.precipitation = 0.0;
        accum.precipitation_type = "None".to_string();
    }
}

/// Rule 4: Storms (04-storms.rhai)
fn rule_storms(
    tile: &Tile,
    neighbors: &[&Tile],
    rng: &mut Rng,
    accum: &mut WeatherAccum,
) {
    let terrain_str = crate::simulation::engine::terrain_type_str(tile.geology.terrain_type);
    let current_storm = accum.storm_intensity;
    let humidity = accum.humidity;       // reads Rule 2/3's output
    let temp = accum.temperature;        // reads Rule 1's output
    let cloud = accum.cloud_cover;       // reads Rule 3's output
    let wind_speed = accum.wind_speed;   // reads Rule 1's output
    let pressure = tile.weather.pressure as f64; // immutable macro field

    // === GATHER NEIGHBOR DATA ===
    let mut max_temp_diff = 0.0;
    for n in neighbors.iter() {
        let diff = (temp - n.weather.temperature as f64).abs();
        if diff > max_temp_diff { max_temp_diff = diff; }
    }

    let neighbor_storm_avg = if !neighbors.is_empty() {
        neighbor_avg_f64(neighbors, |t| t.weather.storm_intensity as f64)
    } else {
        0.0
    };

    let mut new_storm = current_storm;

    // === PRESSURE-DRIVEN NUCLEATION ===
    let pressure_deficit = 1013.25 - pressure;

    if pressure_deficit > 3.0 && humidity > 0.4 && cloud > 0.35 {
        let mut pressure_factor = (pressure_deficit - 3.0) * 0.015;
        if pressure_factor > 0.4 { pressure_factor = 0.4; }
        let nucleation = pressure_factor * humidity * cloud;
        if nucleation > new_storm {
            new_storm = new_storm + (nucleation - new_storm) * 0.3 + rng.rand_range(0.0, 0.03);
        }
    }

    // === FORMATION (secondary mechanisms) ===

    // 1. Frontal storms
    if max_temp_diff > 5.0 && humidity > 0.45 && cloud > 0.4 {
        let mut frontal = (max_temp_diff - 5.0) * 0.02 * humidity * cloud;
        if frontal > 0.3 { frontal = 0.3; }
        if frontal > new_storm {
            new_storm = new_storm + (frontal - new_storm) * 0.25 + rng.rand_range(0.0, 0.03);
        }
    }

    // 2. Convective storms
    if temp > 295.0 && humidity > 0.55 && cloud > 0.55 {
        let mut convective = (temp - 295.0) * 0.006 * humidity;
        if convective > 0.2 { convective = 0.2; }
        if convective > new_storm {
            new_storm = new_storm + (convective - new_storm) * 0.25 + rng.rand_range(0.0, 0.02);
        }
    }

    // 3. Orographic storms
    if humidity > 0.45 && (terrain_str == "Mountains" || terrain_str == "Cliffs") && cloud > 0.45 {
        let oro_storm = humidity * 0.08 * cloud;
        if oro_storm > new_storm {
            new_storm = new_storm + (oro_storm - new_storm) * 0.2;
        }
    }

    // 4. Coastal convergence
    if terrain_str == "Coast" && humidity > 0.50 && cloud > 0.45 {
        let coast_storm = humidity * 0.05 * cloud;
        if coast_storm > 0.08 && coast_storm > new_storm * 0.8 {
            new_storm += coast_storm * 0.15;
        }
    }

    // === NEIGHBOR SPREADING ===
    if neighbor_storm_avg > 0.15 {
        let spread = neighbor_storm_avg * 0.08;
        if spread > 0.02 {
            new_storm += spread;
        }
    }

    if neighbor_storm_avg > 0.1 {
        new_storm += neighbor_storm_avg * 0.01;
    }

    // === INTENSIFICATION ===
    if new_storm > 0.1 && humidity > 0.4 {
        let fuel = (humidity - 0.4) * 0.025;
        let pressure_boost = if pressure_deficit > 5.0 { 1.0 + pressure_deficit * 0.01 } else { 1.0 };
        let headroom = 1.0 - new_storm;
        let intensification = fuel * headroom * pressure_boost;
        new_storm += intensification;
    }

    // === DECAY ===
    let mut decay_rate = match terrain_str {
        "Ocean" => if temp > 293.0 { 0.025 } else { 0.045 },
        "Coast" => 0.045,
        "Mountains" | "Cliffs" => 0.08,
        "Hills" => 0.06,
        _ => 0.05,
    };

    // High pressure suppresses storms
    if pressure_deficit < -3.0 {
        let mut high_pressure_decay = (-pressure_deficit - 3.0) * 0.01;
        if high_pressure_decay > 0.05 { high_pressure_decay = 0.05; }
        decay_rate += high_pressure_decay;
    }

    // Low humidity starvation
    if humidity < 0.3 {
        let starvation = (0.3 - humidity) * 0.15;
        new_storm *= 1.0 - decay_rate - starvation;
    } else {
        new_storm *= 1.0 - decay_rate;
    }

    if new_storm < 0.03 { new_storm = 0.0; }
    if new_storm > 1.0 { new_storm = 1.0; }

    accum.storm_intensity = new_storm;

    // === STORM EFFECTS ON WEATHER ===
    if new_storm > 0.08 {
        // Wind amplification — amplifies Rule 1's computed wind, not stale snapshot
        let mut storm_wind = wind_speed * (1.0 + new_storm * 2.0);
        if storm_wind > 25.0 { storm_wind = 25.0; }
        accum.wind_speed = storm_wind;

        // Cloud darkening — builds on Rule 3's cloud_cover, not stale snapshot
        let mut storm_cloud = accum.cloud_cover + new_storm * 0.5;
        if storm_cloud > 1.0 { storm_cloud = 1.0; }
        accum.cloud_cover = storm_cloud;

        // Cyclonic rotation — rotates Rule 1's wind direction
        let dir = accum.wind_direction;
        let lat = tile.climate.latitude as f64;
        let coriolis_bias = if lat >= 0.0 { -1.0 } else { 1.0 };
        let rotation = new_storm * (coriolis_bias * 12.0 + rng.rand_range(-8.0, 8.0));
        let mut new_dir = (dir + rotation) % 360.0;
        if new_dir < 0.0 { new_dir += 360.0; }
        accum.wind_direction = new_dir;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::tile::Position;

    fn make_test_tile() -> Tile {
        Tile::new_default(0, vec![1, 2, 3, 4, 5, 6], Position::flat(0.0, 0.0))
    }

    #[test]
    fn native_weather_rng_deterministic() {
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let tile = make_test_tile();

        let result_a = evaluator.evaluate(&tile, &[], Season::Spring, 0, 42);
        let result_b = evaluator.evaluate(&tile, &[], Season::Spring, 0, 42);

        // Same seed → same mutations
        assert_eq!(result_a.mutations.len(), result_b.mutations.len());
        for (a, b) in result_a.mutations.iter().zip(result_b.mutations.iter()) {
            assert_eq!(a.0, b.0, "Field name mismatch");
            assert_eq!(
                a.1.as_float().ok(),
                b.1.as_float().ok(),
                "Value mismatch for field {}",
                a.0
            );
        }
    }

    #[test]
    fn native_weather_produces_expected_fields() {
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let tile = make_test_tile();

        let result = evaluator.evaluate(&tile, &[], Season::Summer, 1, 12345);

        let fields: Vec<&str> = result.mutations.iter().map(|(f, _)| f.as_str()).collect();
        assert!(fields.contains(&"wind_direction"), "Missing wind_direction");
        assert!(fields.contains(&"wind_speed"), "Missing wind_speed");
        assert!(fields.contains(&"temperature"), "Missing temperature");
        assert!(fields.contains(&"humidity"), "Missing humidity");
        assert!(fields.contains(&"cloud_cover"), "Missing cloud_cover");
        assert!(fields.contains(&"storm_intensity"), "Missing storm_intensity");
        assert!(fields.contains(&"precipitation"), "Missing precipitation");
        assert!(fields.contains(&"precipitation_type"), "Missing precipitation_type");
    }

    #[test]
    fn accum_no_duplicate_mutations() {
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let tile = make_test_tile();

        let result = evaluator.evaluate(&tile, &[], Season::Summer, 1, 99999);

        // WeatherAccum produces exactly 8 mutations, one per field
        assert_eq!(result.mutations.len(), 8, "Expected exactly 8 mutations, got {}", result.mutations.len());

        let mut seen = std::collections::HashSet::new();
        for (field, _) in &result.mutations {
            assert!(seen.insert(field.as_str()), "Duplicate mutation for field: {}", field);
        }
    }

    #[test]
    fn accum_humidity_chain() {
        // Rule 2 computes humidity from temperature+macro; Rule 3 should read
        // that computed value (not stale tile snapshot) for precipitation.
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let mut tile = make_test_tile();

        // Set up conditions for high humidity + precipitation:
        // high macro_humidity, warm temperature, some existing cloud cover
        tile.weather.macro_humidity = 0.9;
        tile.climate.base_temperature = 295.0;
        tile.weather.temperature = 295.0;
        tile.weather.humidity = 0.7;
        tile.weather.cloud_cover = 0.5;

        let result = evaluator.evaluate(&tile, &[], Season::Summer, 1, 42);

        // Find humidity and precipitation in mutations
        let humidity_val = result.mutations.iter()
            .find(|(f, _)| f == "humidity")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("humidity mutation missing");
        let precip_val = result.mutations.iter()
            .find(|(f, _)| f == "precipitation")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("precipitation mutation missing");

        // With high macro_humidity, there should be meaningful humidity
        assert!(humidity_val > 0.0, "Humidity should be positive, got {}", humidity_val);

        // If precipitation occurred, humidity should reflect consumption
        // (Rule 3 reads Rule 2's humidity, then subtracts consumed moisture)
        if precip_val > 0.0 {
            // The final humidity should be less than what Rule 2 would have
            // produced alone, because Rule 3 consumed some
            assert!(humidity_val < 0.9, "Humidity should be reduced by precipitation consumption, got {}", humidity_val);
        }
    }

    #[test]
    fn accum_storm_reads_fresh_cloud() {
        // Rule 3 builds cloud_cover from humidity; Rule 4 should see that
        // fresh value when checking nucleation thresholds (cloud > 0.35).
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let mut tile = make_test_tile();

        // Start with zero cloud cover but high humidity + low pressure
        // so Rule 3 will build clouds, and Rule 4 can use them for nucleation.
        tile.weather.cloud_cover = 0.0;
        tile.weather.humidity = 0.8;
        tile.weather.macro_humidity = 0.85;
        tile.weather.pressure = 1000.0; // low pressure (deficit ~13)
        tile.climate.base_temperature = 290.0;
        tile.weather.temperature = 290.0;

        let result = evaluator.evaluate(&tile, &[], Season::Summer, 1, 42);

        let cloud_val = result.mutations.iter()
            .find(|(f, _)| f == "cloud_cover")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("cloud_cover mutation missing");

        // Rule 3 should have built some cloud cover from the high humidity
        // (even though tile started at 0.0, the humidity-driven target is nonzero)
        assert!(cloud_val > 0.0, "Cloud cover should be built from humidity, got {}", cloud_val);
    }

    #[test]
    fn accum_storm_amplifies_rule1_wind() {
        // Rule 1 computes wind; Rule 4 should amplify that computed wind
        // (not the tile snapshot's wind_speed) during storms.
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let mut tile = make_test_tile();

        // Set up an active storm with conditions that keep it alive
        tile.weather.storm_intensity = 0.5;
        tile.weather.wind_speed = 0.0; // stale snapshot has zero wind
        tile.weather.humidity = 0.7;
        tile.weather.macro_humidity = 0.7;
        tile.weather.cloud_cover = 0.6;
        tile.weather.pressure = 1000.0;
        tile.climate.base_temperature = 290.0;
        tile.weather.temperature = 290.0;
        // Give macro wind so Rule 1 computes nonzero wind
        tile.weather.macro_wind_speed = 8.0;
        tile.weather.macro_wind_direction = 180.0;

        let result = evaluator.evaluate(&tile, &[], Season::Summer, 1, 42);

        let wind_val = result.mutations.iter()
            .find(|(f, _)| f == "wind_speed")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("wind_speed mutation missing");
        let storm_val = result.mutations.iter()
            .find(|(f, _)| f == "storm_intensity")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("storm_intensity mutation missing");

        // If storm is active, wind should be amplified above Rule 1's base output.
        // Rule 1 would produce ~3-5 m/s from macro_wind_speed=8 with friction.
        // Storm amplification multiplies by (1 + storm * 2), so with storm~0.4
        // the wind should be noticeably higher than the ~3-5 base.
        if storm_val > 0.08 {
            // With storm amplification, wind should be > what zero-wind would give
            assert!(wind_val > 1.0,
                "Storm should amplify Rule 1's computed wind (not stale 0.0), got wind={} storm={}",
                wind_val, storm_val);
        }
    }

    #[test]
    fn test_humidity_stable_without_macro() {
        // An inland tile with moderate soil/vegetation and NO macro coverage
        // should maintain humidity within +/-10% per tick (not crash to 0).
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let mut tile = make_test_tile();

        // Inland tile: no macro coverage, moderate conditions
        tile.weather.macro_humidity = 0.0;
        tile.weather.macro_wind_speed = 0.0;
        tile.weather.humidity = 0.30;
        tile.conditions.soil_moisture = 0.4;
        tile.biome.vegetation_density = 0.5;
        tile.biome.vegetation_health = 0.8;
        tile.climate.base_temperature = 290.0;
        tile.weather.temperature = 290.0;

        let result = evaluator.evaluate(&tile, &[], Season::Summer, 1, 42);

        let humidity_val = result.mutations.iter()
            .find(|(f, _)| f == "humidity")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("humidity mutation missing");

        // Without macro coverage, humidity should stay roughly stable
        // Old formula: 0.30 -> ~0.08 (catastrophic 73% loss)
        // New formula: 0.30 -> ~0.29-0.35 (stable with evapotranspiration)
        assert!(humidity_val > 0.20,
            "Humidity should not crash without macro coverage: started at 0.30, got {}",
            humidity_val);
        assert!(humidity_val < 0.50,
            "Humidity should not spike unreasonably: started at 0.30, got {}",
            humidity_val);
    }

    #[test]
    fn test_evapotranspiration_scales_with_vegetation() {
        let evaluator = NativeWeatherEvaluator::new(&[]);

        // Tile with dense, healthy forest
        let mut forest_tile = make_test_tile();
        forest_tile.weather.macro_humidity = 0.0;
        forest_tile.weather.humidity = 0.20;
        forest_tile.conditions.soil_moisture = 0.5;
        forest_tile.biome.vegetation_density = 0.9;
        forest_tile.biome.vegetation_health = 1.0;
        forest_tile.climate.base_temperature = 295.0;
        forest_tile.weather.temperature = 295.0;

        // Tile with bare ground
        let mut bare_tile = make_test_tile();
        bare_tile.weather.macro_humidity = 0.0;
        bare_tile.weather.humidity = 0.20;
        bare_tile.conditions.soil_moisture = 0.5;
        bare_tile.biome.vegetation_density = 0.0;
        bare_tile.biome.vegetation_health = 0.0;
        bare_tile.climate.base_temperature = 295.0;
        bare_tile.weather.temperature = 295.0;

        let forest_result = evaluator.evaluate(&forest_tile, &[], Season::Summer, 1, 42);
        let bare_result = evaluator.evaluate(&bare_tile, &[], Season::Summer, 1, 42);

        let forest_h = forest_result.mutations.iter()
            .find(|(f, _)| f == "humidity")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("humidity mutation missing");
        let bare_h = bare_result.mutations.iter()
            .find(|(f, _)| f == "humidity")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("humidity mutation missing");

        // Forest should produce significantly more evapotranspiration
        assert!(forest_h > bare_h,
            "Forest (veg=0.9) should produce more humidity than bare ground (veg=0.0): forest={}, bare={}",
            forest_h, bare_h);
    }

    #[test]
    fn test_precipitation_sustains_humidity() {
        // Heavy rain at humidity 0.5 should not drain humidity below 0.35
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let mut tile = make_test_tile();

        // Set up conditions for heavy precipitation
        tile.weather.macro_humidity = 0.6;
        tile.weather.humidity = 0.5;
        tile.weather.cloud_cover = 0.7;
        tile.climate.base_temperature = 295.0;
        tile.weather.temperature = 295.0;
        tile.conditions.soil_moisture = 0.5;

        let result = evaluator.evaluate(&tile, &[], Season::Summer, 1, 42);

        let humidity_val = result.mutations.iter()
            .find(|(f, _)| f == "humidity")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("humidity mutation missing");
        let precip_val = result.mutations.iter()
            .find(|(f, _)| f == "precipitation")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("precipitation mutation missing");

        if precip_val > 0.1 {
            // With the new scaled consumption, rain shouldn't drain humidity catastrophically
            assert!(humidity_val > 0.25,
                "After heavy rain (intensity={}), humidity should stay above 0.25, got {}",
                precip_val, humidity_val);
        }
    }

    // === Advection tests (geodesic tiles with real lat/lon) ===

    fn make_geo_tile(id: u32, lat: f64, lon: f64, neighbors: Vec<u32>) -> Tile {
        let mut t = Tile::new_default(id, neighbors, Position {
            x: lat.to_radians().cos() * lon.to_radians().cos(),
            y: lat.to_radians().cos() * lon.to_radians().sin(),
            z: lat.to_radians().sin(),
            lat,
            lon,
        });
        t.climate.latitude = lat as f32;
        t
    }

    #[test]
    fn test_humidity_advects_downwind() {
        // Tile 0 (humid, upwind) should push humidity toward tile 1 (dry, downwind).
        // Tile 0 is west of tile 1, with eastward wind (bearing ~90°).
        let mut t0 = make_geo_tile(0, 0.0, 0.0, vec![1]);
        t0.weather.humidity = 0.8;
        t0.weather.wind_direction = 90.0; // blowing east
        t0.weather.wind_speed = 8.0;
        t0.weather.macro_humidity = 0.0;

        let mut t1 = make_geo_tile(1, 0.0, 5.0, vec![0]);
        t1.weather.humidity = 0.1;
        t1.weather.wind_direction = 90.0;
        t1.weather.wind_speed = 8.0;
        t1.weather.macro_humidity = 0.0;
        t1.conditions.soil_moisture = 0.3;
        t1.biome.vegetation_density = 0.3;
        t1.biome.vegetation_health = 0.5;

        let tiles = vec![t0.clone(), t1.clone()];
        let evaluator = NativeWeatherEvaluator::new(&tiles);

        // Evaluate tile 1 with tile 0 as neighbor
        let result = evaluator.evaluate(&t1, &[&t0], Season::Summer, 1, 42);

        let humidity_val = result.mutations.iter()
            .find(|(f, _)| f == "humidity")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("humidity mutation missing");

        // Tile 1 should gain humidity from upwind tile 0
        assert!(humidity_val > 0.1,
            "Downwind tile should gain humidity from upwind neighbor: started at 0.1, got {}",
            humidity_val);
    }

    #[test]
    fn test_no_advection_crosswind() {
        // With multiple neighbors, wind direction should determine which neighbor's
        // humidity dominates. Tile 2 (target) has two neighbors:
        //   - Tile 0 (west, humid=0.8, wind blowing east toward tile 2)
        //   - Tile 1 (south, dry=0.05, wind blowing east — crosswind to tile 2)
        // Isotropic would average both neighbors equally. Wind-directed advection
        // should weight tile 0 heavily (upwind) and tile 1 weakly (crosswind).
        let mut t0 = make_geo_tile(0, 0.0, -5.0, vec![2]); // west of t2
        t0.weather.humidity = 0.8;
        t0.weather.wind_direction = 90.0; // blowing east toward t2
        t0.weather.wind_speed = 10.0;

        let mut t1 = make_geo_tile(1, -5.0, 0.0, vec![2]); // south of t2
        t1.weather.humidity = 0.05;
        t1.weather.wind_direction = 90.0; // blowing east — crosswind relative to t2
        t1.weather.wind_speed = 10.0;

        let mut t2 = make_geo_tile(2, 0.0, 0.0, vec![0, 1]); // target tile
        t2.weather.humidity = 0.1;
        t2.weather.macro_humidity = 0.0;
        t2.conditions.soil_moisture = 0.3;
        t2.biome.vegetation_density = 0.3;
        t2.biome.vegetation_health = 0.5;

        let tiles = vec![t0.clone(), t1.clone(), t2.clone()];
        let evaluator = NativeWeatherEvaluator::new(&tiles);

        let result = evaluator.evaluate(&t2, &[&t0, &t1], Season::Summer, 1, 42);

        let h_val = result.mutations.iter()
            .find(|(f, _)| f == "humidity")
            .and_then(|(_, v)| v.as_float().ok())
            .unwrap();

        // Isotropic average of neighbors = (0.8 + 0.05) / 2 = 0.425
        // Wind-directed should weight tile 0 (upwind, humid) much more heavily.
        // With 0.75 self-retention of 0.1 = 0.075, neighbor contribution via advection
        // should be dominated by tile 0's 0.8 humidity.
        // Result should be noticeably above what isotropic-only would give.
        let isotropic_neighbor_avg = (0.8 + 0.05) / 2.0;
        let isotropic_blend = 0.1 * 0.75 + isotropic_neighbor_avg * 0.20; // ~0.16

        // With advection, neighbor blend should favor tile 0's 0.8 over the 0.425 average
        assert!(h_val > isotropic_blend,
            "Wind-directed humidity ({}) should exceed pure isotropic blend ({}) because upwind neighbor is humid",
            h_val, isotropic_blend);
    }

    #[test]
    fn test_cloud_clearing_from_precipitation() {
        // Heavy precipitation should reduce cloud cover via the clearing mechanism.
        let evaluator = NativeWeatherEvaluator::new(&[]);
        let mut tile = make_test_tile();

        // Set up conditions for heavy precipitation
        tile.weather.macro_humidity = 0.9;
        tile.weather.humidity = 0.9;
        tile.weather.cloud_cover = 0.8;
        tile.climate.base_temperature = 295.0;
        tile.weather.temperature = 295.0;

        let result = evaluator.evaluate(&tile, &[], Season::Summer, 1, 42);

        let cloud_val = result.mutations.iter()
            .find(|(f, _)| f == "cloud_cover")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("cloud_cover mutation missing");
        let precip_val = result.mutations.iter()
            .find(|(f, _)| f == "precipitation")
            .and_then(|(_, v)| v.as_float().ok())
            .expect("precipitation mutation missing");

        if precip_val > 0.3 {
            // With heavy rain, cloud clearing should prevent cloud from staying at max
            // cloud_clearing = intensity^2 * 0.20, e.g., 0.6^2 * 0.2 = 0.072
            assert!(cloud_val < 0.8,
                "Heavy precipitation ({}) should clear some clouds, but cloud_cover={}",
                precip_val, cloud_val);
        }
    }

    #[test]
    fn test_convergence_zones_latitude_pattern() {
        // Equatorial tile should get more cloud cover than subtropical tile (30°).
        let mut equatorial = make_test_tile();
        equatorial.climate.latitude = 5.0;
        equatorial.weather.humidity = 0.5;
        equatorial.weather.macro_humidity = 0.3;
        equatorial.climate.base_temperature = 300.0;
        equatorial.weather.temperature = 300.0;
        equatorial.weather.cloud_cover = 0.3;
        equatorial.conditions.soil_moisture = 0.5;

        let mut subtropical = make_test_tile();
        subtropical.climate.latitude = 30.0;
        subtropical.weather.humidity = 0.5;
        subtropical.weather.macro_humidity = 0.3;
        subtropical.climate.base_temperature = 300.0;
        subtropical.weather.temperature = 300.0;
        subtropical.weather.cloud_cover = 0.3;
        subtropical.conditions.soil_moisture = 0.5;

        let evaluator = NativeWeatherEvaluator::new(&[]);

        let eq_result = evaluator.evaluate(&equatorial, &[], Season::Summer, 1, 42);
        let st_result = evaluator.evaluate(&subtropical, &[], Season::Summer, 1, 42);

        let eq_cloud = eq_result.mutations.iter()
            .find(|(f, _)| f == "cloud_cover")
            .and_then(|(_, v)| v.as_float().ok())
            .unwrap();
        let st_cloud = st_result.mutations.iter()
            .find(|(f, _)| f == "cloud_cover")
            .and_then(|(_, v)| v.as_float().ok())
            .unwrap();

        // ITCZ (equatorial) rising air → more clouds
        // Subtropical subsidence → fewer clouds
        assert!(eq_cloud > st_cloud,
            "Equatorial cloud ({}) should exceed subtropical cloud ({}) due to convergence zones",
            eq_cloud, st_cloud);
    }

    #[test]
    fn test_advection_weight_alignment() {
        // Wind blowing directly toward target (alignment=1) should give max weight
        let w_aligned = advection_weight(90.0, 90.0, 10.0);
        assert!(w_aligned > 0.9, "Perfectly aligned wind should give high weight, got {}", w_aligned);

        // Wind blowing perpendicular (alignment=0) should give zero
        let w_perp = advection_weight(0.0, 90.0, 10.0);
        assert!(w_perp < 0.01, "Perpendicular wind should give ~0 weight, got {}", w_perp);

        // Wind blowing opposite (alignment=-1) should give zero
        let w_opp = advection_weight(270.0, 90.0, 10.0);
        assert!(w_opp < 0.01, "Opposite wind should give 0 weight, got {}", w_opp);
    }

    #[test]
    fn test_convergence_vertical_motion_values() {
        // ITCZ (equator) should have positive (rising)
        let itcz = convergence_vertical_motion(0.0, Season::Spring);
        assert!(itcz > 0.0, "ITCZ should have rising motion, got {}", itcz);

        // Subtropical (~30°) should have negative (sinking)
        let subtropical = convergence_vertical_motion(30.0, Season::Spring);
        assert!(subtropical < 0.0, "Subtropical should have sinking motion, got {}", subtropical);

        // Polar front (~60°) should have positive (rising)
        let polar_front = convergence_vertical_motion(60.0, Season::Spring);
        assert!(polar_front > 0.0, "Polar front should have rising motion, got {}", polar_front);

        // Polar cap (>75°) should have negative (sinking)
        let polar = convergence_vertical_motion(80.0, Season::Spring);
        assert!(polar < 0.0, "Polar cap should have sinking motion, got {}", polar);
    }

    #[test]
    fn test_neighbor_bearings_flat_hex_fallback() {
        // Flat hex tiles (lat=0, lon=0) should set has_geo=false
        let tiles = vec![
            Tile::new_default(0, vec![1], Position::flat(0.0, 0.0)),
            Tile::new_default(1, vec![0], Position::flat(1.0, 0.0)),
        ];
        let bearings = NeighborBearings::from_tiles(&tiles);
        assert!(!bearings.has_geo, "Flat hex tiles should not have geo bearings");
    }

    #[test]
    fn test_neighbor_bearings_geodesic() {
        // Geodesic tiles with real lat/lon should compute bearings
        let tiles = vec![
            make_geo_tile(0, 0.0, 0.0, vec![1]),
            make_geo_tile(1, 0.0, 5.0, vec![0]),
        ];
        let bearings = NeighborBearings::from_tiles(&tiles);
        assert!(bearings.has_geo, "Geodesic tiles should have geo bearings");

        // Bearing from tile 0 to tile 1 should be roughly east (~90°)
        let b = bearings.bearing(1, 0); // bearing from tile 0 (neighbor idx 0 of tile 1) toward tile 1
        // Actually: bearing from neighbor 0 of tile 1 (which is tile 0) toward tile 1
        // Tile 0 is at (0,0), tile 1 is at (0,5) — east. So bearing from t0 to t1 is ~90°
        // But reverse_bearings[1][0] = bearing from t0 TO t1 = ~90°
        assert!((b - 90.0).abs() < 5.0,
            "Bearing from (0,0) to (0,5) should be ~90°, got {}", b);
    }
}
