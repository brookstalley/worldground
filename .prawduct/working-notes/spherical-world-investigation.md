# Investigation: Genuinely Spherical World Model

**Created:** 2026-02-20
**Status:** Investigation complete, awaiting decision

## Executive Summary

Making Worldground's world model genuinely spherical is feasible and the codebase is better-positioned for it than most. The tile model is already topology-agnostic (tiles are just IDs with neighbor lists). The main work is: (1) replacing the flat hex grid generator with a geodesic sphere generator, (2) fixing ~5 position-dependent functions in the engine and rules, and (3) updating the viewer to render a sphere or spherical projection.

The recommended approach is **icosahedron subdivision** (geodesic polyhedron), which produces a hex grid with exactly 12 pentagons. This is the same approach used by climate models (NICAM, MPAS-A) and is the most practical for simulation.

**Estimated scope:** ~15-20 files touched, structural change. The simulation engine and rule system need moderate changes; the viewer needs a full rewrite of its rendering pipeline.

---

## Current State: What's Planar

### Already topology-agnostic (no changes needed)

These components work with any topology because they only access tiles by ID and neighbor list:

- **`Tile` struct** — `id: u32`, `neighbors: Vec<u32>`, `position: Position`
- **Simulation tick loop** (`simulation/mod.rs`) — iterates phases, advances tick/season
- **Phase execution** (`simulation/phase.rs`) — double-buffered parallel eval via rayon
- **Mutation application** (`simulation/engine.rs` `apply_mutations()`)
- **Statistics computation** (`simulation/statistics.rs`)
- **Snapshot save/load** (`persistence/snapshot.rs`)
- **Neighbor aggregation functions** — `neighbor_avg`, `neighbor_sum`, `neighbor_max`
- **All Conditions rules** (soil moisture, snow/mud) — don't use position
- **All Terrain rules** (biome pressure, vegetation, transitions) — don't use position
- **Resource regeneration rule** — doesn't use position
- **`TopologyType::Geodesic` enum variant** — already exists in the type system (unused)

### Planar assumptions that must change

| # | Location | What it assumes | Impact |
|---|----------|----------------|--------|
| 1 | `world/topology.rs` `generate_flat_hex_grid()` | Rectangular grid with toroidal wrapping | **Replace entirely** — need geodesic grid generator |
| 2 | `world/topology.rs` `offset_to_pixel()` | Planar hex position formula | **Replace** — positions become 3D (or lat/lon) |
| 3 | `world/topology.rs` `grid_dimensions()` | Width × height rectangle | **Replace** — geodesic grids use subdivision level, not dimensions |
| 4 | `world/generation.rs` `generate_elevation()` | Perlin noise at planar (x,y) | **Adapt** — sample 3D noise at (x,y,z) on unit sphere |
| 5 | `world/generation.rs` `assign_climate()` | Latitude = linear from pixel Y | **Adapt** — latitude from spherical coordinates (trivial) |
| 6 | `world/generation.rs` `assign_soil()` | Perlin noise at planar (x,y) | **Adapt** — same as elevation |
| 7 | `simulation/engine.rs` `wind_align()` | Euclidean (dx,dy) for direction | **Rewrite** — use great-circle direction or tangent-plane vectors |
| 8 | `simulation/engine.rs` `direction_to()` | Euclidean (dx,dy) | **Rewrite** — same issue |
| 9 | `simulation/engine.rs` `tile_to_rhai_map()` | Exposes position.x, position.y | **Extend** — add lat/lon or use 3D coords |
| 10 | `rules/weather/01-wind-temperature.rhai` | Uses direction_to() with planar coords | **Adapt** — will work if direction_to() is fixed |
| 11 | `rules/weather/02-humidity.rhai` | Uses wind_align() with planar coords | **Adapt** — will work if wind_align() is fixed |
| 12 | `rules/weather/04-storms.rhai` | Uses wind_align() with planar coords | **Adapt** — will work if wind_align() is fixed |
| 13 | `server/protocol.rs` `TileSnapshot` | Sends Position {x, y} | **Extend** — send lat/lon or 3D coords |
| 14 | `viewer/index.html` | Renders tiles at flat (x*hexSize, y*hexSize) | **Major rewrite** — need spherical projection or 3D rendering |

### Existing bug discovered during investigation

