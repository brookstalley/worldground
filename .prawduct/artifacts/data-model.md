# Data Model — Worldground
<!-- Artifact: Data Model | Version: 1 | Tier: 1 -->
<!-- Inferred from codebase analysis — verify with product owner -->
<!-- sourced: src/world/tile.rs, src/world/mod.rs, 2026-02-20 -->

## World

The top-level container for all simulation state.

| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Unique world identifier |
| name | String | World name |
| created_at | String | Creation timestamp |
| tick_count | u64 | Current simulation tick |
| season | Season | Current season (Spring/Summer/Autumn/Winter) |
| season_length | u32 | Ticks per season |
| tile_count | u32 | Number of tiles |
| topology_type | TopologyType | FlatHex or Geodesic |
| generation_params | GenerationParams | Parameters used to generate this world |
| snapshot_path | Option&lt;String&gt; | Path to last saved snapshot |
| tiles | Vec&lt;Tile&gt; | All tiles in the world |

## Tile

Each tile has an ID, position, neighbor list, and 6 data layers. Tiles may be hexagons (flat grid) or a mix of hexagons and pentagons (geodesic).

| Field | Type | Description |
|-------|------|-------------|
| id | u32 | Unique tile identifier |
| neighbors | Vec&lt;u32&gt; | IDs of adjacent tiles |
| position | Position | 3D position with lat/lon |
| geology | GeologyLayer | Immutable terrain data |
| climate | ClimateLayer | Immutable climate data |
| biome | BiomeLayer | Mutable ecological state |
| resources | ResourceLayer | Mutable resource deposits |
| weather | WeatherLayer | Mutable weather state |
| conditions | ConditionsLayer | Mutable ground conditions |

## Position

| Field | Type | Description |
|-------|------|-------------|
| x | f64 | X coordinate (unit sphere for geodesic, pixel offset for flat) |
| y | f64 | Y coordinate |
| z | f64 | Z coordinate (0.0 for flat) |
| lat | f64 | Latitude in degrees (-90 to 90). Populated for both topologies. |
| lon | f64 | Longitude in degrees (-180 to 180). Populated for both topologies. |

Constructor: `Position::flat(x, y)` creates a flat-grid position (z=0, lat/lon=0 until climate assignment populates them).

## Layers

### GeologyLayer (Immutable — set at generation)
| Field | Type | Range | Description |
|-------|------|-------|-------------|
| terrain_type | TerrainType | enum | Ocean, Coast, Plains, Hills, Mountains, Cliffs, Wetlands |
| elevation | f32 | 0.0-1.0 | Normalized height |
| soil_type | SoilType | enum | Sand, Clay, Loam, Rock, Silt |
| drainage | f32 | 0.0-1.0 | How quickly water drains |
| tectonic_stress | f32 | 0.0-1.0 | Geological instability |

### ClimateLayer (Immutable — set at generation)
| Field | Type | Range | Description |
|-------|------|-------|-------------|
| zone | ClimateZone | enum | Polar, Subpolar, Temperate, Subtropical, Tropical |
| base_temperature | f32 | Kelvin | Baseline temperature for this location |
| base_precipitation | f32 | 0.0-1.0 | Baseline precipitation probability |
| latitude | f32 | -1.0 to 1.0 | North-south position |

### WeatherLayer (Mutable — updated by Weather phase)
| Field | Type | Range | Description |
|-------|------|-------|-------------|
| temperature | f32 | Kelvin | Current temperature |
| precipitation | f32 | 0.0-1.0 | Current precipitation intensity |
| precipitation_type | PrecipitationType | enum | None, Rain, Snow, Hail, Sleet |
| wind_speed | f32 | ≥0.0 | Wind speed |
| wind_direction | f32 | 0-360 | Wind direction in degrees |
| cloud_cover | f32 | 0.0-1.0 | Cloud coverage |
| humidity | f32 | 0.0-1.0 | Air moisture |
| storm_intensity | f32 | 0.0-1.0 | Storm strength |

### ConditionsLayer (Mutable — updated by Conditions phase)
| Field | Type | Range | Description |
|-------|------|-------|-------------|
| soil_moisture | f32 | 0.0-1.0 | Ground water saturation |
| snow_depth | f32 | ≥0.0 | Snow accumulation |
| mud_level | f32 | 0.0-1.0 | Ground softness |
| flood_level | f32 | 0.0-1.0 | Flooding intensity |
| frost_days | u32 | ≥0 | Consecutive days below freezing |
| drought_days | u32 | ≥0 | Consecutive days without rain |
| fire_risk | f32 | 0.0-1.0 | Wildfire probability |

### BiomeLayer (Mutable — updated by Terrain phase)
| Field | Type | Range | Description |
|-------|------|-------|-------------|
| biome_type | BiomeType | enum | Ocean, Ice, Tundra, BorealForest, TemperateForest, Grassland, Savanna, Desert, TropicalForest, Wetland, Barren |
| vegetation_density | f32 | 0.0-1.0 | How much vegetation covers the tile |
| vegetation_health | f32 | 0.0-1.0 | Plant health |
| transition_pressure | f32 | -1.0 to 1.0 | Pressure to change biome type |
| ticks_in_current_biome | u32 | ≥0 | Stability counter (higher = more resistant to change) |

### ResourceLayer (Mutable — updated by Resources phase)
Contains a Vec of ResourceDeposit:

| Field | Type | Range | Description |
|-------|------|-------|-------------|
| resource_type | String | - | Resource name (e.g., "iron", "timber") |
| quantity | f32 | 0.0-max | Current quantity |
| max_quantity | f32 | >0.0 | Maximum capacity |
| renewal_rate | f32 | ≥0.0 | Regeneration per tick |
| requires_biome | Option&lt;Vec&lt;BiomeType&gt;&gt; | - | Biomes where this resource can exist |

## Enumerations

### Season
Spring → Summer → Autumn → Winter → Spring (cycles)

### TopologyType
- **FlatHex:** Flat hexagonal grid
- **Geodesic:** Spherical geodesic topology

### TerrainType
Ocean, Coast, Plains, Hills, Mountains, Cliffs, Wetlands

### SoilType
Sand, Clay, Loam, Rock, Silt

### ClimateZone
Polar, Subpolar, Temperate, Subtropical, Tropical

### BiomeType
Ocean, Ice, Tundra, BorealForest, TemperateForest, Grassland, Savanna, Desert, TropicalForest, Wetland, Barren

### PrecipitationType
None, Rain, Snow, Hail, Sleet

## Generation Parameters
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| seed | u64 | 0 (random) | Deterministic seed |
| tile_count | u32 | 16000 | Number of hex tiles |
| ocean_ratio | f32 | 0.6 | Fraction of water tiles |
| mountain_ratio | f32 | 0.1 | Fraction of mountainous land |
| elevation_roughness | f32 | 0.5 | Terrain variation intensity |
| climate_bands | bool | true | Use latitude-based climate zones |
| resource_density | f32 | 0.3 | Resource scattering density |
| initial_biome_maturity | f32 | 0.5 | Initial biome establishment level |
| topology | TopologyConfig | (see below) | Grid topology configuration |

### TopologyConfig
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| mode | String | "flat" | "flat" (hex grid) or "geodesic" (icosphere) |
| subdivision_level | u32 | 4 | Geodesic only: 1-7. Tile count = 10 * 4^level + 2 |

## Serialization
- **Persistence:** Bincode (binary, compact, fast) for snapshots
- **WebSocket:** JSON via serde for viewer communication
- **All types derive:** Debug, Clone, PartialEq, Serialize, Deserialize
