# Security Model — Worldground
<!-- Artifact: Security Model | Version: 2 | Tier: 2 -->
<!-- Inferred from codebase analysis — verify with product owner -->

## Threat Surface
Worldground is a local-only simulation engine. The primary security concerns are:
1. **Rhai script sandbox escape** — malicious rules could access the filesystem or network
2. **WebSocket input** — malformed messages from a connected viewer
3. **Snapshot deserialization** — crafted bincode files could exploit deserialization bugs

## Rhai Sandbox
The scripting engine enforces strict limits:
- **Operation limit:** 100,000 operations per rule evaluation (prevents infinite loops)
- **String size limit:** 1,024 bytes
- **Array size limit:** 1,000 elements
- **Map size limit:** 100 entries
- **No filesystem access:** Rules cannot read/write files
- **No network access:** Rules cannot make network calls
- **Registered functions only:** The following categories are available:
  - **Core:** `set(field, value)`, `log(msg)` — mutation and debugging
  - **RNG:** `rand()`, `rand_range(min, max)` — deterministic pseudo-random via xorshift64
  - **Math:** `sin_deg(deg)`, `cos_deg(deg)`, `sqrt(x)`, `abs(v)`, `clamp(v, min, max)` — trigonometry and clamping
  - **Spatial:** `wind_align(from_x, from_y, to_x, to_y, wind_dir)`, `direction_to(from_x, from_y, to_x, to_y)` — directional wind/position calculations in native Rust
  - **Aggregate:** `neighbor_avg(neighbors, path)`, `neighbor_sum(neighbors, path)`, `neighbor_max(neighbors, path)` — native neighbor field aggregation via dot-path (e.g., "weather.temperature")

## WebSocket Server
- Binds to 127.0.0.1 by default (local only)
- No authentication (single-user local tool)
- Client messages are ignored (read-only stream from server to client)
- Graceful handling of client disconnects and lagged clients

## Snapshot Integrity
- Bincode deserialization uses Rust's type system for structural validation
- Snapshots are local files written by the application itself
- No untrusted snapshot sources in normal operation

## Network Exposure
- WebSocket server on configurable port (default 8118), localhost only
- HTTP health endpoint on same port, localhost only
- No outbound network connections

## Recommendations
- Keep WebSocket bind address as 127.0.0.1 for local use
- If exposing to network, add authentication and rate limiting
- Validate snapshot file paths to prevent directory traversal