`wind_align()` and `direction_to()` in `engine.rs` compute directions using raw `(x2-x1, y2-y1)` **without toroidal wrapping**. For tiles that are neighbors across the wrap boundary (col 0 ↔ col width-1, or row 0 ↔ row height-1), the direction vector points the wrong way — spanning nearly the entire grid instead of the short hop across the seam. This means weather rules produce incorrect results for ~10-15% of tiles near wrap edges. A spherical model would fix this entirely since there are no seam discontinuities in direction calculations on a sphere.

---

## Approach Options

### Option A: Icosahedron Subdivision (Recommended)

Start with a regular icosahedron (20 triangular faces). Subdivide each face into smaller triangles, project vertices to the unit sphere, take the dual graph. Result: a hex grid on a sphere with exactly 12 pentagons (at the original icosahedron vertices).

**Tile counts by subdivision level:**

| Level | Tiles | Hexagons | Pentagons | Comparable to current |
|-------|-------|----------|-----------|----------------------|
| 3 | 642 | 630 | 12 | ~1K config |
| 4 | 2,562 | 2,550 | 12 | ~4K config |
| 5 | 10,242 | 10,230 | 12 | ~16K config |
| 6 | 40,962 | 40,950 | 12 | — |

**Pros:**
- Standard approach used by real climate models (NICAM, MPAS-A, ICON)
- All tiles nearly equal area (can be improved with spring relaxation or ISEA projection)
- No seam discontinuities — neighbor relationships are seamless
- 12 pentagons is the theoretical minimum; manageable special cases
- Cache-friendly: 20 face-local patches for data layout
- Parallelize naturally over 20 faces (or over tiles as today)

