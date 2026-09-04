//! Typed decoders for external aviation data.

use alloc::vec::Vec;

use json_bourne::{FromJson, parse_str};
use log::warn;

use crate::{
    flight::{Aircraft, MAX_AIRCRAFT},
    helpers::truncate_string,
};

/// Converts altitude from feet, as provided by ADS-B, to metres.
const FEET_TO_METRES: f32 = 0.3048;

/// Converts ground speed from knots, as provided by ADS-B, to metres per second.
const KNOTS_TO_METRES_PER_SECOND: f32 = 0.514444;

#[derive(FromJson)]
#[bourne(deny_unknown_fields = false)]
struct AdsbResponse<'input> {
    ac: Option<Vec<AdsbAircraft<'input>>>,
}

#[derive(FromJson)]
#[bourne(deny_unknown_fields = false)]
struct AdsbAircraft<'input> {
    hex: &'input str,
    flight: Option<&'input str>,
    lat: Option<f32>,
    lon: Option<f32>,
    alt_geom: Option<f32>,
    gs: Option<f32>,
    track: Option<f32>,
    seen: Option<f32>,
    category: Option<&'input str>,
}

pub fn decode_aircraft(body: &str) -> [Option<Aircraft>; MAX_AIRCRAFT] {
    let mut batch = [const { None }; MAX_AIRCRAFT];
    let response = match parse_str::<AdsbResponse<'_>>(body) {
        Ok(parsed) => parsed,
        Err(error) => {
            warn!("ADSB.fi JSON parse failed: {:?}", error);
            return batch;
        }
    };
    for state in response.ac.into_iter().flatten().take(MAX_AIRCRAFT) {
        let Some(latitude) = state.lat else { continue };
        let Some(longitude) = state.lon else { continue };
        let name = state.flight.map(truncate_string::<8>).unwrap_or_default();
        let category = state
            .category
            .and_then(|value| value.as_bytes().last().copied())
            .and_then(|value| value.checked_sub(b'0'))
            .unwrap_or(0);
        let size = match category {
            2 => 2,
            3 => 3,
            4.. => 5,
            _ => 3,
        };
        let mut aircraft = Aircraft {
            id: hex_id(state.hex.as_bytes()),
            latitude,
            longitude,
            altitude_m: state.alt_geom.unwrap_or(0.0) * FEET_TO_METRES,
            heading_deg: state.track.unwrap_or(0.0),
            speed_mps: state.gs.unwrap_or(0.0) * KNOTS_TO_METRES_PER_SECOND,
            name,
            size,
            category,
        };
        if let Some(seen) = state.seen {
            aircraft.advance(seen);
        }
        if let Some(slot) = batch.iter_mut().find(|item| item.is_none()) {
            *slot = Some(aircraft);
        }
    }
    batch
}

fn hex_id(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .filter_map(|&byte| match byte {
            b'0'..=b'9' => Some((byte - b'0') as u32),
            b'a'..=b'f' => Some((byte - b'a' + 10) as u32),
            b'A'..=b'F' => Some((byte - b'A' + 10) as u32),
            _ => None,
        })
        .fold(0u32, |acc, digit| (acc << 4) | digit)
}
