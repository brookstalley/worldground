use serde::{Deserialize, Serialize};
use std::path::Path;

/// Topology configuration: flat hex grid or geodesic sphere.
///
/// Uses a flat struct (not a tagged enum) for bincode + TOML compatibility.
/// `mode` is "flat" (default) or "geodesic".
/// `subdivision_level` is only used in geodesic mode (1-7, default 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopologyConfig {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_subdivision_level")]
    pub subdivision_level: u32,
}

fn default_mode() -> String {
    "flat".to_string()
}

fn default_subdivision_level() -> u32 {
    4
}

impl TopologyConfig {
    pub fn is_geodesic(&self) -> bool {
        self.mode == "geodesic"
    }
}

impl Default for TopologyConfig {
    fn default() -> Self {
        TopologyConfig {
            mode: "flat".to_string(),
            subdivision_level: 4,
        }
    }
}

/// Parameters used to procedurally generate a world.
/// Stored with the world for reproducibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationParams {
    pub seed: u64,
    pub tile_count: u32,
    pub ocean_ratio: f32,
    pub mountain_ratio: f32,
    pub elevation_roughness: f32,
    pub climate_bands: bool,
    pub resource_density: f32,
    pub initial_biome_maturity: f32,
    #[serde(default)]
    pub topology: TopologyConfig,
}

impl GenerationParams {
    /// Load generation parameters from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
        let params: Self =
            toml::from_str(&content).map_err(|e| format!("Invalid TOML in {}: {}", path.display(), e))?;
        params.validate()?;
        Ok(params)
    }

    /// Validate parameter ranges.
    pub fn validate(&self) -> Result<(), String> {
        if self.tile_count < 100 {
            return Err(format!(
                "tile_count must be >= 100, got {}",
                self.tile_count
            ));
        }
        if !(0.0..=1.0).contains(&self.ocean_ratio) {
            return Err(format!(
                "ocean_ratio must be 0.0-1.0, got {}",
                self.ocean_ratio
            ));
        }
        if !(0.0..=0.5).contains(&self.mountain_ratio) {
            return Err(format!(
                "mountain_ratio must be 0.0-0.5, got {}",
                self.mountain_ratio
            ));
        }
        if !(0.0..=1.0).contains(&self.elevation_roughness) {
            return Err(format!(
                "elevation_roughness must be 0.0-1.0, got {}",
                self.elevation_roughness
            ));
        }
        if !(0.0..=1.0).contains(&self.resource_density) {
            return Err(format!(
                "resource_density must be 0.0-1.0, got {}",
                self.resource_density
            ));
        }
        if !(0.0..=1.0).contains(&self.initial_biome_maturity) {
            return Err(format!(
                "initial_biome_maturity must be 0.0-1.0, got {}",
                self.initial_biome_maturity
            ));
        }
        if self.topology.mode != "flat" && self.topology.mode != "geodesic" {
            return Err(format!(
                "topology.mode must be 'flat' or 'geodesic', got '{}'",
                self.topology.mode
            ));
        }
        if self.topology.is_geodesic() && !(1..=7).contains(&self.topology.subdivision_level) {
            return Err(format!(
                "subdivision_level must be 1-7, got {}",
                self.topology.subdivision_level
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn valid_params() {
        let params = GenerationParams {
            seed: 42,
            tile_count: 1000,
            ocean_ratio: 0.6,
            mountain_ratio: 0.1,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
            topology: TopologyConfig::default(),
        };
        assert!(params.validate().is_ok());
    }

    #[test]
    fn invalid_tile_count() {
        let params = GenerationParams {
            seed: 42,
            tile_count: 50,
            ocean_ratio: 0.6,
            mountain_ratio: 0.1,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
            topology: TopologyConfig::default(),
        };
        let err = params.validate().unwrap_err();
        assert!(
            err.contains("tile_count"),
            "Error should mention tile_count: {}",
            err
        );
    }

    #[test]
    fn invalid_ocean_ratio() {
        let params = GenerationParams {
            seed: 42,
            tile_count: 1000,
            ocean_ratio: 1.5,
            mountain_ratio: 0.1,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
            topology: TopologyConfig::default(),
        };
        let err = params.validate().unwrap_err();
        assert!(
            err.contains("ocean_ratio"),
            "Error should mention ocean_ratio: {}",
            err
        );
    }

    #[test]
    fn invalid_mountain_ratio() {
        let params = GenerationParams {
            seed: 42,
            tile_count: 1000,
            ocean_ratio: 0.6,
            mountain_ratio: 0.7,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
            topology: TopologyConfig::default(),
        };
        let err = params.validate().unwrap_err();
        assert!(
            err.contains("mountain_ratio"),
            "Error should mention mountain_ratio: {}",
            err
        );
    }

    #[test]
    fn from_toml_string() {
        let toml_str = r#"
seed = 42
tile_count = 1000
ocean_ratio = 0.6
mountain_ratio = 0.1
elevation_roughness = 0.5
climate_bands = true
resource_density = 0.3
initial_biome_maturity = 0.5
"#;
        let params: GenerationParams = toml::from_str(toml_str).unwrap();
        assert_eq!(params.seed, 42);
        assert_eq!(params.tile_count, 1000);
        params.validate().unwrap();
    }

    #[test]
    fn from_file_valid() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
seed = 0
tile_count = 500
ocean_ratio = 0.4
mountain_ratio = 0.2
elevation_roughness = 0.7
climate_bands = false
resource_density = 0.5
initial_biome_maturity = 0.3
"#
        )
        .unwrap();

        let params = GenerationParams::from_file(tmpfile.path()).unwrap();
        assert_eq!(params.tile_count, 500);
        assert_eq!(params.ocean_ratio, 0.4);
        assert!(!params.climate_bands);
    }

    #[test]
    fn from_file_missing() {
        let err = GenerationParams::from_file(Path::new("/nonexistent/file.toml")).unwrap_err();
        assert!(err.contains("Cannot read"), "Error: {}", err);
    }

    #[test]
    fn from_file_invalid_toml() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(tmpfile, "this is not valid toml {{{{").unwrap();

        let err = GenerationParams::from_file(tmpfile.path()).unwrap_err();
        assert!(err.contains("Invalid TOML"), "Error: {}", err);
    }

    #[test]
    fn from_file_out_of_range() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
seed = 1
tile_count = 10
ocean_ratio = 0.5
mountain_ratio = 0.1
elevation_roughness = 0.5
climate_bands = true
resource_density = 0.3
initial_biome_maturity = 0.5
"#
        )
        .unwrap();

        let err = GenerationParams::from_file(tmpfile.path()).unwrap_err();
        assert!(err.contains("tile_count"), "Error: {}", err);
    }
}
