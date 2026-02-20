use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

use crate::world::World;

/// Metadata about a snapshot file on disk.
#[derive(Debug, Clone)]
pub struct SnapshotMetadata {
    pub path: PathBuf,
    pub tick_count: u64,
    pub timestamp: u64,
    pub file_size: u64,
}

/// Errors that can occur during snapshot operations.
#[derive(Debug)]
pub enum SnapshotError {
    Io(io::Error),
    Serialize(String),
    Deserialize(String),
    Corrupt(PathBuf),
    NoValidSnapshots,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Io(e) => write!(f, "I/O error: {}", e),
            SnapshotError::Serialize(e) => write!(f, "Serialization error: {}", e),
            SnapshotError::Deserialize(e) => write!(f, "Deserialization error: {}", e),
            SnapshotError::Corrupt(path) => {
                write!(f, "Corrupt snapshot: {}", path.display())
            }
            SnapshotError::NoValidSnapshots => {
                write!(
                    f,
                    "No valid snapshots found. Generate a new world with: worldground generate"
                )
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<io::Error> for SnapshotError {
    fn from(e: io::Error) -> Self {
        SnapshotError::Io(e)
    }
}

/// Build a snapshot filename from tick count and timestamp.
fn snapshot_filename(tick_count: u64, timestamp: u64) -> String {
    format!("world-tick{}-{}.bin", tick_count, timestamp)
}

/// Parse tick count and timestamp from a snapshot filename.
/// Expected format: `world-tick{N}-{timestamp}.bin`
fn parse_snapshot_filename(filename: &str) -> Option<(u64, u64)> {
    let stem = filename.strip_suffix(".bin")?;
    let rest = stem.strip_prefix("world-tick")?;
    let (tick_str, ts_str) = rest.split_once('-')?;
    let tick = tick_str.parse::<u64>().ok()?;
    let ts = ts_str.parse::<u64>().ok()?;
    Some((tick, ts))
}

fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Save a world snapshot to the snapshot directory using atomic write.
///
/// Writes to a temporary file first, then atomically renames to the final path.
/// This ensures a partial write never corrupts an existing snapshot.
pub fn save_snapshot(world: &World, snapshot_dir: &Path) -> Result<PathBuf, SnapshotError> {
    fs::create_dir_all(snapshot_dir)?;

    let ts = unix_timestamp_now();
    let filename = snapshot_filename(world.tick_count, ts);
    let target = snapshot_dir.join(&filename);
    let tmp = snapshot_dir.join(format!(".{}.tmp", filename));

    let encoded = bincode::serialize(world).map_err(|e| SnapshotError::Serialize(e.to_string()))?;

    // Write to temp file, then atomic rename
    if let Err(e) = fs::write(&tmp, &encoded) {
        // Clean up temp file on failure
        let _ = fs::remove_file(&tmp);
        return Err(SnapshotError::Io(e));
    }

    if let Err(e) = fs::rename(&tmp, &target) {
        let _ = fs::remove_file(&tmp);
        return Err(SnapshotError::Io(e));
    }

    Ok(target)
}

/// Load a world from a snapshot file.
///
/// Validates that the deserialized world has consistent tile count.
pub fn load_snapshot(path: &Path) -> Result<World, SnapshotError> {
    let data = fs::read(path)?;
    let world: World =
        bincode::deserialize(&data).map_err(|e| SnapshotError::Deserialize(e.to_string()))?;

    // Validate tile count consistency
    if world.tiles.len() as u32 != world.tile_count {
        return Err(SnapshotError::Corrupt(path.to_path_buf()));
    }

    Ok(world)
}

/// List all valid snapshots in a directory, sorted by timestamp descending (newest first).
pub fn list_snapshots(snapshot_dir: &Path) -> Result<Vec<SnapshotMetadata>, SnapshotError> {
    if !snapshot_dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();

    for entry in fs::read_dir(snapshot_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip temp files
        if filename.starts_with('.') {
            continue;
        }

        if let Some((tick_count, timestamp)) = parse_snapshot_filename(&filename) {
            let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            snapshots.push(SnapshotMetadata {
                path: path.clone(),
                tick_count,
                timestamp,
                file_size,
            });
        }
    }

    // Sort by timestamp descending (newest first), then tick count as tiebreaker
    snapshots.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then(b.tick_count.cmp(&a.tick_count))
    });

    Ok(snapshots)
}

/// Prune old snapshots, keeping only the `max_snapshots` most recent.
///
/// Returns the list of deleted file paths.
pub fn prune_snapshots(
    snapshot_dir: &Path,
    max_snapshots: usize,
) -> Result<Vec<PathBuf>, SnapshotError> {
    let snapshots = list_snapshots(snapshot_dir)?;

    let mut deleted = Vec::new();
    if snapshots.len() > max_snapshots {
        for snapshot in &snapshots[max_snapshots..] {
            fs::remove_file(&snapshot.path)?;
            deleted.push(snapshot.path.clone());
        }
    }

    Ok(deleted)
}

