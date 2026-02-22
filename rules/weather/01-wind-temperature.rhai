// Wind & Temperature rule
// - Primary wind from macro weather system (pressure-system driven)
// - Terrain friction modifies wind speed
// - Sea breeze: coastal tiles get onshore wind boost (seasonal)
// - Temperature: climate base - altitude lapse + smooth seasonal curve + ocean moderation
// - Thermal diffusion with neighbors for spatial coherence

let lat = tile.climate.latitude;
let abs_lat = if lat < 0.0 { -lat } else { lat };
let terrain = tile.geology.terrain_type;
let elev = tile.geology.elevation;

// === WIND (macro-driven) ===
// Macro weather provides pressure-system-derived wind direction and speed.
// Local rules blend in terrain friction and small perturbations.

let macro_speed = tile.weather.macro_wind_speed;
let macro_dir = tile.weather.macro_wind_direction;

// Terrain friction factor
let friction = if terrain == "Mountains" || terrain == "Cliffs" {
    0.4
} else if terrain == "Hills" {
    0.7
} else if terrain == "Ocean" {
    1.3
} else if terrain == "Coast" {
    1.15
} else if terrain == "Wetlands" {
    0.9
} else {
    1.0
};

// If macro provides wind, use it as primary; otherwise fall back to latitude bands
let target_dir = if macro_speed > 0.5 {
    macro_dir
} else {
    // Fallback: latitude-band prevailing winds (for early ticks before systems spawn)
    if abs_lat < 30.0 {
        if lat >= 0.0 { 45.0 } else { 135.0 }
    } else if abs_lat < 60.0 {
        if lat >= 0.0 { 225.0 } else { 315.0 }
    } else {
        if lat >= 0.0 { 45.0 } else { 135.0 }
    }
};

// Seasonal wind shift: ITCZ migrates with the sun
let seasonal_shift = if season == "Summer" { 8.0 }
    else if season == "Winter" { -8.0 }
    else if season == "Spring" { 4.0 }
    else { -4.0 };
let target_dir = target_dir + seasonal_shift;

let target_speed = if macro_speed > 0.5 {
    macro_speed * friction
} else {
    // Fallback base speed from latitude bands
    let base = if abs_lat < 30.0 { 4.0 }
        else if abs_lat < 60.0 { 6.5 }
        else { 3.5 };
    base * friction
};

// Smooth transition: wind drifts toward target
let current_dir = tile.weather.wind_direction;
let current_speed = tile.weather.wind_speed;
let diff = target_dir - current_dir;
let adj_diff = diff;
if adj_diff > 180.0 { adj_diff = adj_diff - 360.0; }
if adj_diff < -180.0 { adj_diff = adj_diff + 360.0; }

// Blend rate: macro wind has authority (0.35), fallback uses slower drift (0.2)
let blend = if macro_speed > 0.5 { 0.35 } else { 0.2 };
let norm_dir = current_dir + adj_diff * blend + rand_range(-10.0, 10.0);
if norm_dir < 0.0 { norm_dir = norm_dir + 360.0; }
let norm_dir = norm_dir % 360.0;

let new_speed = current_speed * 0.6 + target_speed * 0.4 + rand_range(-0.5, 0.5);

// === SEA BREEZE ===
// Coastal tiles near ocean get an onshore wind boost (seasonal thermal contrast).
// The macro weather system handles large-scale onshore flow; this adds the
// local thermal sea-breeze component.
if terrain == "Coast" {
    let ocean_count = 0;
    for n in neighbors {
        if n.geology.terrain_type == "Ocean" {
            ocean_count += 1;
        }
    }
    if ocean_count > 0 {
        let season_factor = if season == "Summer" { 1.0 }
            else if season == "Spring" { 0.6 }
            else if season == "Autumn" { 0.4 }
            else { 0.15 };
        // Boost wind speed from thermal contrast — stronger with more ocean exposure
        new_speed = new_speed + 0.3 * season_factor;
    }
}

// Hard cap: wind speed must stay in realistic range (storms can boost up to 25)
if new_speed > 20.0 { new_speed = 20.0; }
if new_speed < 0.3 { new_speed = 0.3; }

set("wind_direction", norm_dir);
set("wind_speed", new_speed);

// === TEMPERATURE ===
let base_temp = tile.climate.base_temperature;

// Altitude lapse rate: ~6.5K per 1000m, elevation is 0-1 representing 0-3000m
let elev_adj = if elev > 0.0 { elev * 20.0 } else { 0.0 };

// Smooth seasonal cycle using triangle wave with 4 seasons
let season_factor = if season == "Spring" {
    0.5   // warming up
} else if season == "Summer" {
    1.0   // peak heat
} else if season == "Autumn" {
    -0.5  // cooling down
} else {
    -1.0  // deep cold
};

// Amplitude: higher latitude = bigger seasonal swing, but capped
let seasonal_amplitude = 6.0 + abs_lat * 0.15;  // 6K at equator, ~19.5K at poles
if seasonal_amplitude > 18.0 { seasonal_amplitude = 18.0; }

let seasonal_mod = season_factor * seasonal_amplitude;

// Ocean thermal inertia: oceans moderate temperature extremes
let ocean_damping = if terrain == "Ocean" {
    0.25
} else if terrain == "Coast" {
    0.55
} else {
    1.0
};

// Thermal diffusion: temperature blends slightly with neighbors
let diffusion_amount = 0.08; // 8% blend with neighbor average
let local_temp = base_temp - elev_adj + seasonal_mod * ocean_damping + rand_range(-1.5, 1.5);

if neighbors.len() > 0 {
    let n_avg_temp = neighbor_avg(neighbors, "weather.temperature");
    let temp = local_temp * (1.0 - diffusion_amount) + n_avg_temp * diffusion_amount;
    set("temperature", temp);
} else {
    set("temperature", local_temp);
}
