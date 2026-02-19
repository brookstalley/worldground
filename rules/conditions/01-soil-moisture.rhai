// Soil moisture rule: accumulate from precipitation, drain based on geology
let precip = tile.weather.precipitation;
let drainage = tile.geology.drainage;
let current = tile.conditions.soil_moisture;

// Precipitation adds moisture
let added = precip * 0.3;

// Drainage removes moisture
let drained = current * drainage * 0.1;

let new_moisture = current + added - drained;
if new_moisture < 0.0 { set("soil_moisture", 0.0); }
else if new_moisture > 1.0 { set("soil_moisture", 1.0); }
else { set("soil_moisture", new_moisture); }

// Track drought/frost days
let temp = tile.weather.temperature;
if precip < 0.05 {
    set("drought_days", tile.conditions.drought_days + 1);
} else {
    set("drought_days", 0);
}

if temp < 273.0 {
    set("frost_days", tile.conditions.frost_days + 1);
} else {
    set("frost_days", 0);
}
