# Operational Spec — Worldground
<!-- Artifact: Operational Spec | Version: 1 | Tier: 2 -->
<!-- Inferred from codebase analysis — verify with product owner -->

## Deployment
Worldground is a local application. No cloud deployment, containers, or CI/CD pipeline.

### Build
```sh
cargo build --release  # Recommended for worlds over 1K tiles
```

### Run
```sh
# Generate a world
cargo run --release -- generate [--worldgen worldgen.toml] [--output snapshots]

# Start simulation server
cargo run --release -- run [--world PATH] [--tick-rate HZ] [--port PORT]

# Serve viewer (separate terminal)
cd viewer && python3 -m http.server 8081
```

## Configuration Files
- `config.toml` — Simulation runtime settings (tick rate, snapshot interval, ports, rule directory)
- `worldgen.toml` — World generation parameters (seed, tile count, ratios)

## Logging
- Uses `tracing` crate with structured logging
- Default level: `info` (configurable via `config.toml` or `--log-level` CLI flag)
- Supports `RUST_LOG` environment variable for fine-grained control
- Key log events: server start, WebSocket connect/disconnect, rule cascade warnings

## Monitoring
- **HTTP /health endpoint** on WebSocket port, returns JSON:
  - `tick`: current tick count
  - `tick_rate`: calculated ticks/sec
  - `diversity_index`: biome diversity measure
  - `rule_errors`: error count
  - `snapshot_age_ticks`: ticks since last snapshot
  - `tile_count`: world size
  - `season`: current season

## Persistence
- **Snapshots:** Bincode files in configurable directory (default: `./snapshots`)
- **Auto-save:** Every N ticks (default: 100)
- **Pruning:** Keeps max N snapshots (default: 10), deletes oldest
- **Recovery:** `worldground snapshots restore FILE` loads a previous state

## Resource Requirements
- **CPU:** Benefits from multiple cores (rayon parallelism)
- **Memory:** ~50MB peak for 10K tiles
- **Disk:** Snapshots are compact bincode (~few MB per 16K-tile world)
- **Network:** Localhost only (WebSocket + HTTP health)
