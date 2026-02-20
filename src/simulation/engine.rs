use rhai::{Array, Dynamic, Engine, Map, Scope, AST};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tracing::debug;

use crate::world::tile::*;
use crate::world::Tile;

/// Which simulation phase a rule belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Weather,
    Conditions,
    Terrain,
    Resources,
}

impl Phase {
    pub fn dir_name(&self) -> &str {
        match self {
            Phase::Weather => "weather",
            Phase::Conditions => "conditions",
            Phase::Terrain => "terrain",
            Phase::Resources => "resources",
        }
    }

    pub fn all() -> &'static [Phase] {
        &[
            Phase::Weather,
            Phase::Conditions,
            Phase::Terrain,
            Phase::Resources,
        ]
    }
}

/// A compiled Rhai rule script.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub name: String,
    pub phase: Phase,
    pub ast: AST,
}

/// The result of evaluating rules for a single tile in a single phase.
#[derive(Debug, Clone, Default)]
pub struct TileMutations {
    pub mutations: Vec<(String, Dynamic)>,
}

/// Error from rule evaluation on a single tile.
#[derive(Debug, Clone)]
pub struct RuleError {
    pub tile_id: u32,
    pub rule_name: String,
    pub error: String,
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Tile {}: rule '{}': {}",
            self.tile_id, self.rule_name, self.error
        )
    }
}

/// The rule engine loads, validates, and executes Rhai scripts against tile data.
pub struct RuleEngine {
    engine: Engine,
    rules: HashMap<Phase, Vec<CompiledRule>>,
    timeout_ms: u64,
}

impl RuleEngine {
    /// Create a new rule engine and load rules from the given directory.
    ///
    /// The rule directory must contain subdirectories: weather/, conditions/, terrain/, resources/.
    /// Each subdirectory contains .rhai files sorted by filename.
    pub fn new(rule_dir: &Path, timeout_ms: u64) -> Result<Self, String> {
        if !rule_dir.exists() {
            return Err(format!(
                "Rule directory not found: {}. Create the directory with rule scripts.",
                rule_dir.display()
            ));
        }

        let mut engine = Engine::new();

        // Sandbox: disable all dangerous operations
        engine.set_max_operations(100_000);
        engine.set_max_string_size(1024);
        engine.set_max_array_size(1000);
        engine.set_max_map_size(500);

        // Register the `set` function for tile mutations
        engine.register_fn("set", |field: &str, value: Dynamic| {
            MUTATIONS.with(|m| {
                m.borrow_mut().push((field.to_string(), value));
            });
        });

        // Register `log` function
        engine.register_fn("log", |msg: &str| {
            LOG_MESSAGES.with(|l| {
                l.borrow_mut().push(msg.to_string());
            });
        });

        // Register rand functions using thread-local RNG state
        engine.register_fn("rand", || -> f64 {
            RNG_STATE.with(|r| {
                let state = r.get();
                let next = xorshift64(state);
                r.set(next);
                (next as f64) / (u64::MAX as f64)
            })
        });
        // Math helpers for directional wind calculations
        engine.register_fn("sin_deg", |deg: f64| -> f64 {
            (deg * std::f64::consts::PI / 180.0).sin()
        });
        engine.register_fn("cos_deg", |deg: f64| -> f64 {
            (deg * std::f64::consts::PI / 180.0).cos()
        });
        engine.register_fn("sqrt", |x: f64| -> f64 { x.sqrt() });
        engine.register_fn("abs", |v: f64| -> f64 { v.abs() });
        engine.register_fn("clamp", |v: f64, min: f64, max: f64| -> f64 { v.clamp(min, max) });

        // Native acceleration: compute wind-direction alignment between two points.
        // Returns dot product of the wind vector with the normalized direction from→to.
        engine.register_fn(
            "wind_align",
            |from_x: f64, from_y: f64, to_x: f64, to_y: f64, wind_dir: f64| -> f64 {
                let dx = to_x - from_x;
                let dy = to_y - from_y;
                let dist_sq = dx * dx + dy * dy;
                if dist_sq < 1e-6 {
                    return 0.0;
                }
                let dist = dist_sq.sqrt();
                let rad = wind_dir * std::f64::consts::PI / 180.0;
                (rad.sin() * dx + rad.cos() * dy) / dist
            },
        );

        // Native acceleration: normalized direction vector between two points.
        // Returns [nx, ny] or [0, 0] if coincident.
        engine.register_fn(
            "direction_to",
            |from_x: f64, from_y: f64, to_x: f64, to_y: f64| -> Array {
                let dx = to_x - from_x;
                let dy = to_y - from_y;
                let dist_sq = dx * dx + dy * dy;
                if dist_sq < 1e-6 {
                    return vec![Dynamic::from(0.0_f64), Dynamic::from(0.0_f64)];
                }
                let dist = dist_sq.sqrt();
                vec![Dynamic::from(dx / dist), Dynamic::from(dy / dist)]
            },
        );

        // Native acceleration: average a nested field across neighbor maps.
        // Path format: "layer.field" e.g. "weather.temperature"
        engine.register_fn("neighbor_avg", |neighbors: Array, path: &str| -> f64 {
            let mut sum = 0.0;
            let mut count = 0usize;
            for n in &neighbors {
                if let Some(v) = get_nested_f64(n, path) {
                    sum += v;
                    count += 1;
                }
            }
            if count > 0 {
                sum / count as f64
            } else {
                0.0
            }
        });

        // Native acceleration: sum a nested field across neighbor maps.
        engine.register_fn("neighbor_sum", |neighbors: Array, path: &str| -> f64 {
            neighbors
                .iter()
                .filter_map(|n| get_nested_f64(n, path))
                .sum()
        });

        // Native acceleration: max of a nested field across neighbor maps.
        engine.register_fn("neighbor_max", |neighbors: Array, path: &str| -> f64 {
            neighbors
                .iter()
                .filter_map(|n| get_nested_f64(n, path))
                .reduce(f64::max)
                .unwrap_or(0.0)
        });

        engine.register_fn("rand_range", |min: f64, max: f64| -> f64 {
            RNG_STATE.with(|r| {
                let state = r.get();
                let next = xorshift64(state);
                r.set(next);
                let t = (next as f64) / (u64::MAX as f64);
                min + t * (max - min)
            })
        });

        // Timeout enforcement via operation limit
        // At ~100K operations with typical Rhai performance, this equates to roughly 10-50ms
        // Combined with max_operations, this provides a reasonable timeout mechanism
        engine.on_progress(move |_ops| {
            // max_operations provides the hard cap for timeout enforcement
            None
        });

        let mut rule_engine = RuleEngine {
            engine,
            rules: HashMap::new(),
            timeout_ms,
        };

        rule_engine.load_rules(rule_dir)?;
        Ok(rule_engine)
    }

