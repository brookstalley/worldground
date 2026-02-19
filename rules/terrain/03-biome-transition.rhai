// Biome transition based on accumulated pressure and biome stability.
// Requires sustained pressure beyond a threshold that increases with biome age.
// Only transitions to adjacent biomes on the moisture/temperature gradient.

let biome = tile.biome.biome_type;
let pressure = tile.biome.transition_pressure;
let stability = tile.biome.ticks_in_current_biome;

// Ocean never transitions via rules (geological change only)
if biome == "Ocean" { return; }

// Resistance: established biomes require higher pressure to transition.
// Base threshold 0.6, increases by 0.0006 per tick up to +0.3 (at 500 ticks).
let resist = stability * 0.0006;
if resist > 0.3 { resist = 0.3; }
let threshold = 0.6 + resist;

// Drying/cooling transitions (negative pressure)
if pressure < -threshold {
    if biome == "Grassland" { set("biome_type", "Savanna"); }
    else if biome == "Savanna" { set("biome_type", "Desert"); }
    else if biome == "TemperateForest" { set("biome_type", "Grassland"); }
    else if biome == "BorealForest" { set("biome_type", "TemperateForest"); }
    else if biome == "TropicalForest" { set("biome_type", "Savanna"); }
    else if biome == "Wetland" { set("biome_type", "Grassland"); }
    set("transition_pressure", 0.0);
}

// Wetting/warming transitions (positive pressure)
if pressure > threshold {
    if biome == "Desert" { set("biome_type", "Savanna"); }
    else if biome == "Savanna" { set("biome_type", "Grassland"); }
    else if biome == "Grassland" { set("biome_type", "TemperateForest"); }
    else if biome == "Tundra" { set("biome_type", "BorealForest"); }
    else if biome == "Ice" { set("biome_type", "Tundra"); }
    set("transition_pressure", 0.0);
}
