use std::collections::HashSet;

use hexasphere::shapes::IcoSphereBase;
use hexasphere::Subdivided;

use crate::world::tile::{Position, Tile};

/// Neighbor offsets for even rows (row % 2 == 0) in odd-r offset layout.
const EVEN_ROW_NEIGHBORS: [(i32, i32); 6] = [
    (1, 0),   // East
    (-1, 0),  // West
    (0, -1),  // Northeast
    (-1, -1), // Northwest
    (0, 1),   // Southeast
    (-1, 1),  // Southwest
];

/// Neighbor offsets for odd rows (row % 2 == 1) in odd-r offset layout.
const ODD_ROW_NEIGHBORS: [(i32, i32); 6] = [
    (1, 0),  // East
    (-1, 0), // West
    (1, -1), // Northeast
    (0, -1), // Northwest
    (1, 1),  // Southeast
    (0, 1),  // Southwest
];

/// Compute grid dimensions for approximately `target_count` tiles.
///
/// Height is always even for correct toroidal wrapping.
/// Returns (width, height) where width * height >= target_count.
pub fn grid_dimensions(target_count: u32) -> (u32, u32) {
    let side = ((target_count as f64).sqrt().ceil() as u32).max(2);
    let height = if side % 2 == 0 { side } else { side + 1 };
    let width = side.max(2);
    (width, height)
}

/// Generate a flat hex grid with toroidal (wrapping) topology.
///
/// Creates `width * height` tiles, each with exactly 6 neighbors.
/// Uses odd-r offset coordinates for neighbor computation.
/// All neighbor relationships are bidirectional.
///
/// # Panics
/// Panics if width < 2, height < 2, or height is odd.
pub fn generate_flat_hex_grid(width: u32, height: u32) -> Vec<Tile> {
    assert!(width >= 2, "Grid width must be at least 2");
    assert!(height >= 2, "Grid height must be at least 2");
    assert!(
        height % 2 == 0,
        "Grid height must be even for toroidal wrapping"
    );

    let total = (width * height) as usize;
    let mut tiles = Vec::with_capacity(total);

    for row in 0..height {
        for col in 0..width {
            let id = row * width + col;
            let position = offset_to_pixel(col, row);
            tiles.push(Tile::new_default(id, Vec::with_capacity(6), position));
        }
    }

    for row in 0..height {
        for col in 0..width {
            let id = (row * width + col) as usize;
            let offsets = if row % 2 == 0 {
                &EVEN_ROW_NEIGHBORS
            } else {
                &ODD_ROW_NEIGHBORS
            };

            let mut neighbors = Vec::with_capacity(6);
            for &(dc, dr) in offsets {
                let nc = (col as i32 + dc).rem_euclid(width as i32) as u32;
                let nr = (row as i32 + dr).rem_euclid(height as i32) as u32;
                neighbors.push(nr * width + nc);
            }
            tiles[id].neighbors = neighbors;
        }
    }

    tiles
}

/// Convert offset coordinates to pixel position (pointy-top hex layout).
fn offset_to_pixel(col: u32, row: u32) -> Position {
    let size = 1.0_f64;
    let x = size * 3.0_f64.sqrt() * (col as f64 + 0.5 * (row % 2) as f64);
    let y = size * 1.5 * row as f64;
    Position::flat(x, y)
}

/// Calculate the exact tile count for a geodesic grid at a given subdivision level.
///
/// Formula: `10 * 4^level + 2`. Level 4 → 2,562 tiles.
pub fn geodesic_tile_count(level: u32) -> u32 {
    10 * 4u32.pow(level) + 2
}

