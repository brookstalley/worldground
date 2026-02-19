// Snow accumulation and mud formation
let temp = tile.weather.temperature;
let precip = tile.weather.precipitation;
let precip_type = tile.weather.precipitation_type;
let snow = tile.conditions.snow_depth;
let drainage = tile.geology.drainage;

// Snow accumulates when precipitation is snow
if precip_type == "Snow" {
    set("snow_depth", snow + precip * 0.5);
} else if temp > 275.0 && snow > 0.0 {
    // Snow melts above ~2C, adding to soil moisture
    let melt = snow * 0.1;
    let new_snow = snow - melt;
    if new_snow < 0.01 { set("snow_depth", 0.0); }
    else { set("snow_depth", new_snow); }
}

// Mud forms from rain on poorly-drained soil
if precip_type == "Rain" && drainage < 0.5 {
    let mud_increase = precip * (1.0 - drainage) * 0.2;
    let new_mud = tile.conditions.mud_level + mud_increase;
    if new_mud > 1.0 { set("mud_level", 1.0); }
    else { set("mud_level", new_mud); }
} else {
    // Mud dries slowly
    let new_mud = tile.conditions.mud_level * 0.95;
    if new_mud < 0.01 { set("mud_level", 0.0); }
    else { set("mud_level", new_mud); }
}
