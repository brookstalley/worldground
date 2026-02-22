/// Correct sphere-surface spatial math for weather simulation.
///
/// Replaces the broken 2D planar helpers (wind_align, direction_to) that use
/// position.x/y on geodesic tiles where those are 3D sphere coordinates.
///
/// Coordinate convention matches server: x=cos(lat)*cos(lon), y=cos(lat)*sin(lon), z=sin(lat).

/// Great-circle angular distance between two points on a unit sphere (in radians).
/// Points given as (lat, lon) in degrees.
pub fn angular_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1 = lat1.to_radians();
    let lon1 = lon1.to_radians();
    let lat2 = lat2.to_radians();
    let lon2 = lon2.to_radians();

    // Use unit-sphere dot product for numerical stability
    let x1 = lat1.cos() * lon1.cos();
    let y1 = lat1.cos() * lon1.sin();
    let z1 = lat1.sin();

    let x2 = lat2.cos() * lon2.cos();
    let y2 = lat2.cos() * lon2.sin();
    let z2 = lat2.sin();

    let dot = (x1 * x2 + y1 * y2 + z1 * z2).clamp(-1.0, 1.0);
    dot.acos()
}

/// Direction from one point to another on the sphere surface, returned as
/// (east, north) components in a local tangent plane.
/// Points given as (lat, lon) in degrees.
/// Returns (0, 0) for coincident or antipodal points.
pub fn direction_on_sphere(from_lat: f64, from_lon: f64, to_lat: f64, to_lon: f64) -> (f64, f64) {
    let lat1 = from_lat.to_radians();
    let lon1 = from_lon.to_radians();
    let lat2 = to_lat.to_radians();
    let lon2 = to_lon.to_radians();

    // Unit vectors
    let x1 = lat1.cos() * lon1.cos();
    let y1 = lat1.cos() * lon1.sin();
    let z1 = lat1.sin();

    let x2 = lat2.cos() * lon2.cos();
    let y2 = lat2.cos() * lon2.sin();
    let z2 = lat2.sin();

    // Local tangent basis at from_point
    // East = d(position)/d(lon), normalized
    let east_x = -lon1.sin();
    let east_y = lon1.cos();
    let east_z = 0.0;

    // North = d(position)/d(lat), normalized
    let north_x = -lat1.sin() * lon1.cos();
    let north_y = -lat1.sin() * lon1.sin();
    let north_z = lat1.cos();

    // Vector from point 1 to point 2 (in 3D)
    let dx = x2 - x1;
    let dy = y2 - y1;
    let dz = z2 - z1;

    // Project onto tangent plane
    let east_component = dx * east_x + dy * east_y + dz * east_z;
    let north_component = dx * north_x + dy * north_y + dz * north_z;

    let mag = (east_component * east_component + north_component * north_component).sqrt();
    if mag < 1e-10 {
        return (0.0, 0.0);
    }

    (east_component / mag, north_component / mag)
}

/// Rotate a tangent-plane vector (east, north) by an angle in radians.
pub fn rotate_tangent_vector(east: f64, north: f64, angle: f64) -> (f64, f64) {
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    (
        east * cos_a - north * sin_a,
        east * sin_a + north * cos_a,
    )
}

/// Convert a tangent-plane vector (east, north) to a bearing in degrees (0=N, 90=E, 180=S, 270=W).
pub fn tangent_to_bearing(east: f64, north: f64) -> f64 {
    let bearing = east.atan2(north).to_degrees();
    ((bearing % 360.0) + 360.0) % 360.0
}

