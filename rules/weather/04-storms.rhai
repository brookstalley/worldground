// Storm formation, pressure-driven dynamics, and decay
//
// Key features:
// - Pressure-driven nucleation: low pressure systems spawn and sustain storms
// - Gradual intensification with humidity fuel and pressure support
// - Slow decay: storms last 30-80 ticks for visible lifecycle
// - Coastal convergence: land-sea boundary creates uplift storms
// - Coriolis-correct cyclonic rotation
// - High pressure suppresses storms (faster decay)
//
// NOTE: This Rhai script reads from the pre-phase tile snapshot. The native
// evaluator (native_weather.rs) chains rule outputs via WeatherAccum, so it
// reads Rule 1's wind, Rule 2/3's humidity, and Rule 3's cloud_cover instead
// of the stale snapshot values here. This means storm nucleation uses
// current-tick clouds, and storm wind amplification builds on computed wind.

let terrain = tile.geology.terrain_type;
let current_storm = tile.weather.storm_intensity;
let humidity = tile.weather.humidity;
let temp = tile.weather.temperature;
let cloud = tile.weather.cloud_cover;
let wind_speed = tile.weather.wind_speed;
let pressure = tile.weather.pressure;

// === GATHER NEIGHBOR DATA ===
let max_temp_diff = 0.0;
for n in neighbors {
    let diff = abs(temp - n.weather.temperature);
    if diff > max_temp_diff { max_temp_diff = diff; }
}

let neighbor_count = neighbors.len();
let neighbor_storm_avg = if neighbor_count > 0 {
    neighbor_avg(neighbors, "weather.storm_intensity")
} else { 0.0 };

let new_storm = current_storm;

// === PRESSURE-DRIVEN NUCLEATION ===
// Low pressure creates uplift and convergence — the primary storm driver.
let pressure_deficit = 1013.25 - pressure;

// Strong low pressure + humidity + clouds -> storm nucleation
if pressure_deficit > 3.0 && humidity > 0.4 && cloud > 0.35 {
    let pressure_factor = (pressure_deficit - 3.0) * 0.015;
    if pressure_factor > 0.4 { pressure_factor = 0.4; }
    let nucleation = pressure_factor * humidity * cloud;
    if nucleation > new_storm {
        new_storm = new_storm + (nucleation - new_storm) * 0.3 + rand_range(0.0, 0.03);
    }
}

// === FORMATION (secondary mechanisms) ===

// 1. Frontal storms: temperature gradient indicates a front
if max_temp_diff > 5.0 && humidity > 0.45 && cloud > 0.4 {
    let frontal = (max_temp_diff - 5.0) * 0.02 * humidity * cloud;
    if frontal > 0.3 { frontal = 0.3; }
    if frontal > new_storm {
        new_storm = new_storm + (frontal - new_storm) * 0.25 + rand_range(0.0, 0.03);
    }
}

// 2. Convective storms: hot + humid + cloudy (tropical thunderstorms)
if temp > 295.0 && humidity > 0.55 && cloud > 0.55 {
    let convective = (temp - 295.0) * 0.006 * humidity;
    if convective > 0.2 { convective = 0.2; }
    if convective > new_storm {
        new_storm = new_storm + (convective - new_storm) * 0.25 + rand_range(0.0, 0.02);
    }
}

// 3. Orographic storms: forced uplift in mountains during moderate humidity
if humidity > 0.45 && (terrain == "Mountains" || terrain == "Cliffs") && cloud > 0.45 {
    let oro_storm = humidity * 0.08 * cloud;
    if oro_storm > new_storm {
        new_storm = new_storm + (oro_storm - new_storm) * 0.2;
    }
}

// 4. Coastal convergence: land-sea boundary creates uplift
if terrain == "Coast" && humidity > 0.50 && cloud > 0.45 {
    let coast_storm = humidity * 0.05 * cloud;
    if coast_storm > 0.08 && coast_storm > new_storm * 0.8 {
        new_storm = new_storm + coast_storm * 0.15;
    }
}

// === NEIGHBOR SPREADING ===
// Storms spread to adjacent tiles via proximity (replaces broken wind_align propagation)
if neighbor_storm_avg > 0.15 {
    let spread = neighbor_storm_avg * 0.08;
    if spread > 0.02 {
        new_storm = new_storm + spread;
    }
}

// Gentle ambient effect from nearby storms (turbulent spreading)
if neighbor_storm_avg > 0.1 {
    new_storm = new_storm + neighbor_storm_avg * 0.01;
}

// === INTENSIFICATION ===
// Active storms feed on humidity and low pressure, with diminishing returns
if new_storm > 0.1 && humidity > 0.4 {
    let fuel = (humidity - 0.4) * 0.025;
    // Low pressure amplifies intensification
    let pressure_boost = if pressure_deficit > 5.0 { 1.0 + pressure_deficit * 0.01 } else { 1.0 };
    let headroom = 1.0 - new_storm;
    let intensification = fuel * headroom * pressure_boost;
    new_storm = new_storm + intensification;
}

// === DECAY ===
// Storms decay gradually — running out of humidity fuel is the main death mechanism
let decay_rate = if terrain == "Ocean" {
    if temp > 293.0 { 0.025 } else { 0.045 }  // warm tropical ocean sustains longest
} else if terrain == "Coast" {
    0.045
} else if terrain == "Mountains" || terrain == "Cliffs" {
    0.08   // mountains disrupt storms
} else if terrain == "Hills" {
    0.06
} else {
    0.05   // land: moderate decay
};

// High pressure suppresses storms (faster decay under ridges)
if pressure_deficit < -3.0 {
    let high_pressure_decay = (-pressure_deficit - 3.0) * 0.01;
    if high_pressure_decay > 0.05 { high_pressure_decay = 0.05; }
    decay_rate = decay_rate + high_pressure_decay;
}

// Low humidity starvation
if humidity < 0.3 {
    let starvation = (0.3 - humidity) * 0.15;
    new_storm = new_storm * (1.0 - decay_rate - starvation);
} else {
    new_storm = new_storm * (1.0 - decay_rate);
}

// Storms below threshold die off
if new_storm < 0.03 { new_storm = 0.0; }
if new_storm > 1.0 { new_storm = 1.0; }

set("storm_intensity", new_storm);

// === STORM EFFECTS ON WEATHER ===
if new_storm > 0.08 {
    // Wind amplification
    let storm_wind = wind_speed * (1.0 + new_storm * 2.0);
    if storm_wind > 25.0 { storm_wind = 25.0; }
    set("wind_speed", storm_wind);

    // Cloud darkening — storms produce thick, dark clouds
    let storm_cloud = cloud + new_storm * 0.5;
    if storm_cloud > 1.0 { storm_cloud = 1.0; }
    set("cloud_cover", storm_cloud);

    // Cyclonic rotation — hemisphere-dependent for realism
    let dir = tile.weather.wind_direction;
    let lat = tile.climate.latitude;
    let coriolis_bias = if lat >= 0.0 { -1.0 } else { 1.0 };
    let rotation = new_storm * (coriolis_bias * 12.0 + rand_range(-8.0, 8.0));
    let new_dir = (dir + rotation) % 360.0;
    if new_dir < 0.0 { new_dir = new_dir + 360.0; }
    set("wind_direction", new_dir);
}
