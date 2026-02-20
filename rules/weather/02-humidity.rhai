// Humidity rule: evaporation + DIRECTIONAL moisture transport + orographic stripping
//
// Key features:
// - Humidity flows WITH the wind via directional advection
// - Onshore moisture boost from ocean neighbors with aligned wind
// - Directional rain shadow: tiles downwind of mountains get drier
// - Orographic stripping on mountains/hills themselves

let terrain = tile.geology.terrain_type;
let temp = tile.weather.temperature;
let current_humidity = tile.weather.humidity;
let wind_speed = tile.weather.wind_speed;
let my_x = tile.position.x;
let my_y = tile.position.y;

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
    (soil_m * 0.03 + veg * 0.01) * temp_factor
};

let season_evap_mult = if season == "Summer" { 1.3 }
    else if season == "Winter" { 0.7 }
    else { 1.0 };
let evaporation = evaporation * season_evap_mult;

// === ONSHORE MOISTURE BOOST ===
// Wind blowing from ocean neighbors carries extra moisture inland
let onshore_boost = 0.0;
if terrain != "Ocean" {
    for n in neighbors {
        if n.geology.terrain_type == "Ocean" {
            let alignment = wind_align(n.position.x, n.position.y, my_x, my_y, n.weather.wind_direction);
            if alignment > 0.2 {
                let boost = alignment * n.weather.wind_speed * 0.004 * n.weather.humidity;
                onshore_boost = onshore_boost + boost;
            }
        }
    }
    if onshore_boost > 0.06 { onshore_boost = 0.06; }
}
let evaporation = evaporation + onshore_boost;

// === DIRECTIONAL ADVECTION ===
// For each neighbor, check if its wind blows TOWARD us.
// Weight humidity contribution by wind alignment * wind speed.
let advection_sum = 0.0;
let advection_weight = 0.0;
let diffusion_sum = neighbor_sum(neighbors, "weather.humidity");
let n_count = neighbors.len();

for n in neighbors {
    let alignment = wind_align(n.position.x, n.position.y, my_x, my_y, n.weather.wind_direction);

    if alignment > 0.0 {
        // Stronger wind + better alignment = more moisture transport
        let n_speed = clamp(n.weather.wind_speed, 0.0, 15.0);
        let weight = alignment * alignment * n_speed;  // squared alignment for sharper directionality
        advection_sum += n.weather.humidity * weight;
        advection_weight += weight;
    }
}

// Directional advection: humidity arriving via wind
let advected = if advection_weight > 0.01 {
    advection_sum / advection_weight
} else {
    current_humidity
};

// Isotropic diffusion: gentle mixing with all neighbors (atmospheric turbulence)
let diffused = if n_count > 0 {
    diffusion_sum / n_count
} else {
    current_humidity
};

// How much the wind drives directional transport vs isotropic mixing
let capped_wind = wind_speed;
if capped_wind > 15.0 { capped_wind = 15.0; }

// Strong wind: mostly directional. Calm: mostly diffusion.
let wind_factor = capped_wind / 10.0;
if wind_factor > 0.6 { wind_factor = 0.6; }
if wind_factor < 0.05 { wind_factor = 0.05; }

let diffusion_factor = 0.04;  // always a little isotropic mixing
let retention = 1.0 - wind_factor - diffusion_factor;

let new_humidity = current_humidity * retention
    + advected * wind_factor
    + diffused * diffusion_factor
    + evaporation;

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

// Rain shadow: is wind blowing FROM a mountain toward us?
// If so, the air arriving has already been stripped — we get extra drying.
// This propagates dryness downwind of mountains over multiple ticks.
let shadow_factor = 0.0;
let upwind_dir = tile.weather.wind_direction + 180.0;

for n in neighbors {
    let n_terrain = n.geology.terrain_type;
    if n_terrain == "Mountains" || n_terrain == "Cliffs" || n_terrain == "Hills" {
        // Is this mountain in the upwind direction?
        let alignment = wind_align(my_x, my_y, n.position.x, n.position.y, upwind_dir);
        if alignment > 0.3 {
            let strength = if n_terrain == "Mountains" || n_terrain == "Cliffs" {
                alignment * n.geology.elevation * 0.25
            } else {
                alignment * n.geology.elevation * 0.10
            };
            if strength > shadow_factor { shadow_factor = strength; }
        }
    }
}

let total_loss = orographic_loss + shadow_factor;
if total_loss > 0.85 { total_loss = 0.85; }
let new_humidity = new_humidity * (1.0 - total_loss);

// Natural humidity loss (dry air mixing from above) — slow to preserve gradients
let new_humidity = new_humidity * 0.997;

if new_humidity < 0.0 { new_humidity = 0.0; }
if new_humidity > 1.0 { new_humidity = 1.0; }

set("humidity", new_humidity);
