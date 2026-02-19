// Biome transition pressure: accumulate based on conditions vs current biome
let biome = tile.biome.biome_type;
let moisture = tile.conditions.soil_moisture;
let drought = tile.conditions.drought_days;
let temp = tile.weather.temperature;
let current_pressure = tile.biome.transition_pressure;

// Skip ocean tiles
if biome == "Ocean" {
    return;
}

let pressure_delta = 0.0;

// Drought pushes toward desert
if drought > 10 {
    pressure_delta = -0.01 * (drought - 10);
}

// High moisture pushes toward forest/wetland
if moisture > 0.7 {
    pressure_delta = 0.01 * (moisture - 0.7) * 10.0;
}

// Extreme cold pushes toward tundra/ice
if temp < 260.0 {
    pressure_delta = -0.005;
}

let new_pressure = current_pressure + pressure_delta;
if new_pressure < -1.0 { set("transition_pressure", -1.0); }
else if new_pressure > 1.0 { set("transition_pressure", 1.0); }
else { set("transition_pressure", new_pressure); }
