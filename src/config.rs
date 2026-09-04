//! User-facing radar and network configuration.

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use micromath::F32Ext;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct RadarConfig {
    pub center_latitude: f32,
    pub center_longitude: f32,
    pub radius_km: f32,
    pub timezone_offset_minutes: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadarConfigError {
    InvalidLatitude,
    InvalidLongitude,
    InvalidRadius,
    InvalidTimezone,
}

impl RadarConfig {
    pub const DEFAULT: Self = Self {
        center_latitude: 49.3264,
        center_longitude: 12.1097,
        radius_km: 50.0,
        timezone_offset_minutes: 60,
    };

    pub const fn radius_m(self) -> f32 {
        self.radius_km * 1_000.0
    }

    pub fn try_new(
        latitude: f32,
        longitude: f32,
        radius_km: f32,
        timezone_offset_minutes: i32,
    ) -> Result<Self, RadarConfigError> {
        if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
            return Err(RadarConfigError::InvalidLatitude);
        }
        if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
            return Err(RadarConfigError::InvalidLongitude);
        }
        if !radius_km.is_finite() || !(1.0..=250.0).contains(&radius_km) {
            return Err(RadarConfigError::InvalidRadius);
        }
        if !(-720..=840).contains(&timezone_offset_minutes) {
            return Err(RadarConfigError::InvalidTimezone);
        }
        Ok(Self {
            center_latitude: latitude,
            center_longitude: longitude,
            radius_km,
            timezone_offset_minutes,
        })
    }

    /// Geographic bounding box for this radar: south, north, west, east.
    pub fn bounds(self) -> (f32, f32, f32, f32) {
        let latitude_delta = self.radius_km / 110.54;
        let longitude_delta = self.radius_km / (111.32 * self.center_latitude.to_radians().cos());
        (
            self.center_latitude - latitude_delta,
            self.center_latitude + latitude_delta,
            self.center_longitude - longitude_delta,
            self.center_longitude + longitude_delta,
        )
    }
}

static RADAR_CONFIG: Mutex<CriticalSectionRawMutex, RadarConfig> = Mutex::new(RadarConfig::DEFAULT);

pub async fn current() -> RadarConfig {
    *RADAR_CONFIG.lock().await
}

pub async fn update(config: RadarConfig) {
    *RADAR_CONFIG.lock().await = config;
}