/// Advance a position on the sphere by a velocity vector in the tangent plane.
/// Uses Rodrigues' rotation formula for accuracy.
/// lat, lon in degrees; vel_east, vel_north in radians/tick; dt = 1.0 for one tick.
/// Returns (new_lat, new_lon) in degrees.
pub fn advance_position(lat: f64, lon: f64, vel_east: f64, vel_north: f64, dt: f64) -> (f64, f64) {
    let lat_rad = lat.to_radians();
    let lon_rad = lon.to_radians();

    // Current position as unit vector
    let px = lat_rad.cos() * lon_rad.cos();
    let py = lat_rad.cos() * lon_rad.sin();
    let pz = lat_rad.sin();

    // Local tangent basis
    let east_x = -lon_rad.sin();
    let east_y = lon_rad.cos();
    let east_z = 0.0;

    let north_x = -lat_rad.sin() * lon_rad.cos();
    let north_y = -lat_rad.sin() * lon_rad.sin();
    let north_z = lat_rad.cos();

    // Velocity direction in 3D (tangent plane)
    let vx = vel_east * dt * east_x + vel_north * dt * north_x;
    let vy = vel_east * dt * east_y + vel_north * dt * north_y;
    let vz = vel_east * dt * east_z + vel_north * dt * north_z;

    let speed = (vx * vx + vy * vy + vz * vz).sqrt();
    if speed < 1e-12 {
        return (lat, lon);
    }

    // Rotation axis: perpendicular to position and velocity direction
    // k = p × v_hat (normalized)
    let v_hat_x = vx / speed;
    let v_hat_y = vy / speed;
    let v_hat_z = vz / speed;

    let kx = py * v_hat_z - pz * v_hat_y;
    let ky = pz * v_hat_x - px * v_hat_z;
    let kz = px * v_hat_y - py * v_hat_x;

    let k_mag = (kx * kx + ky * ky + kz * kz).sqrt();
    if k_mag < 1e-12 {
        return (lat, lon);
    }

    let kx = kx / k_mag;
    let ky = ky / k_mag;
    let kz = kz / k_mag;

    // Rodrigues' rotation: rotate p by angle=speed around axis k
    let cos_s = speed.cos();
    let sin_s = speed.sin();

    // k × p
    let cross_x = ky * pz - kz * py;
    let cross_y = kz * px - kx * pz;
    let cross_z = kx * py - ky * px;

    // k · p
    let dot = kx * px + ky * py + kz * pz;

    let new_x = px * cos_s + cross_x * sin_s + kx * dot * (1.0 - cos_s);
    let new_y = py * cos_s + cross_y * sin_s + ky * dot * (1.0 - cos_s);
    let new_z = pz * cos_s + cross_z * sin_s + kz * dot * (1.0 - cos_s);

    // Convert back to lat/lon
    let new_lat = new_z.clamp(-1.0, 1.0).asin().to_degrees();
    let new_lon = new_y.atan2(new_x).to_degrees();

    (new_lat, new_lon)
}

/// Convert lat/lon (degrees) to unit sphere coordinates (x, y, z).
pub fn lat_lon_to_xyz(lat: f64, lon: f64) -> (f64, f64, f64) {
    let lat_rad = lat.to_radians();
    let lon_rad = lon.to_radians();
    (
        lat_rad.cos() * lon_rad.cos(),
        lat_rad.cos() * lon_rad.sin(),
        lat_rad.sin(),
    )
}