**Cons:**
- 12 tiles have 5 neighbors instead of 6 — rules must handle this
- Tile counts jump by 4× per level (can't get exactly 4,000 tiles)
- No mature simulation-ready Rust crate (hexasphere provides geometry + adjacency but not simulation primitives)
- Viewer must switch to spherical projection or WebGL 3D

**Rust crate: `hexasphere`**
- Generates the geodesic mesh from an icosahedron
- Provides vertex positions on the unit sphere
- Can compute adjacency (with feature flag)
- Does NOT provide equal-area correction, lat/lon conversion, or face-local indexing
- We'd build the simulation grid on top of its geometry

### Option B: H3 (Uber's Hexagonal Grid)

Aperture-7 icosahedral grid with hierarchical 64-bit cell indexing.

**Pros:**
- Mature Rust crate (`h3o`) with excellent performance
- Built-in lat/lon conversion
- Fast neighbor queries
- Well-tested at scale

**Cons:**
- **Not equal area** — largest/smallest cells differ by ~2× at any resolution
- Designed for geospatial indexing, not physical simulation
- Tile counts are very large at useful resolutions (41K at res 3, 288K at res 4)
- No fine control over tile count
- The aperture-7 structure means tile counts jump by 7× per level
- Overkill dependency for a local simulation engine

**Verdict:** Wrong tool for the job. H3 optimizes for fast spatial queries, not simulation correctness.

### Option C: Custom Goldberg Polyhedron

Build GP(m,n) directly, choosing m and n to hit desired tile counts.

**Pros:**
- More tile count options than pure aperture-4 subdivision
- Well-understood mathematics

**Cons:**
- No Rust library — implement from scratch
- Class III (m≠n) grids are chiral and harder to index
- Addressing/neighbor lookup requires building the full index structure
- Class I GP(n,0) is exactly icosahedron subdivision (Option A)

**Verdict:** If Option A's tile count jumps (×4) are a problem, GP(m,n) provides intermediate sizes. Otherwise, Option A is simpler.

### Option D: Cylindrical Approximation

Wrap east-west (already done via toroidal wrapping), but fix the poles — either close them or make polar tiles smaller/special.

**Pros:**
- Minimal change to existing code
- Latitude/longitude mapping is natural
- No pentagons
- Viewer changes are minimal (Mercator-like projection already)

**Cons:**
- **Not genuinely spherical** — still a cylinder with pole caps
- Area distortion at poles (Mercator problem)
- Polar tiles either need special handling or the world has hard latitude boundaries
- Wind/weather at poles is physically wrong

**Verdict:** This is what Civilization does. It's the pragmatic choice if "genuinely spherical" isn't a hard requirement.

---

## Recommended Approach: Icosahedron Subdivision

### Implementation Plan Sketch

#### Phase 1: Core Topology (world/topology.rs)

1. Add `generate_geodesic_grid(subdivision_level: u32) -> (Vec<Tile>, TopologyType)` using the `hexasphere` crate to generate vertices and adjacency
2. Each vertex of the dual graph (hexasphere triangle center → hex center, or hexasphere vertex → hex vertex) becomes a tile
3. Store 3D position on the unit sphere: extend `Position` to include `(x, y, z)` or add a `SphericalPosition { lat, lon, x, y, z }`
4. Derive `lat` and `lon` from 3D position: `lat = asin(z)`, `lon = atan2(y, x)`
5. Neighbor list comes from hexasphere's adjacency computation
6. Pentagon tiles naturally have 5 neighbors; hexagons have 6

#### Phase 2: World Generation (world/generation.rs)

1. **Elevation:** Replace 2D Perlin noise with 3D Perlin noise sampled at `(x, y, z)` on the unit sphere. The `noise` crate already supports 3D. This naturally produces continuous, seamless noise on the sphere — no wrapping artifacts.
2. **Climate:** Latitude is `asin(z)` — trivial and physically correct. Polar tiles naturally cluster near the poles with correct area ratios.
3. **Soil:** Same as elevation — 3D noise.
4. **Biome/resource/weather init:** Already position-independent or use latitude; minimal changes.

#### Phase 3: Simulation Engine (simulation/engine.rs)

1. **`direction_to()`:** Replace Euclidean subtraction with tangent-plane direction. On a sphere, the direction from tile A to tile B is the projection of (B - A) onto A's tangent plane, normalized. For adjacent tiles this is straightforward:
   ```
   d = B_pos - A_pos  (3D vector)
   d_tangent = d - (d · A_normal) * A_normal  (project onto tangent plane)
   normalize(d_tangent)
   ```
   Then convert to a (dx, dy) in A's local tangent frame (east, north basis).

2. **`wind_align()`:** Uses `direction_to()` internally — once that's fixed, wind_align works.

3. **Expose to Rhai:** Add `tile.lat`, `tile.lon` to the Rhai map. Keep `position.x/y` as the flat-map projection for backward compatibility, or replace entirely with lat/lon.

4. **Pentagon handling in rules:** The 12 pentagon tiles have 5 neighbors instead of 6. The `neighbor_avg/sum/max` functions already handle variable-length neighbor lists (they iterate `neighbors`). Weather rules that use `wind_align` for directional transport should still work — the direction computation is neighbor-relative, not count-dependent. The only concern is biome adjacency constraints that assume 6 neighbors.

#### Phase 4: Weather Rules (rules/)

1. Rules that use `direction_to()` or `wind_align()` will work correctly once the engine functions are fixed
2. The Hadley cell wind model (`01-wind-temperature.rhai`) uses latitude bands — this works better on a sphere since latitude is now physically meaningful
3. Storm propagation and humidity advection are directional — they'll work correctly with proper tangent-plane directions
4. **Polar convergence:** On a flat grid, wind from the north and south never converges. On a sphere, winds converge at the poles. This is a simulation improvement, not a bug — but may produce unexpected weather patterns at poles that need tuning.

#### Phase 5: Viewer (viewer/index.html)

This is the largest single piece of work. Options:

**5a. Orthographic/stereographic 2D projection** (moderate effort):
- Project 3D sphere coordinates to a 2D viewport using an orthographic or equal-area projection
- Render hexes as projected polygons on Canvas2D
- User rotates the "camera" to see different parts of the globe
- Simpler than full 3D; looks like a globe from one angle

**5b. WebGL 3D globe** (significant effort):
- Render tiles as 3D polygons on a sphere using WebGL/Three.js
- Full rotation, zoom, tilt
- Best visual result but major rewrite
- Three.js is ~500KB; adds dependency

**5c. Map projection** (least effort):
- Use an equal-area projection (Mollweide, Hammer, or equirectangular) to flatten the sphere
- Render hexes on Canvas2D at projected positions
- Familiar map-like view; shows the whole world at once
- Distortion at edges (like any map projection)
- Could offer multiple projections as overlay modes

**5d. Hybrid** (recommended):
- Default view: equirectangular or Mollweide projection (Canvas2D, sees everything)
- Optional 3D globe view (WebGL, for exploration/beauty)
- Toggle between them like current overlay modes

#### Phase 6: Server Protocol (server/protocol.rs)

1. Extend `TileSnapshot` to include `lat`, `lon` (and optionally `x`, `y`, `z`)
2. The viewer uses whichever coordinate system it needs for rendering
3. Backward-compatible if we keep the existing fields

#### Phase 7: Configuration

1. Replace `tile_count` with `subdivision_level` (or keep tile_count and auto-select the nearest subdivision level)
2. Remove `grid_dimensions()` — no longer applicable
3. Add `topology: "geodesic"` to generation config (keep `flat_hex` as an option for backward compatibility)

---

## The Pentagon Problem

Every spherical hex grid has exactly 12 pentagons (Euler's formula). These need explicit consideration:

**Where simulation rules are affected:**
- Weather rules that compute directional transport: 5 directions instead of 6. The `wind_align` approach (dot product with neighbor direction) handles this naturally — it just has fewer terms in the sum.
- Biome adjacency constraints: currently check all 6 neighbors for adjacency. Need to handle 5-neighbor case. Trivial code change.

**Where to place them:**
- At poles (6 top, 6 bottom) — worst for weather simulation since polar weather is complex
- At icosahedron vertices (distributed) — each pentagon is surrounded by hexagons, minimizing local impact
- **Recommended:** Use icosahedron vertices (the natural placement) and don't try to hide them. 12 out of 2,500+ tiles is 0.5% — negligible impact on simulation behavior.

**Practical impact:** In climate models like NICAM running at 3.5km resolution (~10M cells), the 12 pentagons cause zero measurable impact on simulation accuracy. At Worldground's scale (2K-10K tiles), the impact is even less significant relative to other approximations.

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| hexasphere crate doesn't provide what we need | Medium | It provides mesh + adjacency; we build indexing on top. Fallback: implement icosahedron subdivision from scratch (well-documented algorithm). |
| Performance regression from 3D noise / tangent-plane math | Low | 3D Perlin is ~30% slower than 2D; tangent-plane projection is a few multiplies per tile. Negligible at 10K tiles. |
| Weather rules produce unexpected behavior on sphere | Medium | Expected — polar convergence, Coriolis effects, etc. are physically correct but may need rule tuning. This is a feature, not a bug. |
| Viewer rewrite takes longer than simulation changes | High | The viewer is the biggest piece. A map projection approach minimizes effort; WebGL globe is optional. |
| Pentagon tiles cause simulation artifacts | Low | 12/2500 = 0.5%. Climate models demonstrate this is negligible. |
| Snapshot backward compatibility | Low | Version the snapshot format; add migration path. |

---

## Effort Estimate by Component

| Component | Files | Complexity | Notes |
|-----------|-------|-----------|-------|
| Topology generator | 1 (topology.rs) | High | Core new algorithm; hexasphere integration |
| Position model | 1 (tile.rs) | Low | Extend Position or add SphericalPosition |
| World generation | 1 (generation.rs) | Medium | 3D noise, spherical latitude, seam-free |
| Engine functions | 1 (engine.rs) | Medium | Tangent-plane direction math |
| Weather rules | 3 (rules/weather/) | Low | Work once engine is fixed; may need tuning |
| Server protocol | 1 (protocol.rs) | Low | Add lat/lon fields |
| Viewer | 1 (viewer/index.html) | High | Projection or 3D rendering |
| Configuration | 2 (generation.rs, config) | Low | Subdivision level parameter |
| Tests | ~6 files | Medium | Topology tests need full rewrite |
| Config files | 2 (worldgen.toml, config.toml) | Trivial | |

**Total: ~15-18 files, with topology.rs and viewer/index.html as the heavy lifts.**

---

## Open Questions for Decision

1. **Tile count flexibility:** Icosahedron subdivision gives tile counts at 642, 2562, 10242, 40962. The current default is 4,000. Accept 2,562 or 10,242? Or use Goldberg GP(m,n) for intermediate counts?

2. **Keep flat mode?** The `TopologyType` enum already has both variants. Maintain both as a config option, or go sphere-only?

3. **Viewer approach:** Map projection (fast, simple), WebGL globe (beautiful, complex), or hybrid?

4. **Equal-area correction:** Accept hexasphere's ~5% area variation, or implement spring relaxation / ISEA projection for exact equal area?

5. **3D noise library:** The existing `noise` crate supports 3D Perlin. Confirm it's sufficient or evaluate alternatives.

6. **Coordinate system for Rhai:** Expose (lat, lon)? (x, y, z)? Both? Replace or supplement the current (x, y)?
