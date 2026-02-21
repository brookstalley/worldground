// Cloud formation & Precipitation rule
//
// Key features:
// - Slow asymmetric cloud inertia (builds slowly, dissipates even slower)
// - Neighbor cloud averaging for spatial coherence
// - Pre-storm cloud darkening from approaching storms
// - Precipitation with orographic and convective enhancement
//
// NOTE: This Rhai script reads from the pre-phase tile snapshot. The native
// evaluator (native_weather.rs) chains rule outputs via WeatherAccum, so it
// reads Rule 2's computed humidity instead of the stale snapshot value here.
// This means the native path produces more physically consistent results
// (clouds reflect current-tick humidity, precipitation consumes it in-place).

let temp = tile.weather.temperature;
let humidity = tile.weather.humidity;
let cloud = tile.weather.cloud_cover;
let terrain = tile.geology.terrain_type;
let elev = tile.geology.elevation;

// === SATURATION HUMIDITY ===
// How much moisture the air can hold — warm air holds much more
// At 230K (~-43C): capacity ~0.15, at 300K (~27C): capacity ~0.85
let saturation = 0.08 + (temp - 230.0) * 0.011;
if saturation < 0.08 { saturation = 0.08; }
if saturation > 0.95 { saturation = 0.95; }

// Relative humidity: how close to saturation we are
let relative_humidity = humidity / saturation;
if relative_humidity > 2.0 { relative_humidity = 2.0; }

// === CLOUD COVER ===
// Clouds form when RH approaches saturation
// Below 0.4 RH: mostly clear; 0.4-0.7: scattered; 0.7-1.0: overcast
let target_cloud = if relative_humidity < 0.35 {
    relative_humidity * 0.1   // mostly clear skies
} else if relative_humidity < 0.6 {
    0.035 + (relative_humidity - 0.35) * 0.7  // scattered clouds
} else if relative_humidity < 0.85 {
    0.21 + (relative_humidity - 0.6) * 1.6  // building toward overcast
} else if relative_humidity < 1.0 {
    0.61 + (relative_humidity - 0.85) * 2.0  // heavy overcast
} else {
    0.91 + (relative_humidity - 1.0) * 0.1  // fully saturated, capped
};
if target_cloud > 1.0 { target_cloud = 1.0; }

// Neighbor cloud influence: clouds have spatial coherence
let neighbor_cloud_avg = if neighbors.len() > 0 {
    neighbor_avg(neighbors, "weather.cloud_cover")
} else {
    cloud
};
let neighbor_storm_max = neighbor_max(neighbors, "weather.storm_intensity");

// Blend target slightly toward neighbor average (spatial coherence)
let target_cloud = target_cloud * 0.85 + neighbor_cloud_avg * 0.15;
if target_cloud > 1.0 { target_cloud = 1.0; }

// Pre-storm cloud enhancement: approaching storms darken the sky
if neighbor_storm_max > 0.2 {
    let storm_cloud_boost = neighbor_storm_max * 0.15;
    target_cloud = target_cloud + storm_cloud_boost;
    if target_cloud > 1.0 { target_cloud = 1.0; }
}

// Cloud inertia — clouds build slowly and dissipate even slower
// This creates the dramatic "building" effect
let cloud_speed = if target_cloud > cloud {
    let urgency = target_cloud - cloud;
    if urgency > 0.3 {
        0.18  // rapid buildup when very humid (convective tower)
    } else {
        0.10  // gentle buildup: wisps forming, growing
    }
} else {
    0.06  // dissipation: clouds linger after clearing
};

let new_cloud = cloud + (target_cloud - cloud) * cloud_speed + rand_range(-0.02, 0.02);
if new_cloud < 0.0 { new_cloud = 0.0; }
if new_cloud > 1.0 { new_cloud = 1.0; }
set("cloud_cover", new_cloud);

// === PRECIPITATION ===
// Rain requires both sufficient moisture AND cloud formation
// Threshold: RH > 0.70 and meaningful cloud cover > 0.35
if relative_humidity > 0.70 && new_cloud > 0.35 {
    // Intensity scales with moisture excess and cloud thickness
    let excess = relative_humidity - 0.70;
    let intensity = excess * new_cloud * 1.2;

    // Orographic enhancement: forced uplift over terrain produces extra rain
    if terrain == "Mountains" || terrain == "Cliffs" {
        intensity = intensity * 1.8;  // strong orographic rain
    } else if terrain == "Hills" {
        intensity = intensity * 1.3;
    }

    // Convective enhancement in warm, humid conditions
    if temp > 290.0 && humidity > 0.5 {
        intensity = intensity * 1.2;  // tropical downpours
    }

    if intensity > 1.0 { intensity = 1.0; }
    if intensity < 0.01 { intensity = 0.0; }

    if intensity > 0.0 {
        set("precipitation", intensity);

        // Precipitation type — more nuanced temperature bands
        if temp < 258.0 {
            set("precipitation_type", "Snow");      // deep cold: dry snow
        } else if temp < 268.0 {
            set("precipitation_type", "Snow");      // cold: wet snow
        } else if temp < 273.0 {
            set("precipitation_type", "Sleet");     // near freezing: sleet/mix
        } else {
            set("precipitation_type", "Rain");      // above freezing: rain
        }

        // Precipitation removes moisture from the air
        let consumed = intensity * 0.25;
        let new_h = humidity - consumed;
        if new_h < 0.02 { new_h = 0.02; }  // atmosphere always has trace moisture
        set("humidity", new_h);
    } else {
        set("precipitation", 0.0);
        set("precipitation_type", "None");
    }
} else {
    set("precipitation", 0.0);
    set("precipitation_type", "None");
}
