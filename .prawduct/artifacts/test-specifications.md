# Test Specifications — Worldground
<!-- Artifact: Test Specifications | Version: 1 | Tier: 2 -->
<!-- Inferred from codebase analysis — verify with product owner -->

## Test Infrastructure
- **Framework:** Rust built-in test framework (`#[test]`, `#[tokio::test]`)
- **Dev dependencies:** `tempfile` (temporary directories for rule engine tests)
- **Run:** `cargo test` (all tests), `cargo test --release` (with optimizations for performance tests)
- **Test location:** Inline `#[cfg(test)] mod tests` in each source file (12 files, 118 tests total)

## Test Coverage by Module

### Rule Engine (src/simulation/engine.rs) — 17 tests
| Test | What it validates |
|------|-------------------|
| load_valid_rules | Rhai scripts compile and load from phase directories |
| missing_rule_dir_error | Missing rule directory produces clear error |
| rhai_syntax_error_detected | Malformed scripts are caught at load time |
| empty_phase_dir_is_noop | Phases without rules produce no mutations |
| rule_reads_tile_data | Rules can access tile.geology, tile.climate, etc. |
| rule_reads_neighbors | Rules can iterate over neighbor tile states |
| rule_reads_season_and_tick | Season and tick are available as constants |
| rule_error_returns_rule_error | Runtime errors report tile ID and rule name |
| rule_timeout_enforced | Infinite loops are terminated within seconds |
| apply_weather_mutations | Weather field mutations apply correctly |
| apply_conditions_mutations_with_clamping | Values clamp to valid ranges |
| apply_terrain_mutations | Biome/vegetation mutations apply correctly |
| wrong_phase_mutations_ignored | Cross-phase mutations are rejected |
| multiple_rules_last_write_wins | Later rules overwrite earlier set() calls |
| rules_sorted_by_filename | Rules execute in deterministic filename order |
| xorshift64_deterministic | PRNG is reproducible given same seed |
| tile_to_map_has_all_layers | Rhai map includes all 6 tile layers |

### Simulation Loop (src/simulation/mod.rs) — 12 tests
| Test | What it validates |
|------|-------------------|
| single_tick_produces_state_changes | One tick modifies world state |
| phase_ordering_causal_chain | Rain → moisture → vegetation health chain works |
| season_advances_at_interval | Season changes every N ticks |
| season_full_cycle | 4 season changes return to start |
| ticks_in_current_biome_increments | Biome stability counter increases each tick |
| simulation_determinism_100_ticks | Same seed produces identical results over 100 ticks |
| multi_tick_evolution_400_ticks | Full year produces biome transitions and diversity changes |
| established_biome_resists_change | Old biomes resist transition more than young ones |
| cascade_detection_with_failing_rules | >10% tile errors triggers warning |
| performance_10k_tiles_100_ticks | ≤1000ms/tick at 10K tiles (release) |
| per_phase_timing_within_budget | Each phase within its timing budget |
| memory_estimate_10k_tiles_under_50mb | Peak memory under 50MB for 10K tiles |

### Phase Execution (src/simulation/phase.rs) — 7 tests
Tests parallel phase execution, double-buffering, error isolation, and mutation application.

### Statistics (src/simulation/statistics.rs) — 6 tests
Tests biome distribution, diversity index, average calculations, and statistics computation.

### Server (src/server/mod.rs + protocol.rs) — 19 tests
| Test | What it validates |
|------|-------------------|
| build_snapshot_json_is_valid | Snapshot serialization produces valid JSON |
| build_diff_json_is_valid | Diff serialization includes only changed layers |
| build_diff_json_empty_when_no_changes | No changes = empty diff |
| server_state_on_tick_updates_health | Health data reflects latest tick |
| server_state_updates_snapshot_for_new_clients | New snapshot available for late joiners |
| tick_rate_calculation | Tick rate computed from recent durations |
| health_recent_durations_capped_at_100 | Rolling window doesn't grow unbounded |
| broadcast_diff_to_subscribers | Tick diffs reach subscribed clients |
| websocket_client_receives_snapshot_and_diff | Full integration: connect → snapshot → diff |
| health_endpoint_returns_json | HTTP GET /health returns valid JSON |
| client_disconnect_does_not_crash_server | Graceful disconnect handling |
| world_snapshot_contains_all_tiles | Snapshot includes every tile |
| snapshot_serializes_to_json | Snapshot produces valid JSON |
| diff_detects_weather_change | Weather-only changes detected |
| diff_detects_multiple_layer_changes | Multi-layer changes detected |
| diff_empty_when_no_changes | No false positives |
| diff_only_includes_changed_tiles | Unchanged tiles excluded |
| tick_diff_serializes_to_json | Diff JSON valid, null layers omitted |
| health_status_serializes | Health endpoint JSON valid |

### World (src/world/tile.rs + generation.rs + topology.rs) — 26 tests
Tests tile creation, serde round-trip, season cycling, enum serialization, world generation determinism, biome distribution, climate bands, topology neighbors, hex grid edge cases.

### Config (src/config/generation.rs + simulation.rs) — 20 tests
Tests TOML parsing, default values, validation, error handling for both config files.

### Persistence (src/persistence/snapshot.rs) — 19 tests
Tests snapshot save/load round-trip, directory management, listing, pruning, error cases.

## Coverage Gaps
- **Viewer (viewer/index.html):** No automated tests. Manual testing only.
- **CLI integration:** No end-to-end tests for full CLI commands. Individual components are well-tested.
- **Production rules (rules/*.rhai):** Not directly tested. Engine tests verify execution mechanics using inline test rules.

## Performance Test Notes
- Performance tests use `cfg!(debug_assertions)` to adjust expectations for debug vs release builds
- Debug mode: Rhai is 10-50x slower unoptimized — thresholds are 10x more lenient
- Run `cargo test --release` for accurate performance measurements