/// Generate a geodesic grid by subdividing an icosahedron.
///
/// Produces a hex grid on a unit sphere with exactly 12 pentagons (5 neighbors)
/// and all other tiles as hexagons (6 neighbors).
///
/// # Panics
/// Panics if `level` is not in 1..=7.
pub fn generate_geodesic_grid(level: u32) -> Vec<Tile> {
    assert!(
        (1..=7).contains(&level),
        "Geodesic subdivision level must be 1-7, got {}",
        level
    );

    // hexasphere uses linear edge subdivision: n subdivisions → (n+1)^2 vertices per face.
    // To match our formula (10 * 4^level + 2), we need subdivisions = 2^level - 1.
    let hexasphere_subdivisions = (1usize << level) - 1;
    let sphere = Subdivided::<(), IcoSphereBase>::new(hexasphere_subdivisions, |_| ());
    let points = sphere.raw_points();
    let indices = sphere.get_all_indices();
    let vertex_count = points.len();

    // Build neighbor adjacency from shared triangle edges
    let mut neighbor_sets: Vec<HashSet<u32>> = vec![HashSet::new(); vertex_count];
    for chunk in indices.chunks(3) {
        let a = chunk[0];
        let b = chunk[1];
        let c = chunk[2];
        neighbor_sets[a as usize].insert(b);
        neighbor_sets[a as usize].insert(c);
        neighbor_sets[b as usize].insert(a);
        neighbor_sets[b as usize].insert(c);
        neighbor_sets[c as usize].insert(a);
        neighbor_sets[c as usize].insert(b);
    }

    let mut tiles = Vec::with_capacity(vertex_count);
    for (i, point) in points.iter().enumerate() {
        let x = point.x as f64;
        let y = point.y as f64;
        let z = point.z as f64;
        let lat = z.asin().to_degrees();
        let lon = y.atan2(x).to_degrees();

        let mut neighbor_vec: Vec<u32> = neighbor_sets[i].iter().copied().collect();
        neighbor_vec.sort_unstable(); // deterministic ordering

        let position = Position {
            x,
            y,
            z,
            lat,
            lon,
        };
        tiles.push(Tile::new_default(i as u32, neighbor_vec, position));
    }

    tiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn flat_hex_correct_tile_count() {
        let tiles = generate_flat_hex_grid(32, 32);
        assert_eq!(tiles.len(), 1024);
    }

    #[test]
    fn all_tiles_have_six_neighbors() {
        let tiles = generate_flat_hex_grid(10, 10);
        for tile in &tiles {
            assert_eq!(
                tile.neighbors.len(),
                6,
                "Tile {} has {} neighbors",
                tile.id,
                tile.neighbors.len()
            );
        }
    }

    #[test]
    fn neighbors_are_bidirectional() {
        let tiles = generate_flat_hex_grid(10, 10);
        for tile in &tiles {
            for &neighbor_id in &tile.neighbors {
                let neighbor = &tiles[neighbor_id as usize];
                assert!(
                    neighbor.neighbors.contains(&tile.id),
                    "Tile {} has neighbor {}, but {} does not have {} as neighbor",
                    tile.id,
                    neighbor_id,
                    neighbor_id,
                    tile.id
                );
            }
        }
    }

    #[test]
    fn no_self_neighbors() {
        let tiles = generate_flat_hex_grid(10, 10);
        for tile in &tiles {
            assert!(
                !tile.neighbors.contains(&tile.id),
                "Tile {} is its own neighbor",
                tile.id
            );
        }
    }

    #[test]
    fn no_duplicate_neighbors() {
        let tiles = generate_flat_hex_grid(10, 10);
        for tile in &tiles {
            let unique: HashSet<u32> = tile.neighbors.iter().copied().collect();
            assert_eq!(
                unique.len(),
                tile.neighbors.len(),
                "Tile {} has duplicate neighbors: {:?}",
                tile.id,
                tile.neighbors
            );
        }
    }

    #[test]
    fn all_tiles_reachable() {
        let tiles = generate_flat_hex_grid(10, 10);
        let total = tiles.len();
        let mut visited = vec![false; total];
        let mut queue = VecDeque::new();
        queue.push_back(0u32);
        visited[0] = true;
        let mut count = 1;

        while let Some(id) = queue.pop_front() {
            for &neighbor_id in &tiles[id as usize].neighbors {
                if !visited[neighbor_id as usize] {
                    visited[neighbor_id as usize] = true;
                    count += 1;
                    queue.push_back(neighbor_id);
                }
            }
        }

        assert_eq!(
            count, total,
            "Only {} of {} tiles reachable from tile 0",
            count, total
        );
    }

    #[test]
    fn small_grid_neighbor_verification() {
        let tiles = generate_flat_hex_grid(4, 4);
        assert_eq!(tiles.len(), 16);

        // Tile 0 (col=0, row=0, even row)
        let t0 = &tiles[0];
        assert!(t0.neighbors.contains(&1), "Tile 0 should neighbor 1 (East)");
        assert!(
            t0.neighbors.contains(&3),
            "Tile 0 should neighbor 3 (West, wraps)"
        );
        assert!(t0.neighbors.contains(&4), "Tile 0 should neighbor 4 (SE)");
        assert!(
            t0.neighbors.contains(&12),
            "Tile 0 should neighbor 12 (NE, wraps)"
        );

        // Tile 5 (col=1, row=1, odd row)
        let t5 = &tiles[5];
        assert!(t5.neighbors.contains(&6), "Tile 5 should neighbor 6 (East)");
        assert!(t5.neighbors.contains(&4), "Tile 5 should neighbor 4 (West)");
        assert!(t5.neighbors.contains(&2), "Tile 5 should neighbor 2 (NE)");
        assert!(t5.neighbors.contains(&1), "Tile 5 should neighbor 1 (NW)");
        assert!(
            t5.neighbors.contains(&10),
            "Tile 5 should neighbor 10 (SE)"
        );
        assert!(t5.neighbors.contains(&9), "Tile 5 should neighbor 9 (SW)");
    }

    #[test]
    fn grid_dimensions_returns_valid_sizes() {
        let (w, h) = grid_dimensions(1000);
        assert!(w * h >= 1000);
        assert!(h % 2 == 0);

        let (w, h) = grid_dimensions(100);
        assert!(w * h >= 100);
        assert!(h % 2 == 0);

        let (w, h) = grid_dimensions(10000);
        assert!(w * h >= 10000);
        assert!(h % 2 == 0);
    }

    #[test]
    fn topology_is_deterministic() {
        let tiles1 = generate_flat_hex_grid(10, 10);
        let tiles2 = generate_flat_hex_grid(10, 10);
        assert_eq!(tiles1.len(), tiles2.len());
        for (t1, t2) in tiles1.iter().zip(tiles2.iter()) {
            assert_eq!(t1.id, t2.id);
            assert_eq!(t1.neighbors, t2.neighbors);
            assert_eq!(t1.position, t2.position);
        }
    }

    // === Geodesic grid tests ===

    #[test]
    fn geodesic_tile_count_formula() {
        assert_eq!(geodesic_tile_count(1), 42);
        assert_eq!(geodesic_tile_count(2), 162);
        assert_eq!(geodesic_tile_count(3), 642);
        assert_eq!(geodesic_tile_count(4), 2562);
        assert_eq!(geodesic_tile_count(5), 10242);
        assert_eq!(geodesic_tile_count(6), 40962);
        assert_eq!(geodesic_tile_count(7), 163842);
    }

    #[test]
    fn geodesic_correct_tile_counts() {
        for level in 1..=5 {
            let tiles = generate_geodesic_grid(level);
            let expected = geodesic_tile_count(level) as usize;
            assert_eq!(
                tiles.len(),
                expected,
                "Level {} should have {} tiles, got {}",
                level,
                expected,
                tiles.len()
            );
        }
    }

    #[test]
    fn geodesic_exactly_12_pentagons() {
        let tiles = generate_geodesic_grid(4);
        let pentagons: Vec<_> = tiles.iter().filter(|t| t.neighbors.len() == 5).collect();
        let hexagons: Vec<_> = tiles.iter().filter(|t| t.neighbors.len() == 6).collect();
        let other: Vec<_> = tiles
            .iter()
            .filter(|t| t.neighbors.len() != 5 && t.neighbors.len() != 6)
            .collect();

        assert_eq!(
            pentagons.len(),
            12,
            "Expected exactly 12 pentagons, got {}",
            pentagons.len()
        );
        assert_eq!(hexagons.len(), tiles.len() - 12);
        assert!(
            other.is_empty(),
            "Found {} tiles with neither 5 nor 6 neighbors",
            other.len()
        );
    }

    #[test]
    fn geodesic_neighbors_bidirectional() {
        let tiles = generate_geodesic_grid(3);
        for tile in &tiles {
            for &neighbor_id in &tile.neighbors {
                let neighbor = &tiles[neighbor_id as usize];
                assert!(
                    neighbor.neighbors.contains(&tile.id),
                    "Tile {} has neighbor {}, but {} does not have {} as neighbor",
                    tile.id,
                    neighbor_id,
                    neighbor_id,
                    tile.id
                );
            }
        }
    }

    #[test]
    fn geodesic_no_self_neighbors() {
        let tiles = generate_geodesic_grid(3);
        for tile in &tiles {
            assert!(
                !tile.neighbors.contains(&tile.id),
                "Tile {} is its own neighbor",
                tile.id
            );
        }
    }

    #[test]
    fn geodesic_no_duplicate_neighbors() {
        let tiles = generate_geodesic_grid(3);
        for tile in &tiles {
            let unique: HashSet<u32> = tile.neighbors.iter().copied().collect();
            assert_eq!(
                unique.len(),
                tile.neighbors.len(),
                "Tile {} has duplicate neighbors",
                tile.id
            );
        }
    }

    #[test]
    fn geodesic_all_tiles_reachable() {
        let tiles = generate_geodesic_grid(3);
        let total = tiles.len();
        let mut visited = vec![false; total];
        let mut queue = VecDeque::new();
        queue.push_back(0u32);
        visited[0] = true;
        let mut count = 1;

        while let Some(id) = queue.pop_front() {
            for &neighbor_id in &tiles[id as usize].neighbors {
                if !visited[neighbor_id as usize] {
                    visited[neighbor_id as usize] = true;
                    count += 1;
                    queue.push_back(neighbor_id);
                }
            }
        }

        assert_eq!(
            count, total,
            "Only {} of {} geodesic tiles reachable from tile 0",
            count, total
        );
    }

    #[test]
    fn geodesic_positions_on_unit_sphere() {
        let tiles = generate_geodesic_grid(3);
        for tile in &tiles {
            let p = &tile.position;
            let r_sq = p.x * p.x + p.y * p.y + p.z * p.z;
            assert!(
                (r_sq - 1.0).abs() < 1e-6,
                "Tile {} not on unit sphere: x²+y²+z² = {}",
                tile.id,
                r_sq
            );
        }
    }

    #[test]
    fn geodesic_lat_lon_ranges() {
        let tiles = generate_geodesic_grid(3);
        for tile in &tiles {
            assert!(
                tile.position.lat >= -90.0 && tile.position.lat <= 90.0,
                "Tile {} lat out of range: {}",
                tile.id,
                tile.position.lat
            );
            assert!(
                tile.position.lon >= -180.0 && tile.position.lon <= 180.0,
                "Tile {} lon out of range: {}",
                tile.id,
                tile.position.lon
            );
        }
    }

    #[test]
    fn geodesic_is_deterministic() {
        let tiles1 = generate_geodesic_grid(3);
        let tiles2 = generate_geodesic_grid(3);
        assert_eq!(tiles1.len(), tiles2.len());
        for (t1, t2) in tiles1.iter().zip(tiles2.iter()) {
            assert_eq!(t1.id, t2.id);
            assert_eq!(t1.neighbors, t2.neighbors);
            assert_eq!(t1.position, t2.position);
        }
    }
}
