use serde::{Deserialize, Serialize};

/// Type of pressure system, determining behavior and lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PressureSystemType {
    MidLatCyclone,
    SubtropicalHigh,
    TropicalLow,
    PolarHigh,
    ThermalLow,
}

/// A pressure system — a macro-scale weather entity that moves, intensifies, and decays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PressureSystem {
    pub id: u32,
    /// Latitude in degrees
    pub lat: f64,
    /// Longitude in degrees
    pub lon: f64,
    /// Unit sphere x coordinate (cos(lat)*cos(lon))
    pub x: f64,
    /// Unit sphere y coordinate (cos(lat)*sin(lon))
    pub y: f64,
    /// Unit sphere z coordinate (sin(lat))
    pub z: f64,
    /// Pressure anomaly in hPa relative to 1013.25 (negative = low pressure)
    pub pressure_anomaly: f32,
    /// Influence radius in radians (~0.3 = ~1700km)
    pub radius: f32,
    /// Eastward velocity in rad/tick
    pub velocity_east: f32,
    /// Northward velocity in rad/tick
    pub velocity_north: f32,
    /// Age in ticks
    pub age: u32,
    /// Maximum age before forced decay
    pub max_age: u32,
    /// System classification
    pub system_type: PressureSystemType,
    /// Moisture content 0.0-1.0
    pub moisture: f32,
}

/// Global macro weather state — pressure systems and RNG state for determinism.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroWeatherState {
    pub systems: Vec<PressureSystem>,
    pub next_id: u32,
    pub rng_state: u64,
}

impl Default for MacroWeatherState {
    fn default() -> Self {
        Self {
            systems: Vec::new(),
            next_id: 1,
            rng_state: 1,
        }
    }
}

impl MacroWeatherState {
    pub fn with_seed(seed: u64) -> Self {
        Self {
            systems: Vec::new(),
            next_id: 1,
            rng_state: if seed == 0 { 1 } else { seed },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_weather_state_default() {
        let state = MacroWeatherState::default();
        assert!(state.systems.is_empty());
        assert_eq!(state.next_id, 1);
        assert_eq!(state.rng_state, 1);
    }

    #[test]
    fn macro_weather_state_with_seed() {
        let state = MacroWeatherState::with_seed(42);
        assert_eq!(state.rng_state, 42);

        // Seed 0 should become 1 (avoid xorshift zero state)
        let state = MacroWeatherState::with_seed(0);
        assert_eq!(state.rng_state, 1);
    }

    #[test]
    fn pressure_system_serde_round_trip() {
        let system = PressureSystem {
            id: 1,
            lat: 45.0,
            lon: -90.0,
            x: 0.5,
            y: -0.5,
            z: 0.707,
            pressure_anomaly: -15.0,
            radius: 0.3,
            velocity_east: 0.01,
            velocity_north: -0.005,
            age: 10,
            max_age: 200,
            system_type: PressureSystemType::MidLatCyclone,
            moisture: 0.7,
        };

        let encoded = bincode::serialize(&system).expect("serialize");
        let decoded: PressureSystem = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(system, decoded);
    }

    #[test]
    fn macro_weather_state_serde_round_trip() {
        let state = MacroWeatherState {
            systems: vec![
                PressureSystem {
                    id: 1,
                    lat: 30.0,
                    lon: 60.0,
                    x: 0.75,
                    y: 0.433,
                    z: 0.5,
                    pressure_anomaly: 10.0,
                    radius: 0.4,
                    velocity_east: 0.0,
                    velocity_north: 0.0,
                    age: 0,
                    max_age: 500,
                    system_type: PressureSystemType::SubtropicalHigh,
                    moisture: 0.3,
                },
            ],
            next_id: 2,
            rng_state: 12345,
        };

        let encoded = bincode::serialize(&state).expect("serialize");
        let decoded: MacroWeatherState = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(state, decoded);
    }

    #[test]
    fn all_system_types_serialize() {
        let types = [
            PressureSystemType::MidLatCyclone,
            PressureSystemType::SubtropicalHigh,
            PressureSystemType::TropicalLow,
            PressureSystemType::PolarHigh,
            PressureSystemType::ThermalLow,
        ];
        for t in &types {
            let encoded = bincode::serialize(t).expect("serialize");
            let decoded: PressureSystemType = bincode::deserialize(&encoded).expect("deserialize");
            assert_eq!(*t, decoded);
        }
    }
}
