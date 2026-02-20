# Product Brief — Worldground
<!-- Artifact: Product Brief | Version: 1 | Tier: 1 -->
<!-- Inferred from codebase analysis — verify with product owner -->
<!-- sourced: README.md, 2026-02-20 -->

## Vision
A perpetual world simulation engine that generates hex-tile worlds and evolves them through scriptable terrain, weather, and ecological rules — useful for game prototyping, systems exploration, and ambient visualization.

## Problem
There's no lightweight, moddable world simulation engine that lets you generate a world, watch it evolve in real time, and change the physics by editing scripts. Game developers prototyping world systems need to either build from scratch or use heavyweight engines. Hobbyists exploring emergent systems lack accessible tools for experimentation.

## Target Users

### World Builder
Someone exploring procedural generation and emergent simulation systems — game developers, hobbyists, or educators.
- **Technical level:** Intermediate (comfortable with scripting, config files, CLI tools)
- **Primary needs:** Generate and customize worlds, write Rhai rules, visualize behavior
- **Key constraint:** Wants fast iteration without recompilation

## Core Flows

### 1. World Generation (must-have)
Generate a hex-tile world from configurable parameters: seed, tile count, ocean ratio, mountain ratio, climate bands, resource density. Each tile gets 6 layers: Geology (immutable), Climate (immutable), Weather, Conditions, Biome, Resources. Output saved as bincode snapshot.

### 2. Simulation Execution (must-have)
Run continuous tick loop executing 4-phase Rhai rules: Weather → Conditions → Terrain → Resources. Each phase reads from a snapshot of the previous state (double-buffered). Tiles evaluated in parallel via rayon. Season advances every N ticks. Deterministic given same seed and rules.

### 3. Real-time Visualization (must-have)
Browser viewer connects via WebSocket. On connect, receives full WorldSnapshot. After each tick, receives TickDiff with only changed tile layers. Viewer renders hex map with 9 overlay modes: Biome, Terrain, Temperature, Humidity, Precipitation, Cloud Cover, Wind Speed, Vegetation, Elevation. Click any tile for full state inspection.

### 4. Rule Authoring (must-have)
Rhai scripts in rules/<phase>/ directories. Scripts receive tile state, neighbor states, season, and tick count. Call set("field", value) to propose mutations. Sandboxed: operation limits (100K), string size limits (1KB), array/map size limits. No recompilation needed — edit scripts and restart simulation.

### 5. Snapshot Management (must-have)
Auto-save at configurable intervals (default: every 100 ticks). Pruning to max N snapshots. CLI list/restore commands. Bincode format for fast save/load.

### 6. World Inspection (nice-to-have)
CLI inspect --tile ID shows full tile state. CLI inspect --world shows summary statistics.

## Success Criteria
- Diverse worlds generated from different seeds
- Weather patterns form and sweep across the world visibly
- Biome transitions occur naturally over time via adjacency-constrained rules
- 1+ ticks/sec at 16K tiles (release build)
- Simulation is deterministic — same seed produces same results
- Rules can be modified without recompilation

## Scope

### v1 (Implemented)
- Hex world generation with configurable parameters
- 4-phase simulation tick loop with 10 Rhai rules
- Browser hex viewer with 9 overlay modes
- WebSocket real-time streaming with diff protocol
- HTTP health endpoint
- Bincode snapshot persistence with auto-save and pruning
- CLI: generate, run, inspect, snapshots
- Season cycling, biome adjacency transitions
- Rayon parallel evaluation, sandboxed Rhai engine
- Deterministic simulation

## Key Risks
- **Rhai performance bottleneck:** Script interpretation at ~1ms/tile limits tick rate at large world sizes. Mitigated by rayon parallelism and operation limits.
- **Rule interaction complexity:** Emergent behavior from 10 rules across 4 phases can produce unexpected results. Mitigated by phase ordering (causal chains) and cascade detection (>10% error threshold).