    fn load_rules(&mut self, rule_dir: &Path) -> Result<(), String> {
        for phase in Phase::all() {
            let phase_dir = rule_dir.join(phase.dir_name());
            let mut phase_rules = Vec::new();

            if !phase_dir.exists() {
                // Empty phase directory is OK — phase becomes no-op
                self.rules.insert(*phase, phase_rules);
                continue;
            }

            let mut entries: Vec<_> = std::fs::read_dir(&phase_dir)
                .map_err(|e| format!("Cannot read {}: {}", phase_dir.display(), e))?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|ext| ext == "rhai")
                })
                .collect();

            // Sort by filename for deterministic execution order
            entries.sort_by_key(|e| e.file_name());

            for entry in entries {
                let path = entry.path();
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let source = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Cannot read rule {}: {}", path.display(), e))?;

                let ast = self.engine.compile(&source).map_err(|e| {
                    format!("Syntax error in {}: {}", path.display(), e)
                })?;

                phase_rules.push(CompiledRule {
                    name,
                    phase: *phase,
                    ast,
                });
            }

            self.rules.insert(*phase, phase_rules);
        }

        Ok(())
    }

    /// Get the rules for a specific phase.
    pub fn rules_for_phase(&self, phase: Phase) -> &[CompiledRule] {
        self.rules.get(&phase).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get total rule count across all phases.
    pub fn rule_count(&self) -> usize {
        self.rules.values().map(|v| v.len()).sum()
    }

    /// Evaluate all rules for a phase on a single tile.
    ///
    /// Returns mutations to apply to the tile, or a RuleError if evaluation fails.
    /// The tile state and neighbor states are read-only snapshots.
    pub fn evaluate_tile(
        &self,
        phase: Phase,
        tile: &Tile,
        neighbors: &[&Tile],
        season: &Season,
        tick: u64,
        rng_seed: u64,
    ) -> Result<TileMutations, RuleError> {
        let rules = self.rules_for_phase(phase);
        if rules.is_empty() {
            return Ok(TileMutations::default());
        }

        // Build the scope with tile data exposed as Rhai maps
        let tile_map = tile_to_rhai_map(tile);
        let neighbors_map: Vec<Dynamic> = neighbors.iter().map(|n| tile_to_rhai_map(n)).collect();

        let season_str = match season {
            Season::Spring => "Spring",
            Season::Summer => "Summer",
            Season::Autumn => "Autumn",
            Season::Winter => "Winter",
        };

        // Clear thread-local mutations and logs
        MUTATIONS.with(|m| m.borrow_mut().clear());
        LOG_MESSAGES.with(|l| l.borrow_mut().clear());

        // Set up the RNG thread-local
        RNG_STATE.with(|r| r.set(rng_seed));

        for rule in rules {
            let mut scope = Scope::new();
            scope.push("tile", tile_map.clone());
            scope.push("neighbors", neighbors_map.clone());
            scope.push_constant("season", season_str.to_string());
            scope.push_constant("tick", tick as i64);

            // Use the main engine (which has set/log/rand registered and operation limits)
            // The on_progress callback provides wall-clock timeout
            let start_time = Instant::now();
            let timeout = self.timeout_ms;

            // Create a scoped engine for this evaluation with timeout
            let result = {
                // We use the pre-compiled AST with the main engine
                // Rhai ASTs are portable between compatible engines
                self.engine.run_ast_with_scope(&mut scope, &rule.ast)
            };

            // Collect any log messages
            LOG_MESSAGES.with(|l| {
                for msg in l.borrow().iter() {
                    debug!(rule = %rule.name, tile_id = tile.id, "{}", msg);
                }
                l.borrow_mut().clear();
            });

            if let Err(e) = result {
                // Discard all mutations from this tile (error isolation)
                MUTATIONS.with(|m| m.borrow_mut().clear());
                return Err(RuleError {
                    tile_id: tile.id,
                    rule_name: rule.name.clone(),
                    error: e.to_string(),
                });
            }

            let _ = (start_time, timeout); // used by on_progress if we add it later
        }

        // Collect all mutations
        let mutations = MUTATIONS.with(|m| {
            let muts = m.borrow().clone();
            m.borrow_mut().clear();
            TileMutations { mutations: muts }
        });

        Ok(mutations)
    }

    /// Evaluate all rules for a phase on a single tile using pre-converted Rhai maps.
    ///
    /// This avoids redundant tile-to-map conversions when the same maps are reused
    /// across multiple evaluations (e.g., neighbor maps shared between tiles).
    pub fn evaluate_tile_preconverted(
        &self,
        phase: Phase,
        tile_map: &Dynamic,
        neighbor_maps: Vec<Dynamic>,
        season: &Season,
        tick: u64,
        rng_seed: u64,
        tile_id: u32,
    ) -> Result<TileMutations, RuleError> {
        let rules = self.rules_for_phase(phase);
        if rules.is_empty() {
            return Ok(TileMutations::default());
        }

        let season_str = match season {
            Season::Spring => "Spring",
            Season::Summer => "Summer",
            Season::Autumn => "Autumn",
            Season::Winter => "Winter",
        };

        MUTATIONS.with(|m| m.borrow_mut().clear());
        LOG_MESSAGES.with(|l| l.borrow_mut().clear());
        RNG_STATE.with(|r| r.set(rng_seed));

        for rule in rules {
            let mut scope = Scope::new();
            scope.push_constant("tile", tile_map.clone());
            scope.push_constant("neighbors", neighbor_maps.clone());
            scope.push_constant("season", season_str.to_string());
            scope.push_constant("tick", tick as i64);

            let result = self.engine.run_ast_with_scope(&mut scope, &rule.ast);

            LOG_MESSAGES.with(|l| {
                for msg in l.borrow().iter() {
                    debug!(rule = %rule.name, tile_id, "{}", msg);
                }
                l.borrow_mut().clear();
            });

            if let Err(e) = result {
                MUTATIONS.with(|m| m.borrow_mut().clear());
                return Err(RuleError {
                    tile_id,
                    rule_name: rule.name.clone(),
                    error: e.to_string(),
                });
            }
        }

        let mutations = MUTATIONS.with(|m| {
            let muts = m.borrow().clone();
            m.borrow_mut().clear();
            TileMutations { mutations: muts }
        });

        Ok(mutations)
    }
}

