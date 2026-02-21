use crate::simulation::engine::{Phase, TileMutations};
use crate::world::tile::Season;
use crate::world::Tile;

/// Trait for native Rust phase evaluators that bypass the Rhai scripting layer.
///
/// Implementations may improve upon Rhai behavior (e.g., chaining rule outputs
/// via shared accumulators instead of snapshot-only reads). All arithmetic should
/// use f64 (matching Rhai's numeric type), and mutations should go through the
/// same apply_mutations path.
pub trait NativePhaseEvaluator: Send + Sync {
    /// Which simulation phase this evaluator handles.
    fn phase(&self) -> Phase;

    /// Evaluate a single tile and return mutations.
    ///
    /// `neighbors` contains references to tiles in their pre-phase state.
    /// `rng_seed` is the same deterministic seed that the Rhai evaluator receives.
    fn evaluate(
        &self,
        tile: &Tile,
        neighbors: &[&Tile],
        season: Season,
        tick: u64,
        rng_seed: u64,
    ) -> TileMutations;
}
