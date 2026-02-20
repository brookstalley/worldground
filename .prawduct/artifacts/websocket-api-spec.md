# WebSocket API Spec — Worldground
<!-- Artifact: WebSocket API Spec | Version: 1 | Tier: 2 -->
<!-- Inferred from codebase analysis — verify with product owner -->
<!-- sourced: src/server/protocol.rs, src/server/mod.rs, 2026-02-20 -->

## Connection
- **URL:** `ws://127.0.0.1:8118` (port configurable)
- **Protocol:** Standard WebSocket (no subprotocol)
- **Authentication:** None (local-only)

## Message Flow

```
Client connects via WebSocket
  ← Server sends WorldSnapshot (full state)
  ← Server sends TickDiff (after each tick)
  ← Server sends TickDiff ...
  ...
Client disconnects
```

The server streams data to the client. Client messages are ignored (except Close frames for graceful disconnect).

## Message Types

### WorldSnapshot (server → client, on connect)
Full world state sent once when a client connects.

```json
{
  "message_type": "WorldSnapshot",
  "world_id": "uuid-string",
  "name": "world name",
  "tick": 42,
  "season": "Summer",
  "season_length": 90,
  "tile_count": 16000,
  "topology_type": "FlatHex",
  "tiles": [
    {
      "id": 0,
      "neighbors": [1, 2, 3, 4, 5, 6],
      "position": { "x": 0.0, "y": 0.0, "z": 0.0, "lat": 45.0, "lon": -90.0 },
      "geology": { "terrain_type": "Plains", "elevation": 0.3, "soil_type": "Loam", "drainage": 0.5, "tectonic_stress": 0.1 },
      "climate": { "zone": "Temperate", "base_temperature": 288.15, "base_precipitation": 0.5, "latitude": 0.2 },
      "biome": { "biome_type": "Grassland", "vegetation_density": 0.6, "vegetation_health": 0.8, "transition_pressure": 0.0, "ticks_in_current_biome": 100 },
      "resources": { "resources": [{ "resource_type": "timber", "quantity": 50.0, "max_quantity": 100.0, "renewal_rate": 0.1, "requires_biome": ["TemperateForest"] }] },
      "weather": { "temperature": 290.0, "precipitation": 0.3, "precipitation_type": "Rain", "wind_speed": 5.0, "wind_direction": 180.0, "cloud_cover": 0.4, "humidity": 0.5, "storm_intensity": 0.0 },
      "conditions": { "soil_moisture": 0.4, "snow_depth": 0.0, "mud_level": 0.1, "flood_level": 0.0, "frost_days": 0, "drought_days": 0, "fire_risk": 0.1 }
    }
  ]
}
```

### TickDiff (server → client, every tick)
Only changed tile layers are included. Unchanged layers are omitted (not null).

```json
{
  "message_type": "TickDiff",
  "tick": 43,
  "season": "Summer",
  "changed_tiles": [
    {
      "id": 0,
      "weather": { "temperature": 291.0, "precipitation": 0.0, "precipitation_type": "None", "wind_speed": 4.5, "wind_direction": 175.0, "cloud_cover": 0.3, "humidity": 0.45, "storm_intensity": 0.0 }
    }
  ],
  "statistics": {
    "tick": 43,
    "biome_distribution": { "Grassland": 4000, "Ocean": 9600, "Desert": 500, "TemperateForest": 1900 },
    "avg_temperature": 288.5,
    "avg_moisture": 0.4,
    "avg_vegetation_health": 0.7,
    "diversity_index": 0.65,
    "rule_errors": 0,
    "tick_duration_ms": 950.0
  }
}
```

In `changed_tiles`, only layers that actually changed are present. If a tile's weather changed but biome didn't, the `biome` key is absent (not null). Enforced by `#[serde(skip_serializing_if = "Option::is_none")]`.

## HTTP Health Endpoint

### GET /health
Returns simulation health status as JSON.

```json
{
  "tick": 100,
  "tick_rate": 1.0,
  "diversity_index": 0.7,
  "rule_errors": 0,
  "snapshot_age_ticks": 5,
  "tile_count": 16000,
  "season": "Autumn"
}
```

The health endpoint shares the WebSocket port. Requests to `/health` without a WebSocket upgrade header receive an HTTP response.

## Error Handling
- **Client lag:** If a client falls behind on diffs, the server logs a warning but keeps the connection alive.
- **Client disconnect:** Handled gracefully. Server logs the disconnect and cleans up resources.
- **Broadcast failure:** If no clients are connected, tick diffs are silently dropped.
