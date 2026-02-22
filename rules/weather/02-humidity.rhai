// Humidity rule: evaporation + macro-driven moisture transport + orographic stripping
//
// Key features:
// - Macro humidity from pressure systems provides primary moisture transport
// - Local evaporation adds moisture from surface
// - Isotropic diffusion for spatial coherence
// - Orographic stripping on mountains/hills themselves
// - Simplified rain shadow from adjacent high terrain

let terrain = tile.geology.terrain_type;
let temp = tile.weather.temperature;
let current_humidity = tile.weather.humidity;
let macro_humidity = tile.weather.macro_humidity;

// === EVAPORATION ===
let temp_factor = (temp - 250.0) / 60.0;
if temp_factor < 0.0 { temp_factor = 0.0; }
if temp_factor > 1.5 { temp_factor = 1.5; }

let evaporation = if terrain == "Ocean" {
    0.08 + temp_factor * 0.12
} else if terrain == "Coast" {
    0.05 + temp_factor * 0.08
} else if terrain == "Wetlands" {
    0.04 + temp_factor * 0.04
} else {
    let soil_m = tile.conditions.soil_moisture;
    let veg = tile.biome.vegetation_density;
    let veg_h = tile.biome.vegetation_health;
    // Bare soil evaporation + transpiration from vegetation
    let soil_evap = soil_m * 0.04 * temp_factor;
    let transpiration = veg * veg_h * 0.08 * temp_factor * soil_m.sqrt();
    let total = soil_evap + transpiration;
    if total > 0.15 { 0.15 } else { total }
};

let season_evap_mult = if season == "Summer" { 1.3 }
    else if season == "Winter" { 0.7 }
    else { 1.0 };
let evaporation = evaporation * season_evap_mult;

// === MACRO-DRIVEN MOISTURE TRANSPORT ===
// Pressure systems carry moisture across the globe via the macro weather engine.
// This replaces the broken wind_align-based directional advection that used
// 2D planar math on 3D sphere coordinates.

// === ISOTROPIC DIFFUSION ===
let n_count = neighbors.len();
let diffused = if n_count > 0 {
    let diffusion_sum = neighbor_sum(neighbors, "weather.humidity");
    diffusion_sum / n_count
} else {
    current_humidity
};

// Blend: macro influence proportional to coverage strength
let macro_weight = macro_humidity * 3.5;
if macro_weight > 0.35 { macro_weight = 0.35; }
let local_weight = 1.0 - macro_weight;

// Local: retain humidity, blend with neighbors, add evaporation
let local_humidity = current_humidity * 0.85 + diffused * 0.15;
let new_humidity = macro_humidity * macro_weight
    + (local_humidity + evaporation) * local_weight;

// === OROGRAPHIC STRIPPING ===
// Direct stripping: mountains/hills force uplift and condense moisture
let orographic_loss = if terrain == "Mountains" || terrain == "Cliffs" {
    let strip = 0.4 + tile.geology.elevation * 0.5;
    if strip > 0.8 { strip = 0.8; }
    strip
} else if terrain == "Hills" {
    let strip = 0.15 + tile.geology.elevation * 0.3;
    if strip > 0.4 { strip = 0.4; }
    strip
} else {
    0.0
};

// Rain shadow: adjacent high terrain strips moisture from air before reaching us.
// Simplified from broken directional version — uses proximity only.
let shadow_factor = 0.0;
for n in neighbors {
    let n_terrain = n.geology.terrain_type;
    if n_terrain == "Mountains" || n_terrain == "Cliffs" || n_terrain == "Hills" {
        let strength = if n_terrain == "Mountains" || n_terrain == "Cliffs" {
            n.geology.elevation * 0.08
        } else {
            n.geology.elevation * 0.03
        };
        if strength > shadow_factor { shadow_factor = strength; }
    }
}

let total_loss = orographic_loss + shadow_factor;
if total_loss > 0.85 { total_loss = 0.85; }
let new_humidity = new_humidity * (1.0 - total_loss);

// Moisture-dependent atmospheric decay: humid air is more stable
let decay = 0.994 + new_humidity * 0.005;
if decay > 0.999 { decay = 0.999; }
let new_humidity = new_humidity * decay;

if new_humidity < 0.0 { new_humidity = 0.0; }
if new_humidity > 1.0 { new_humidity = 1.0; }

set("humidity", new_humidity);
