// Base temperature rule: adjust temperature based on climate zone, elevation, and season
let base_temp = tile.climate.base_temperature;
let elev_adjustment = tile.geology.elevation * 20.0;

// Seasonal variation
let seasonal_mod = if season == "Summer" {
    10.0
} else if season == "Winter" {
    -10.0
} else {
    0.0
};

// Random daily variation
let variation = rand_range(-3.0, 3.0);

let temp = base_temp - elev_adjustment + seasonal_mod + variation;
set("temperature", temp);
