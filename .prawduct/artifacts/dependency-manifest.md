# Dependency Manifest — Worldground
<!-- Artifact: Dependency Manifest | Version: 1 | Tier: 2 -->
<!-- sourced: Cargo.toml, 2026-02-20 -->

## Runtime Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| rhai | 1.x (sync feature) | Scripting engine for simulation rules |
| rayon | 1.10 | Data-parallel tile evaluation |
| tokio | 1.x (full features) | Async runtime for WebSocket server |
| tokio-tungstenite | 0.26 | WebSocket protocol implementation |
| futures-util | 0.3 | Stream/Sink utilities for WebSocket handling |
| serde | 1.x (derive feature) | Serialization framework |
| serde_json | 1.x | JSON serialization for WebSocket protocol |
| bincode | 1.x | Binary serialization for snapshots |
| toml | 0.8 | TOML config file parsing |
| noise | 0.9 | Perlin/simplex noise for procedural terrain generation |
| rand | 0.8 | Random number generation |
| rand_chacha | 0.3 | Deterministic RNG for world generation |
| clap | 4.x (derive feature) | CLI argument parsing |
| tracing | 0.1 | Structured logging |
| tracing-subscriber | 0.3 (json, env-filter) | Log output formatting and filtering |
| uuid | 1.x (v4, serde) | World ID generation |

## Dev Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| tempfile | 3.x | Temporary directories for rule engine tests |

## Key Dependency Relationships
- **rhai** is the scripting engine at the core of the simulation — it's the performance bottleneck
- **rayon** provides the parallelism that makes large worlds viable
- **tokio** + **tokio-tungstenite** handle the WebSocket server for real-time visualization
- **serde** + **serde_json** + **bincode** handle all serialization (JSON for network, bincode for persistence)
- **noise** + **rand** + **rand_chacha** handle procedural world generation
