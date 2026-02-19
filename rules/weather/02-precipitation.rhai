// Precipitation rule: determine precipitation based on climate and cloud cover
let base_precip = tile.climate.base_precipitation;
let cloud = tile.weather.cloud_cover;

// Higher cloud cover increases precipitation chance
let precip_chance = base_precip * 0.5 + cloud * 0.5;
let roll = rand();

if roll < precip_chance {
    let intensity = rand_range(0.1, precip_chance);
    set("precipitation", intensity);

    // Determine type based on temperature
    let temp = tile.weather.temperature;
    if temp < 263.0 {
        set("precipitation_type", "Snow");
    } else if temp < 273.0 {
        set("precipitation_type", "Sleet");
    } else {
        set("precipitation_type", "Rain");
    }
} else {
    set("precipitation", 0.0);
    set("precipitation_type", "None");
}

// Cloud cover drifts toward climate base with random variation
let new_cloud = cloud * 0.8 + base_precip * 0.2 + rand_range(-0.1, 0.1);
if new_cloud < 0.0 { set("cloud_cover", 0.0); }
else if new_cloud > 1.0 { set("cloud_cover", 1.0); }
else { set("cloud_cover", new_cloud); }
