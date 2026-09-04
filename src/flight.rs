//! Flight-domain data and motion.

use micromath::F32Ext;

pub(crate) const METRES_PER_DEGREE_LATITUDE: f32 = 110_540.0;
pub(crate) const METRES_PER_DEGREE_LONGITUDE: f32 = 111_320.0;
pub const MAX_CITIES: usize = 64;
pub const MAX_AIRCRAFT: usize = 32;

/// A single aircraft observation from a flight API.
#[derive(Clone, Debug, Default)]
pub struct Aircraft {
    /// Stable API/ICAO identifier.
    pub id: u32,
    /// Latitude in degrees.
    pub latitude: f32,
    /// Longitude in degrees.
    pub longitude: f32,
    /// Altitude in metres.
    pub altitude_m: f32,
    /// Track direction: 0° is north, increasing clockwise.
    pub heading_deg: f32,
    /// Ground speed in metres per second.
    pub speed_mps: f32,
    pub name: heapless::String<8>,
    pub size: u8,
    pub category: u8,
}

/// A named settlement fetched for the radar map.
#[derive(Clone, Debug, Default)]
pub struct City {
    pub latitude: f32,
    pub longitude: f32,
    pub population: u32,
    pub name: heapless::String<24>,
}

impl Aircraft {
    /// Advance the observation using its heading and ground speed.
    pub fn advance(&mut self, seconds: f32) {
        let heading = self.heading_deg.to_radians();
        let distance = self.speed_mps * seconds;
        self.latitude += heading.cos() * distance / METRES_PER_DEGREE_LATITUDE;
        let latitude_scale = self.latitude.to_radians().cos();
        if latitude_scale.abs() > 0.001 {
            self.longitude +=
                heading.sin() * distance / (METRES_PER_DEGREE_LONGITUDE * latitude_scale);
        }
    }
}