/// Convert unit sphere coordinates to lat/lon (degrees).
pub fn xyz_to_lat_lon(x: f64, y: f64, z: f64) -> (f64, f64) {
    let lat = z.clamp(-1.0, 1.0).asin().to_degrees();
    let lon = y.atan2(x).to_degrees();
    (lat, lon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const EPSILON: f64 = 1e-6;

    #[test]
    fn angular_distance_pole_to_equator() {
        let dist = angular_distance(90.0, 0.0, 0.0, 0.0);
        assert!(
            (dist - PI / 2.0).abs() < EPSILON,
            "Pole to equator should be pi/2, got {}",
            dist
        );
    }

    #[test]
    fn angular_distance_same_point() {
        let dist = angular_distance(45.0, 90.0, 45.0, 90.0);
        assert!(dist.abs() < EPSILON, "Same point should be 0, got {}", dist);
    }

    #[test]
    fn angular_distance_antipodal() {
        let dist = angular_distance(0.0, 0.0, 0.0, 180.0);
        assert!(
            (dist - PI).abs() < EPSILON,
            "Antipodal points should be pi, got {}",
            dist
        );
    }

    #[test]
    fn angular_distance_equator_quarter() {
        // 0,0 to 0,90 = pi/2
        let dist = angular_distance(0.0, 0.0, 0.0, 90.0);
        assert!(
            (dist - PI / 2.0).abs() < EPSILON,
            "Quarter equator should be pi/2, got {}",
            dist
        );
    }

    #[test]
    fn direction_on_sphere_east() {
        // From equator/prime-meridian, due east is along positive lon
        let (east, north) = direction_on_sphere(0.0, 0.0, 0.0, 10.0);
        assert!(
            east > 0.9,
            "East component should be ~1.0 for due-east direction, got {}",
            east
        );
        assert!(
            north.abs() < 0.1,
            "North component should be ~0.0 for due-east, got {}",
            north
        );
    }

    #[test]
    fn direction_on_sphere_north() {
        let (east, north) = direction_on_sphere(0.0, 0.0, 10.0, 0.0);
        assert!(
            north > 0.9,
            "North component should be ~1.0 for due-north, got {}",
            north
        );
        assert!(
            east.abs() < 0.1,
            "East component should be ~0.0 for due-north, got {}",
            east
        );
    }

    #[test]
    fn direction_coincident_returns_zero() {
        let (east, north) = direction_on_sphere(45.0, 90.0, 45.0, 90.0);
        assert!(east.abs() < EPSILON);
        assert!(north.abs() < EPSILON);
    }

    #[test]
    fn tangent_to_bearing_north() {
        let bearing = tangent_to_bearing(0.0, 1.0);
        assert!(
            bearing.abs() < EPSILON || (bearing - 360.0).abs() < EPSILON,
            "Due north should be 0 degrees, got {}",
            bearing
        );
    }

    #[test]
    fn tangent_to_bearing_east() {
        let bearing = tangent_to_bearing(1.0, 0.0);
        assert!(
            (bearing - 90.0).abs() < EPSILON,
            "Due east should be 90 degrees, got {}",
            bearing
        );
    }

    #[test]
    fn tangent_to_bearing_south() {
        let bearing = tangent_to_bearing(0.0, -1.0);
        assert!(
            (bearing - 180.0).abs() < EPSILON,
            "Due south should be 180 degrees, got {}",
            bearing
        );
    }

    #[test]
    fn tangent_to_bearing_west() {
        let bearing = tangent_to_bearing(-1.0, 0.0);
        assert!(
            (bearing - 270.0).abs() < EPSILON,
            "Due west should be 270 degrees, got {}",
            bearing
        );
    }

    #[test]
    fn rotate_tangent_vector_90_degrees() {
        let (e, n) = rotate_tangent_vector(1.0, 0.0, PI / 2.0);
        assert!((e - 0.0).abs() < EPSILON, "Rotated east should be ~0, got {}", e);
        assert!((n - 1.0).abs() < EPSILON, "Rotated north should be ~1, got {}", n);
    }

    #[test]
    fn advance_position_stationary() {
        let (lat, lon) = advance_position(45.0, 90.0, 0.0, 0.0, 1.0);
        assert!((lat - 45.0).abs() < EPSILON);
        assert!((lon - 90.0).abs() < EPSILON);
    }

    #[test]
    fn advance_position_northward() {
        // Move north from equator by 1 degree (in radians)
        let step = 1.0_f64.to_radians();
        let (new_lat, new_lon) = advance_position(0.0, 0.0, 0.0, step, 1.0);
        assert!(
            (new_lat - 1.0).abs() < 0.01,
            "Should move ~1 degree north, got lat={}",
            new_lat
        );
        assert!(
            new_lon.abs() < 0.01,
            "Lon should stay ~0, got {}",
            new_lon
        );
    }

    #[test]
    fn advance_position_eastward_at_equator() {
        let step = 5.0_f64.to_radians();
        let (new_lat, new_lon) = advance_position(0.0, 0.0, step, 0.0, 1.0);
        assert!(
            new_lat.abs() < 0.1,
            "Lat should stay ~0, got {}",
            new_lat
        );
        assert!(
            (new_lon - 5.0).abs() < 0.1,
            "Should move ~5 degrees east, got lon={}",
            new_lon
        );
    }

    #[test]
    fn lat_lon_xyz_round_trip() {
        let cases = [
            (0.0, 0.0),
            (90.0, 0.0),
            (-90.0, 0.0),
            (45.0, 135.0),
            (-30.0, -60.0),
        ];
        for (lat, lon) in cases {
            let (x, y, z) = lat_lon_to_xyz(lat, lon);
            let (lat2, lon2) = xyz_to_lat_lon(x, y, z);
            assert!(
                (lat - lat2).abs() < EPSILON,
                "lat round-trip failed: {} -> {}",
                lat,
                lat2
            );
            assert!(
                (lon - lon2).abs() < EPSILON,
                "lon round-trip failed: {} -> {}",
                lon,
                lon2
            );
        }
    }

    #[test]
    fn angular_distance_symmetric() {
        let d1 = angular_distance(30.0, 45.0, 60.0, -10.0);
        let d2 = angular_distance(60.0, -10.0, 30.0, 45.0);
        assert!(
            (d1 - d2).abs() < EPSILON,
            "Distance should be symmetric: {} vs {}",
            d1,
            d2
        );
    }
}