/// Load the most recent valid snapshot, falling back to older ones if the latest is corrupt.
///
/// Returns an error only if no valid snapshots exist.
pub fn load_latest_valid_snapshot(snapshot_dir: &Path) -> Result<World, SnapshotError> {
    let snapshots = list_snapshots(snapshot_dir)?;

    if snapshots.is_empty() {
        return Err(SnapshotError::NoValidSnapshots);
    }

    for snapshot in &snapshots {
        match load_snapshot(&snapshot.path) {
            Ok(world) => return Ok(world),
            Err(e) => {
                warn!(
                    path = %snapshot.path.display(),
                    error = %e,
                    "Corrupt snapshot, trying next"
                );
            }
        }
    }

    Err(SnapshotError::NoValidSnapshots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::generation::GenerationParams;
    use crate::world::generation::generate_world;
    use std::time::Instant;
    use tempfile::TempDir;

    fn make_test_world(tile_count: u32) -> World {
        let params = GenerationParams {
            seed: 42,
            tile_count,
            ocean_ratio: 0.6,
            mountain_ratio: 0.1,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
            topology: crate::config::generation::TopologyConfig::default(),
        };
        generate_world(&params)
    }

    #[test]
    fn save_and_load_round_trip_identical() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(200);

        let path = save_snapshot(&world, dir.path()).unwrap();
        let restored = load_snapshot(&path).unwrap();

        assert_eq!(world, restored);
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(200);

        let path = save_snapshot(&world, dir.path()).unwrap();
        let restored = load_snapshot(&path).unwrap();

        assert_eq!(world.id, restored.id);
        assert_eq!(world.name, restored.name);
        assert_eq!(world.tick_count, restored.tick_count);
        assert_eq!(world.season, restored.season);
        assert_eq!(world.season_length, restored.season_length);
        assert_eq!(world.tile_count, restored.tile_count);
        assert_eq!(world.topology_type, restored.topology_type);
        assert_eq!(world.tiles.len(), restored.tiles.len());

        for (orig, rest) in world.tiles.iter().zip(restored.tiles.iter()) {
            assert_eq!(orig.id, rest.id);
            assert_eq!(orig.neighbors, rest.neighbors);
            assert_eq!(orig.geology, rest.geology);
            assert_eq!(orig.climate, rest.climate);
            assert_eq!(orig.biome, rest.biome);
            assert_eq!(orig.weather, rest.weather);
            assert_eq!(orig.conditions, rest.conditions);
            assert_eq!(orig.resources.resources.len(), rest.resources.resources.len());
        }
    }

    #[test]
    fn snapshot_filename_parse_round_trip() {
        let filename = snapshot_filename(500, 1708300000);
        assert_eq!(filename, "world-tick500-1708300000.bin");

        let (tick, ts) = parse_snapshot_filename(&filename).unwrap();
        assert_eq!(tick, 500);
        assert_eq!(ts, 1708300000);
    }

    #[test]
    fn parse_invalid_filename_returns_none() {
        assert!(parse_snapshot_filename("random.bin").is_none());
        assert!(parse_snapshot_filename("world-tick.bin").is_none());
        assert!(parse_snapshot_filename("world-tickabc-123.bin").is_none());
        assert!(parse_snapshot_filename("world-tick100-abc.bin").is_none());
        assert!(parse_snapshot_filename("not-a-snapshot.txt").is_none());
    }

    #[test]
    fn list_snapshots_returns_sorted_newest_first() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(100);
        let data = bincode::serialize(&world).unwrap();

        fs::write(dir.path().join("world-tick10-1000.bin"), &data).unwrap();
        fs::write(dir.path().join("world-tick20-2000.bin"), &data).unwrap();
        fs::write(dir.path().join("world-tick30-3000.bin"), &data).unwrap();

        let snapshots = list_snapshots(dir.path()).unwrap();
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].tick_count, 30);
        assert_eq!(snapshots[1].tick_count, 20);
        assert_eq!(snapshots[2].tick_count, 10);
    }

    #[test]
    fn list_snapshots_skips_non_snapshot_files() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(100);
        let data = bincode::serialize(&world).unwrap();

        fs::write(dir.path().join("world-tick10-1000.bin"), &data).unwrap();
        fs::write(dir.path().join("notes.txt"), "not a snapshot").unwrap();
        fs::write(dir.path().join(".world-tick99-9999.bin.tmp"), "temp file").unwrap();

        let snapshots = list_snapshots(dir.path()).unwrap();
        assert_eq!(snapshots.len(), 1);
    }

    #[test]
    fn list_snapshots_empty_dir() {
        let dir = TempDir::new().unwrap();
        let snapshots = list_snapshots(dir.path()).unwrap();
        assert!(snapshots.is_empty());
    }

    #[test]
    fn list_snapshots_nonexistent_dir() {
        let snapshots = list_snapshots(Path::new("/tmp/nonexistent_snapshot_dir_12345")).unwrap();
        assert!(snapshots.is_empty());
    }

    #[test]
    fn prune_keeps_max_snapshots() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(100);
        let data = bincode::serialize(&world).unwrap();

        for i in 0..6u64 {
            fs::write(
                dir.path()
                    .join(format!("world-tick{}-{}.bin", i * 10, 1000 + i)),
                &data,
            )
            .unwrap();
        }

        let deleted = prune_snapshots(dir.path(), 3).unwrap();
        assert_eq!(deleted.len(), 3);

        let remaining = list_snapshots(dir.path()).unwrap();
        assert_eq!(remaining.len(), 3);

        // The 3 newest should remain (highest timestamps)
        assert_eq!(remaining[0].timestamp, 1005);
        assert_eq!(remaining[1].timestamp, 1004);
        assert_eq!(remaining[2].timestamp, 1003);
    }

    #[test]
    fn prune_noop_when_under_limit() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(100);
        let data = bincode::serialize(&world).unwrap();

        fs::write(dir.path().join("world-tick10-1000.bin"), &data).unwrap();
        fs::write(dir.path().join("world-tick20-2000.bin"), &data).unwrap();

        let deleted = prune_snapshots(dir.path(), 5).unwrap();
        assert!(deleted.is_empty());

        let remaining = list_snapshots(dir.path()).unwrap();
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    fn load_corrupt_snapshot_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("world-tick0-1000.bin");
        fs::write(&path, b"this is not valid bincode data").unwrap();

        assert!(load_snapshot(&path).is_err());
    }

    #[test]
    fn load_truncated_snapshot_returns_error() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(100);
        let data = bincode::serialize(&world).unwrap();

        let path = dir.path().join("world-tick0-1000.bin");
        fs::write(&path, &data[..data.len() / 2]).unwrap();

        assert!(load_snapshot(&path).is_err());
    }

    #[test]
    fn load_latest_valid_falls_back_on_corrupt() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(100);
        let valid_data = bincode::serialize(&world).unwrap();

        // Oldest: valid
        fs::write(dir.path().join("world-tick10-1000.bin"), &valid_data).unwrap();
        // Newest: corrupt
        fs::write(
            dir.path().join("world-tick20-2000.bin"),
            b"corrupt data here",
        )
        .unwrap();

        let restored = load_latest_valid_snapshot(dir.path()).unwrap();
        assert_eq!(restored.tile_count, world.tile_count);
    }

    #[test]
    fn load_latest_valid_all_corrupt_returns_error() {
        let dir = TempDir::new().unwrap();

        fs::write(dir.path().join("world-tick10-1000.bin"), b"corrupt1").unwrap();
        fs::write(dir.path().join("world-tick20-2000.bin"), b"corrupt2").unwrap();

        let result = load_latest_valid_snapshot(dir.path());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SnapshotError::NoValidSnapshots
        ));
    }

    #[test]
    fn load_latest_valid_empty_dir_returns_error() {
        let dir = TempDir::new().unwrap();
        assert!(matches!(
            load_latest_valid_snapshot(dir.path()).unwrap_err(),
            SnapshotError::NoValidSnapshots
        ));
    }

    #[test]
    fn atomic_write_no_temp_files_remain() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(100);

        save_snapshot(&world, dir.path()).unwrap();

        let temp_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with('.'))
            })
            .collect();
        assert!(temp_files.is_empty());
    }

    #[test]
    fn serialization_round_trip_10k_tiles_within_one_second() {
        let world = make_test_world(10_000);

        let start = Instant::now();
        let encoded = bincode::serialize(&world).unwrap();
        let _decoded: World = bincode::deserialize(&encoded).unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 1000,
            "10K tile round-trip took {}ms, expected < 1000ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn save_creates_directory_if_missing() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("deep").join("nested").join("snapshots");
        let world = make_test_world(100);

        let path = save_snapshot(&world, &nested).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn multiple_saves_produce_distinct_files() {
        let dir = TempDir::new().unwrap();
        let world = make_test_world(100);

        let path1 = save_snapshot(&world, dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let path2 = save_snapshot(&world, dir.path()).unwrap();

        assert_ne!(path1, path2);
        assert!(path1.exists());
        assert!(path2.exists());
    }
}