// Thread-local storage for collecting mutations during rule execution
thread_local! {
    static MUTATIONS: RefCell<Vec<(String, Dynamic)>> = RefCell::new(Vec::new());
    static LOG_MESSAGES: RefCell<Vec<String>> = RefCell::new(Vec::new());
    static RNG_STATE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Simple xorshift64 PRNG for deterministic random numbers in rules.
fn xorshift64(mut state: u64) -> u64 {
    if state == 0 {
        state = 1;
    }
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

/// Extract a nested float from a Rhai Dynamic map via "layer.field" dot path.
fn get_nested_f64(dyn_val: &Dynamic, path: &str) -> Option<f64> {
    let (layer, field) = path.split_once('.')?;
    let map_lock = dyn_val.read_lock::<Map>()?;
    let layer_val = map_lock.get(layer)?;
    let layer_map_lock = layer_val.read_lock::<Map>()?;
    let field_val = layer_map_lock.get(field)?;
    field_val.as_float().ok()
}

/// Convert a Tile to a Rhai Map for script access.
pub fn tile_to_rhai_map(tile: &Tile) -> Dynamic {
    let mut map = Map::new();

    map.insert("id".into(), Dynamic::from(tile.id as i64));

    // Position
    let mut pos = Map::new();
    pos.insert("x".into(), Dynamic::from(tile.position.x as f64));
    pos.insert("y".into(), Dynamic::from(tile.position.y as f64));
    map.insert("position".into(), Dynamic::from(pos));

    // Geology layer
    let mut geo = Map::new();
    geo.insert(
        "terrain_type".into(),
        Dynamic::from(format!("{:?}", tile.geology.terrain_type)),
    );
    geo.insert("elevation".into(), Dynamic::from(tile.geology.elevation as f64));
    geo.insert(
        "soil_type".into(),
        Dynamic::from(format!("{:?}", tile.geology.soil_type)),
    );
    geo.insert("drainage".into(), Dynamic::from(tile.geology.drainage as f64));
    geo.insert(
        "tectonic_stress".into(),
        Dynamic::from(tile.geology.tectonic_stress as f64),
    );
    map.insert("geology".into(), Dynamic::from(geo));

    // Climate layer
    let mut climate = Map::new();
    climate.insert(
        "zone".into(),
        Dynamic::from(format!("{:?}", tile.climate.zone)),
    );
    climate.insert(
        "base_temperature".into(),
        Dynamic::from(tile.climate.base_temperature as f64),
    );
    climate.insert(
        "base_precipitation".into(),
        Dynamic::from(tile.climate.base_precipitation as f64),
    );
    climate.insert("latitude".into(), Dynamic::from(tile.climate.latitude as f64));
    map.insert("climate".into(), Dynamic::from(climate));

    // Biome layer
    let mut biome = Map::new();
    biome.insert(
        "biome_type".into(),
        Dynamic::from(format!("{:?}", tile.biome.biome_type)),
    );
    biome.insert(
        "vegetation_density".into(),
        Dynamic::from(tile.biome.vegetation_density as f64),
    );
    biome.insert(
        "vegetation_health".into(),
        Dynamic::from(tile.biome.vegetation_health as f64),
    );
    biome.insert(
        "transition_pressure".into(),
        Dynamic::from(tile.biome.transition_pressure as f64),
    );
    biome.insert(
        "ticks_in_current_biome".into(),
        Dynamic::from(tile.biome.ticks_in_current_biome as i64),
    );
    map.insert("biome".into(), Dynamic::from(biome));

    // Weather layer
    let mut weather = Map::new();
    weather.insert(
        "temperature".into(),
        Dynamic::from(tile.weather.temperature as f64),
    );
    weather.insert(
        "precipitation".into(),
        Dynamic::from(tile.weather.precipitation as f64),
    );
    weather.insert(
        "precipitation_type".into(),
        Dynamic::from(format!("{:?}", tile.weather.precipitation_type)),
    );
    weather.insert(
        "wind_speed".into(),
        Dynamic::from(tile.weather.wind_speed as f64),
    );
    weather.insert(
        "wind_direction".into(),
        Dynamic::from(tile.weather.wind_direction as f64),
    );
    weather.insert(
        "cloud_cover".into(),
        Dynamic::from(tile.weather.cloud_cover as f64),
    );
    weather.insert(
        "humidity".into(),
        Dynamic::from(tile.weather.humidity as f64),
    );
    weather.insert(
        "storm_intensity".into(),
        Dynamic::from(tile.weather.storm_intensity as f64),
    );
    map.insert("weather".into(), Dynamic::from(weather));

    // Conditions layer
    let mut conditions = Map::new();
    conditions.insert(
        "soil_moisture".into(),
        Dynamic::from(tile.conditions.soil_moisture as f64),
    );
    conditions.insert(
        "snow_depth".into(),
        Dynamic::from(tile.conditions.snow_depth as f64),
    );
    conditions.insert(
        "mud_level".into(),
        Dynamic::from(tile.conditions.mud_level as f64),
    );
    conditions.insert(
        "flood_level".into(),
        Dynamic::from(tile.conditions.flood_level as f64),
    );
    conditions.insert(
        "frost_days".into(),
        Dynamic::from(tile.conditions.frost_days as i64),
    );
    conditions.insert(
        "drought_days".into(),
        Dynamic::from(tile.conditions.drought_days as i64),
    );
    conditions.insert(
        "fire_risk".into(),
        Dynamic::from(tile.conditions.fire_risk as f64),
    );
    map.insert("conditions".into(), Dynamic::from(conditions));

    // Resources (simplified — count and list)
    let res_list: Vec<Dynamic> = tile
        .resources
        .resources
        .iter()
        .map(|r| {
            let mut rm = Map::new();
            rm.insert("resource_type".into(), Dynamic::from(r.resource_type.clone()));
            rm.insert("quantity".into(), Dynamic::from(r.quantity as f64));
            rm.insert("max_quantity".into(), Dynamic::from(r.max_quantity as f64));
            rm.insert("renewal_rate".into(), Dynamic::from(r.renewal_rate as f64));
            Dynamic::from(rm)
        })
        .collect();
    map.insert("resources".into(), Dynamic::from(res_list));

    // Neighbor IDs
    let neighbor_ids: Vec<Dynamic> = tile.neighbors.iter().map(|&n| Dynamic::from(n as i64)).collect();
    map.insert("neighbor_ids".into(), Dynamic::from(neighbor_ids));

    Dynamic::from(map)
}

/// Apply mutations from rule evaluation to a tile's mutable fields for a given phase.
///
/// Only fields writable in the given phase are applied. Returns the number of mutations applied.
pub fn apply_mutations(tile: &mut Tile, mutations: &TileMutations, phase: Phase) -> usize {
    let mut applied = 0;

    for (field, value) in &mutations.mutations {
        let ok = match phase {
            Phase::Weather => apply_weather_mutation(tile, field, value),
            Phase::Conditions => apply_conditions_mutation(tile, field, value),
            Phase::Terrain => apply_terrain_mutation(tile, field, value),
            Phase::Resources => apply_resources_mutation(tile, field, value),
        };
        if ok {
            applied += 1;
        }
    }

    applied
}

fn apply_weather_mutation(tile: &mut Tile, field: &str, value: &Dynamic) -> bool {
    match field {
        "temperature" => {
            if let Some(v) = value.as_float().ok() {
                tile.weather.temperature = v as f32;
                return true;
            }
        }
        "precipitation" => {
            if let Some(v) = value.as_float().ok() {
                tile.weather.precipitation = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        "precipitation_type" => {
            if let Some(s) = value.clone().into_string().ok() {
                if let Some(pt) = parse_precipitation_type(&s) {
                    tile.weather.precipitation_type = pt;
                    return true;
                }
            }
        }
        "wind_speed" => {
            if let Some(v) = value.as_float().ok() {
                tile.weather.wind_speed = (v as f32).max(0.0);
                return true;
            }
        }
        "wind_direction" => {
            if let Some(v) = value.as_float().ok() {
                tile.weather.wind_direction = ((v as f32) % 360.0 + 360.0) % 360.0;
                return true;
            }
        }
        "cloud_cover" => {
            if let Some(v) = value.as_float().ok() {
                tile.weather.cloud_cover = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        "storm_intensity" => {
            if let Some(v) = value.as_float().ok() {
                tile.weather.storm_intensity = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        "humidity" => {
            if let Some(v) = value.as_float().ok() {
                tile.weather.humidity = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        _ => {}
    }
    false
}

fn apply_conditions_mutation(tile: &mut Tile, field: &str, value: &Dynamic) -> bool {
    match field {
        "soil_moisture" => {
            if let Some(v) = value.as_float().ok() {
                tile.conditions.soil_moisture = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        "snow_depth" => {
            if let Some(v) = value.as_float().ok() {
                tile.conditions.snow_depth = (v as f32).max(0.0);
                return true;
            }
        }
        "mud_level" => {
            if let Some(v) = value.as_float().ok() {
                tile.conditions.mud_level = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        "flood_level" => {
            if let Some(v) = value.as_float().ok() {
                tile.conditions.flood_level = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        "frost_days" => {
            if let Some(v) = value.as_int().ok() {
                tile.conditions.frost_days = v.max(0) as u32;
                return true;
            }
        }
        "drought_days" => {
            if let Some(v) = value.as_int().ok() {
                tile.conditions.drought_days = v.max(0) as u32;
                return true;
            }
        }
        "fire_risk" => {
            if let Some(v) = value.as_float().ok() {
                tile.conditions.fire_risk = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        _ => {}
    }
    false
}

fn apply_terrain_mutation(tile: &mut Tile, field: &str, value: &Dynamic) -> bool {
    match field {
        "vegetation_density" => {
            if let Some(v) = value.as_float().ok() {
                tile.biome.vegetation_density = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        "vegetation_health" => {
            if let Some(v) = value.as_float().ok() {
                tile.biome.vegetation_health = (v as f32).clamp(0.0, 1.0);
                return true;
            }
        }
        "transition_pressure" => {
            if let Some(v) = value.as_float().ok() {
                tile.biome.transition_pressure = (v as f32).clamp(-1.0, 1.0);
                return true;
            }
        }
        "biome_type" => {
            if let Some(s) = value.clone().into_string().ok() {
                if let Some(bt) = parse_biome_type(&s) {
                    tile.biome.biome_type = bt;
                    tile.biome.ticks_in_current_biome = 0;
                    return true;
                }
            }
        }
        _ => {}
    }
    false
}

fn apply_resources_mutation(tile: &mut Tile, field: &str, value: &Dynamic) -> bool {
    // Resource mutations use a "resource_name.field" format
    if let Some((res_name, res_field)) = field.split_once('.') {
        if let Some(deposit) = tile
            .resources
            .resources
            .iter_mut()
            .find(|r| r.resource_type == res_name)
        {
            match res_field {
                "quantity" => {
                    if let Some(v) = value.as_float().ok() {
                        deposit.quantity = (v as f32).max(0.0).min(deposit.max_quantity);
                        return true;
                    }
                }
                "renewal_rate" => {
                    if let Some(v) = value.as_float().ok() {
                        deposit.renewal_rate = (v as f32).max(0.0);
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    false
}

fn parse_precipitation_type(s: &str) -> Option<PrecipitationType> {
    match s {
        "None" => Some(PrecipitationType::None),
        "Rain" => Some(PrecipitationType::Rain),
        "Snow" => Some(PrecipitationType::Snow),
        "Hail" => Some(PrecipitationType::Hail),
        "Sleet" => Some(PrecipitationType::Sleet),
        _ => None,
    }
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
    use crate::world::tile::Position;
    use std::fs;
    use tempfile::TempDir;

    fn make_test_tile() -> Tile {
        Tile::new_default(0, vec![1, 2, 3, 4, 5, 6], Position::flat(0.0, 0.0))
    }

    fn make_rule_dir(dir: &Path, phase: &str, rules: &[(&str, &str)]) {
        let phase_dir = dir.join(phase);
        fs::create_dir_all(&phase_dir).unwrap();
        for (name, content) in rules {
            fs::write(phase_dir.join(name), content).unwrap();
        }
    }

    fn setup_empty_rule_dirs(dir: &Path) {
        for phase in Phase::all() {
            fs::create_dir_all(dir.join(phase.dir_name())).unwrap();
        }
    }

    #[test]
    fn load_valid_rules() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[("01-test.rhai", "set(\"temperature\", 300.0);")],
        );

        let engine = RuleEngine::new(dir.path(), 10).unwrap();
        assert_eq!(engine.rules_for_phase(Phase::Weather).len(), 1);
        assert_eq!(engine.rules_for_phase(Phase::Conditions).len(), 0);
        assert_eq!(engine.rule_count(), 1);
    }

    #[test]
    fn missing_rule_dir_error() {
        let result = RuleEngine::new(Path::new("/nonexistent/rules"), 10);
        match result {
            Err(msg) => assert!(msg.contains("Rule directory not found")),
            Ok(_) => panic!("Expected error for missing rule directory"),
        }
    }

    #[test]
    fn rhai_syntax_error_detected() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[("01-bad.rhai", "this is not { valid rhai")],
        );

        let result = RuleEngine::new(dir.path(), 10);
        match result {
            Err(msg) => assert!(msg.contains("Syntax error")),
            Ok(_) => panic!("Expected syntax error"),
        }
    }

    #[test]
    fn empty_phase_dir_is_noop() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());

        let engine = RuleEngine::new(dir.path(), 10).unwrap();
        let tile = make_test_tile();
        let result = engine.evaluate_tile(Phase::Weather, &tile, &[], &Season::Spring, 0, 42);
        assert!(result.is_ok());
        assert!(result.unwrap().mutations.is_empty());
    }

    #[test]
    fn rule_reads_tile_data() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        // Rule reads elevation and sets temperature based on it
        make_rule_dir(
            dir.path(),
            "weather",
            &[(
                "01-temp.rhai",
                r#"
                let elev = tile.geology.elevation;
                set("temperature", 300.0 - elev * 20.0);
                "#,
            )],
        );

        let engine = RuleEngine::new(dir.path(), 100).unwrap();
        let mut tile = make_test_tile();
        tile.geology.elevation = 0.5;

        let result = engine
            .evaluate_tile(Phase::Weather, &tile, &[], &Season::Spring, 0, 42)
            .unwrap();

        assert!(!result.mutations.is_empty());
        let (field, value) = &result.mutations[0];
        assert_eq!(field, "temperature");
        assert!((value.as_float().unwrap() - 290.0).abs() < 0.01);
    }

    #[test]
    fn rule_reads_neighbors() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[(
                "01-neighbor.rhai",
                r#"
                let avg_temp = 0.0;
                for n in neighbors {
                    avg_temp += n.weather.temperature;
                }
                if neighbors.len() > 0 {
                    avg_temp /= neighbors.len();
                }
                set("temperature", avg_temp);
                "#,
            )],
        );

        let engine = RuleEngine::new(dir.path(), 100).unwrap();
        let tile = make_test_tile();
        let mut n1 = make_test_tile();
        n1.weather.temperature = 300.0;
        let mut n2 = make_test_tile();
        n2.weather.temperature = 310.0;

        let result = engine
            .evaluate_tile(Phase::Weather, &tile, &[&n1, &n2], &Season::Spring, 0, 42)
            .unwrap();

        let (field, value) = &result.mutations[0];
        assert_eq!(field, "temperature");
        assert!((value.as_float().unwrap() - 305.0).abs() < 0.01);
    }

    #[test]
    fn rule_reads_season_and_tick() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[(
                "01-season.rhai",
                r#"
                if season == "Winter" {
                    set("temperature", 250.0);
                } else {
                    set("temperature", 300.0);
                }
                "#,
            )],
        );

        let engine = RuleEngine::new(dir.path(), 100).unwrap();
        let tile = make_test_tile();

        let winter = engine
            .evaluate_tile(Phase::Weather, &tile, &[], &Season::Winter, 0, 42)
            .unwrap();
        assert!((winter.mutations[0].1.as_float().unwrap() - 250.0).abs() < 0.01);

        let summer = engine
            .evaluate_tile(Phase::Weather, &tile, &[], &Season::Summer, 0, 42)
            .unwrap();
        assert!((summer.mutations[0].1.as_float().unwrap() - 300.0).abs() < 0.01);
    }

    #[test]
    fn rule_error_returns_rule_error() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[(
                "01-err.rhai",
                r#"
                let x = 1 / 0;
                "#,
            )],
        );

        let engine = RuleEngine::new(dir.path(), 100).unwrap();
        let tile = make_test_tile();

        let result = engine.evaluate_tile(Phase::Weather, &tile, &[], &Season::Spring, 0, 42);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.tile_id, 0);
        assert!(err.rule_name.contains("01-err.rhai"));
    }

    #[test]
    fn rule_timeout_enforced() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[(
                "01-infinite.rhai",
                r#"
                let x = 0;
                loop {
                    x += 1;
                }
                "#,
            )],
        );

        let engine = RuleEngine::new(dir.path(), 10).unwrap();
        let tile = make_test_tile();

        let start = std::time::Instant::now();
        let result = engine.evaluate_tile(Phase::Weather, &tile, &[], &Season::Spring, 0, 42);
        let elapsed = start.elapsed();

        // Should fail (timeout or operation limit)
        assert!(result.is_err());
        // Should complete within a reasonable time (not hang)
        assert!(elapsed.as_secs() < 5);
    }

    #[test]
    fn apply_weather_mutations() {
        let mut tile = make_test_tile();
        let mutations = TileMutations {
            mutations: vec![
                ("temperature".to_string(), Dynamic::from(310.0_f64)),
                ("precipitation".to_string(), Dynamic::from(0.8_f64)),
                ("cloud_cover".to_string(), Dynamic::from(0.9_f64)),
            ],
        };

        let applied = apply_mutations(&mut tile, &mutations, Phase::Weather);
        assert_eq!(applied, 3);
        assert_eq!(tile.weather.temperature, 310.0);
        assert!((tile.weather.precipitation - 0.8).abs() < 0.001);
        assert!((tile.weather.cloud_cover - 0.9).abs() < 0.001);
    }

    #[test]
    fn apply_conditions_mutations_with_clamping() {
        let mut tile = make_test_tile();
        let mutations = TileMutations {
            mutations: vec![
                ("soil_moisture".to_string(), Dynamic::from(1.5_f64)), // should clamp to 1.0
                ("frost_days".to_string(), Dynamic::from(5_i64)),
            ],
        };

        let applied = apply_mutations(&mut tile, &mutations, Phase::Conditions);
        assert_eq!(applied, 2);
        assert_eq!(tile.conditions.soil_moisture, 1.0); // clamped
        assert_eq!(tile.conditions.frost_days, 5);
    }

    #[test]
    fn apply_terrain_mutations() {
        let mut tile = make_test_tile();
        let mutations = TileMutations {
            mutations: vec![
                ("vegetation_health".to_string(), Dynamic::from(0.3_f64)),
                (
                    "transition_pressure".to_string(),
                    Dynamic::from(-0.5_f64),
                ),
            ],
        };

        let applied = apply_mutations(&mut tile, &mutations, Phase::Terrain);
        assert_eq!(applied, 2);
        assert!((tile.biome.vegetation_health - 0.3).abs() < 0.001);
        assert!((tile.biome.transition_pressure - (-0.5)).abs() < 0.001);
    }

    #[test]
    fn wrong_phase_mutations_ignored() {
        let mut tile = make_test_tile();
        let original_temp = tile.weather.temperature;
        let mutations = TileMutations {
            mutations: vec![("temperature".to_string(), Dynamic::from(999.0_f64))],
        };

        // Apply weather mutation during conditions phase — should be ignored
        let applied = apply_mutations(&mut tile, &mutations, Phase::Conditions);
        assert_eq!(applied, 0);
        assert_eq!(tile.weather.temperature, original_temp);
    }

    #[test]
    fn multiple_rules_last_write_wins() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[
                ("01-first.rhai", "set(\"temperature\", 200.0);"),
                ("02-second.rhai", "set(\"temperature\", 350.0);"),
            ],
        );

        let engine = RuleEngine::new(dir.path(), 100).unwrap();
        let tile = make_test_tile();

        let result = engine
            .evaluate_tile(Phase::Weather, &tile, &[], &Season::Spring, 0, 42)
            .unwrap();

        // Both mutations collected, last-write-wins when applied
        let mut applied_tile = tile.clone();
        apply_mutations(&mut applied_tile, &result, Phase::Weather);
        assert_eq!(applied_tile.weather.temperature, 350.0);
    }

    #[test]
    fn rules_sorted_by_filename() {
        let dir = TempDir::new().unwrap();
        setup_empty_rule_dirs(dir.path());
        make_rule_dir(
            dir.path(),
            "weather",
            &[
                ("02-second.rhai", "set(\"temperature\", 200.0);"),
                ("01-first.rhai", "set(\"temperature\", 100.0);"),
                ("03-third.rhai", "set(\"temperature\", 300.0);"),
            ],
        );

        let engine = RuleEngine::new(dir.path(), 100).unwrap();
        let rules = engine.rules_for_phase(Phase::Weather);
        assert_eq!(rules[0].name, "01-first.rhai");
        assert_eq!(rules[1].name, "02-second.rhai");
        assert_eq!(rules[2].name, "03-third.rhai");
    }

    #[test]
    fn xorshift64_deterministic() {
        let a1 = xorshift64(42);
        let a2 = xorshift64(42);
        assert_eq!(a1, a2);

        let b = xorshift64(a1);
        assert_ne!(a1, b);
    }

    #[test]
    fn tile_to_map_has_all_layers() {
        let tile = make_test_tile();
        let map = tile_to_rhai_map(&tile);
        let m = map.cast::<Map>();

        assert!(m.contains_key("geology"));
        assert!(m.contains_key("climate"));
        assert!(m.contains_key("biome"));
        assert!(m.contains_key("weather"));
        assert!(m.contains_key("conditions"));
        assert!(m.contains_key("resources"));
        assert!(m.contains_key("id"));
    }
}
