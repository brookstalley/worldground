// Vegetation health: affected by moisture, temperature, and drought
let biome = tile.biome.biome_type;
let moisture = tile.conditions.soil_moisture;
let health = tile.biome.vegetation_health;
let drought = tile.conditions.drought_days;

// Skip ocean and non-vegetated biomes
if biome == "Ocean" || biome == "Ice" || biome == "Barren" || biome == "Desert" {
    return;
}

let health_delta = 0.0;

// Adequate moisture promotes health
if moisture > 0.3 && moisture < 0.8 {
    health_delta = 0.01;
} else if moisture < 0.1 {
    health_delta = -0.02;
} else if moisture > 0.9 {
    // Flooding slightly damages vegetation
    health_delta = -0.005;
}

// Extended drought damages health
if drought > 20 {
    health_delta = health_delta - 0.01;
}

let new_health = health + health_delta;
if new_health < 0.0 { set("vegetation_health", 0.0); }
else if new_health > 1.0 { set("vegetation_health", 1.0); }
else { set("vegetation_health", new_health); }
