// Wind & Temperature rule
// - Prevailing wind direction from latitude bands (Hadley cells)
// - Wind drifts toward prevailing + random perturbation
// - Sea breeze: coastal tiles get onshore wind nudge (seasonal)
// - Temperature: climate base - altitude lapse + smooth seasonal curve + ocean moderation
// - Thermal diffusion with neighbors for spatial coherence

let lat = tile.climate.latitude;
let abs_lat = if lat < 0.0 { -lat } else { lat };
let terrain = tile.geology.terrain_type;
let elev = tile.geology.elevation;
let my_x = tile.position.x;
let my_y = tile.position.y;

// === PREVAILING WIND ===
// Trade winds <30, Westerlies 30-60, Polar easterlies >60
let prevailing_dir = if abs_lat < 30.0 {
    if lat >= 0.0 { 45.0 } else { 135.0 }
} else if abs_lat < 60.0 {
    if lat >= 0.0 { 225.0 } else { 315.0 }
} else {
    if lat >= 0.0 { 45.0 } else { 135.0 }
};

// Seasonal wind shift: ITCZ migrates with the sun
let seasonal_shift = if season == "Summer" { 8.0 }
    else if season == "Winter" { -8.0 }
    else if season == "Spring" { 4.0 }
    else { -4.0 };
let prevailing_dir = prevailing_dir + seasonal_shift;

// Wind drifts 20% toward prevailing each tick + random perturbation
let current_dir = tile.weather.wind_direction;
let diff = prevailing_dir - current_dir;
let adj_diff = diff;
if adj_diff > 180.0 { adj_diff = adj_diff - 360.0; }
if adj_diff < -180.0 { adj_diff = adj_diff + 360.0; }
let norm_dir = current_dir + adj_diff * 0.2 + rand_range(-15.0, 15.0);
if norm_dir < 0.0 { norm_dir = norm_dir + 360.0; }
let norm_dir = norm_dir % 360.0;

// Wind speed from latitude band + terrain friction
let base_speed = if abs_lat < 30.0 {
    4.0
} else if abs_lat < 60.0 {
    6.5  // westerlies are strong
} else {
    3.5
};

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

let current_speed = tile.weather.wind_speed;
let target_speed = base_speed * friction + rand_range(-0.8, 0.8);
if target_speed < 0.5 { target_speed = 0.5; }
let new_speed = current_speed * 0.7 + target_speed * 0.3;

// === SEA BREEZE ===
// Coastal tiles with ocean neighbors get an onshore wind component.
// Stronger in summer (thermal contrast), suppressed by strong prevailing winds.
if terrain == "Coast" {
    let onshore_x = 0.0;
    let onshore_y = 0.0;
    let ocean_count = 0;
    for n in neighbors {
        if n.geology.terrain_type == "Ocean" {
            let dir = direction_to(n.position.x, n.position.y, my_x, my_y);
            if dir[0] != 0.0 || dir[1] != 0.0 {
                onshore_x += dir[0];
                onshore_y += dir[1];
                ocean_count += 1;
            }
        }
    }
    if ocean_count > 0 {
        let mag = sqrt(onshore_x * onshore_x + onshore_y * onshore_y);
        if mag > 0.001 {
            onshore_x = onshore_x / mag;
            onshore_y = onshore_y / mag;
        }
        let wx = sin_deg(norm_dir);
        let wy = cos_deg(norm_dir);
        let cross = wx * onshore_y - wy * onshore_x;
        let dot = wx * onshore_x + wy * onshore_y;

        let season_factor = if season == "Summer" { 1.0 }
            else if season == "Spring" { 0.6 }
            else if season == "Autumn" { 0.4 }
            else { 0.15 };

        // Only nudge when wind is light-moderate and not already onshore
        if dot < 0.5 && new_speed < 8.0 {
            let nudge_sign = if cross > 0.0 { 1.0 } else { -1.0 };
            let nudge_mag = if cross > 0.0 { cross } else { -cross };
            if nudge_mag > 1.0 { nudge_mag = 1.0; }
            let nudge = nudge_sign * nudge_mag * 8.0 * season_factor;
            norm_dir = norm_dir + nudge;
            if norm_dir < 0.0 { norm_dir = norm_dir + 360.0; }
            norm_dir = norm_dir % 360.0;
            new_speed = new_speed + 0.3 * season_factor;
        }
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
