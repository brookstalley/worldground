# Configuration Spec — Worldground
<!-- Artifact: Configuration Spec | Version: 1 | Tier: 2 -->
<!-- Inferred from codebase analysis — verify with product owner -->
<!-- sourced: config.toml, worldgen.toml, src/config/, 2026-02-20 -->

## Simulation Configuration (config.toml)

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| tick_rate_hz | f32 | 1.0 | Target ticks per second |
| snapshot_interval | u32 | 100 | Ticks between auto-saves |
| max_snapshots | u32 | 10 | Maximum snapshots retained |
| snapshot_directory | String | "./snapshots" | Snapshot storage path |
| websocket_port | u16 | 8118 | WebSocket server port |
| websocket_bind | String | "127.0.0.1" | Server bind address |
| rule_directory | String | "./rules" | Path to Rhai rule scripts |
| log_level | String | "info" | Logging verbosity |
| season_length | u32 | 90 | Ticks per season |
| rule_timeout_ms | u64 | 10 | Per-tile rule execution limit |
| native_evaluation | bool | true | Enable native Rust evaluation for weather phase, bypassing Rhai for ~10x speedup |

## World Generation Configuration (worldgen.toml)

| Parameter | Type | Default | Range | Description |
|-----------|------|---------|-------|-------------|
| seed | u64 | 0 | 0 = random | Deterministic generation seed |
| tile_count | u32 | 16000 | ≥100 | Number of hex tiles |
| ocean_ratio | f32 | 0.6 | 0.0-1.0 | Fraction that is ocean |
| mountain_ratio | f32 | 0.1 | 0.0-0.5 | Fraction of land that is mountainous |
| elevation_roughness | f32 | 0.5 | 0.0-1.0 | Terrain variation intensity |
| climate_bands | bool | true | - | Enable latitude-based climate zones |
| resource_density | f32 | 0.3 | 0.0-1.0 | Resource scattering density |
| initial_biome_maturity | f32 | 0.5 | 0.0-1.0 | Initial biome establishment level |

### Optional: [topology] section
| Parameter | Type | Default | Range | Description |
|-----------|------|---------|-------|-------------|
| mode | String | "flat" | "flat" or "geodesic" | Grid topology type |
| subdivision_level | u32 | 4 | 1-7 | Geodesic icosphere subdivision level. Tile count = 10 * 4^level + 2. Level 4 = 2,562 tiles. |

If the `[topology]` section is omitted, defaults to flat hex grid.

## CLI Overrides

The `run` subcommand accepts overrides for simulation config:
- `--world PATH` → load a specific snapshot file instead of generating fresh
- `--worldgen FILE` → use alternate generation config (default: worldgen.toml)
- `--tick-rate HZ` → overrides `tick_rate_hz`
- `--port PORT` → overrides `websocket_port`
- `--log-level LEVEL` → overrides `log_level`
- `--config FILE` → use alternate config file (default: config.toml)

The `generate` subcommand accepts:
- `--worldgen FILE` → use alternate generation config (default: worldgen.toml)
- `--output DIR` → override snapshot output directory

## Rule Configuration
Rules are Rhai scripts organized by phase directory:
```
rules/
├── weather/       # Phase 1: wind, temperature, humidity, clouds, storms
├── conditions/    # Phase 2: soil moisture, snow, mud, flood, fire risk
├── terrain/       # Phase 3: biome pressure, vegetation, transitions
└── resources/     # Phase 4: regeneration, consumption
```
Scripts execute in filename order within each phase. No configuration file needed — the directory structure IS the configuration.
