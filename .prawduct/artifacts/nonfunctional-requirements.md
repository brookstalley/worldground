# Non-functional Requirements — Worldground
<!-- Artifact: Non-functional Requirements | Version: 1 | Tier: 2 -->
<!-- Inferred from codebase analysis and test assertions — verify with product owner -->

## Performance

### Tick Rate Targets
| World Size | Target Tick Rate | Measured (M-series Mac) |
|-----------|-----------------|------------------------|
| 1,024 tiles | ≥5 ticks/sec | ~8.5 ticks/sec |
| 4,096 tiles | ≥2 ticks/sec | ~3.3 ticks/sec |
| 16,256 tiles | ≥1 tick/sec | ~1.0 tick/sec |

### Phase Budget (10K tiles, release build)
| Phase | Budget |
|-------|--------|
| Weather | ≤200ms |
| Conditions | ≤120ms |
| Terrain | ≤200ms |
| Resources | ≤120ms |
| Statistics | ≤50ms |
| **Total tick** | **≤1000ms** |

Note: Rhai interpreter overhead creates a ~100-120ms floor per phase at 10K tiles regardless of rule complexity.

### Per-tile Rule Timeout
- Configurable via `rule_timeout_ms` (default: 10ms)
- Enforced via Rhai operation limits (100K operations ≈ 10-50ms)

## Memory
- **Target:** <50MB peak for 10K tiles
- **Measurement:** tile_stack_size + heap_per_tile (neighbors + resources) × tile_count × 2 (double buffer)
- **Enforced by test:** `memory_estimate_10k_tiles_under_50mb`

## Scalability
- Tile counts: 100 to 16K+ (practical limit determined by Rhai interpretation speed)
- Parallel evaluation scales with CPU cores via rayon work-stealing
- Measured ~3x speedup on 10-core machines at large tile counts

## Reliability
- **Cascade detection:** Warns when >10% of tiles produce rule errors in a single tick
- **Error isolation:** A rule error on one tile discards that tile's mutations but doesn't affect other tiles
- **Snapshot recovery:** Auto-save at configurable intervals enables rollback

## Determinism
- Same seed + same rules = identical simulation state after any number of ticks
- Enforced by test: `simulation_determinism_100_ticks`
- Uses xorshift64 PRNG seeded per-tile for rule randomness
