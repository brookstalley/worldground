# Learnings

Accumulated wisdom from building this product.

## Weather parameter coupling

Weather simulation parameters are tightly coupled — saturation base, maritime moisture boost, cloud formation curve, and viewer rendering alpha all interact to produce the visual result. First tuning pass (evaporation diminishing returns) fixed ocean-only clouds but exposed polar overload and lack of land clouds. Second pass adjusted saturation base (0.08→0.40), added maritime moisture boost, flattened cloud curve, and changed viewer alpha (cc²→cc^1.5). Both passes passed all tests — the issue was visual, not logical.

**Takeaway:** When tuning interconnected simulation parameters, expect multiple iteration rounds. Change one axis at a time and visually verify after each, not just test-verify.

## Humidity blending and absent data sources

The humidity blending formula assumed macro weather coverage was always present, using a fixed 0.6 weight for macro humidity. When no macro weather system covered a tile, macro_humidity=0 caused 73% humidity loss per tick, collapsing all non-ocean tiles to desert within ~100 ticks.

**Fix:** Make macro weight dynamic — proportional to actual coverage strength: `macro_weight = min(macro_humidity * 3.5, 0.35)`. Added evapotranspiration feedback, snowmelt moisture, moisture-dependent decay, and reduced precipitation consumption.

**Takeaway:** Blending formulas that weight multiple data sources must degrade gracefully when a source is absent or zero. Never use a fixed weight for an optional input.

## Serde tagged enums vs bincode

Serde tagged enums (`#[serde(tag = "type")]`) are incompatible with bincode deserialization — bincode doesn't implement `deserialize_any`. Config structs that must serialize to both TOML (human-readable) and bincode (snapshots) need flat structs with string mode fields instead of tagged enums.

**Takeaway:** When a type participates in both human-readable and binary serialization, avoid serde enum tagging. Use flat structs with a string discriminant field.

## Native Rust acceleration for Rhai hot paths

Rhai interpreter overhead creates a ~130-140ms floor per phase at 10K tiles regardless of rule complexity (scope setup, immutable map cloning, interpreter startup per tile). Moving hot-path neighbor iteration and trig operations to native Rust functions (registered in the Rhai engine) gave order-of-magnitude speedup without changing the rule authoring interface.

**Takeaway:** Profile before optimizing Rhai rules. If the bottleneck is interpreter overhead on tight numeric loops, register native functions rather than optimizing the script.
