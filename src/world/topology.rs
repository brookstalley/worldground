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
    Position { x, y }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
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
}
