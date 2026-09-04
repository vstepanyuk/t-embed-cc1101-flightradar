//! Geographic-to-screen projection for the radar display.

use bevy_ecs::prelude::Resource;
use embedded_graphics::geometry::Point;
use micromath::F32Ext;

use crate::flight::{Aircraft, METRES_PER_DEGREE_LATITUDE, METRES_PER_DEGREE_LONGITUDE};

/// A projected aircraft ready for radar rendering.
#[derive(Clone, Debug, Default)]
pub struct RadarAircraft {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub direction_x: f32,
    pub direction_y: f32,
    pub altitude_m: f32,
    pub speed_mps: f32,
    pub size: u8,
    pub category: u8,
}

/// Screen-space vertices for an aircraft marker.
#[derive(Clone, Copy, Debug)]
pub struct AircraftMarker {
    pub tip: Point,
    pub left: Point,
    pub right: Point,
}

/// Calculate a direction-aware triangular marker for a projected aircraft.
pub fn aircraft_marker(target: &RadarAircraft) -> AircraftMarker {
    let point = Point::new(target.x as i32, target.y as i32);
    let length = 6 + target.size as i32;
    let width = 3 + target.size as i32 / 2;
    let tip = Point::new(
        point.x + (target.direction_x * length as f32) as i32,
        point.y + (target.direction_y * length as f32) as i32,
    );
    let side = Point::new(
        point.x - (target.direction_x * (length as f32 * 0.55)) as i32,
        point.y - (target.direction_y * (length as f32 * 0.55)) as i32,
    );
    let left = Point::new(
        side.x - (target.direction_y * width as f32) as i32,
        side.y + (target.direction_x * width as f32) as i32,
    );
    let right = Point::new(
        side.x + (target.direction_y * width as f32) as i32,
        side.y - (target.direction_x * width as f32) as i32,
    );

    AircraftMarker { tip, left, right }
}

/// Radar coordinate system centred on a geographic location.
#[derive(Resource, Clone, Copy, Debug)]
pub struct RadarProjection {
    pub latitude: f32,
    pub longitude: f32,
    pub range_m: f32,
    pub center_x: f32,
    pub center_y: f32,
}

impl RadarProjection {
    pub const fn new(
        latitude: f32,
        longitude: f32,
        range_m: f32,
        center_x: f32,
        center_y: f32,
    ) -> Self {
        Self { latitude, longitude, range_m, center_x, center_y }
    }

    pub fn project(&self, aircraft: &Aircraft) -> Option<RadarAircraft> {
        let (x, y) = self.project_location(aircraft.latitude, aircraft.longitude)?;
        let heading = aircraft.heading_deg.to_radians();
        Some(RadarAircraft {
            id: aircraft.id,
            x,
            y,
            direction_x: heading.sin(),
            direction_y: -heading.cos(),
            altitude_m: aircraft.altitude_m,
            speed_mps: aircraft.speed_mps,
            size: aircraft.size,
            category: aircraft.category,
        })
    }

    pub fn project_location(&self, latitude: f32, longitude: f32) -> Option<(f32, f32)> {
        if self.range_m <= 0.0 {
            return None;
        }
        let latitude_delta = (latitude - self.latitude) * METRES_PER_DEGREE_LATITUDE;
        let longitude_scale = self.latitude.to_radians().cos();
        let longitude_delta =
            (longitude - self.longitude) * METRES_PER_DEGREE_LONGITUDE * longitude_scale;
        let distance = (latitude_delta * latitude_delta + longitude_delta * longitude_delta).sqrt();
        if distance > self.range_m {
            return None;
        }
        Some((
            self.center_x + longitude_delta / self.range_m * self.center_x,
            self.center_y - latitude_delta / self.range_m * self.center_y,
        ))
    }
}
