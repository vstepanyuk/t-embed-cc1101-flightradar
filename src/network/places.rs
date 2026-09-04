//! Overpass place response decoding and city selection.

use alloc::vec::Vec;

use json_bourne::{FromJson, parse_str};
use log::warn;

use crate::{
    flight::{City, MAX_CITIES},
    helpers::truncate_string,
};

#[derive(FromJson)]
#[bourne(deny_unknown_fields = false)]
struct OverpassResponse<'input> {
    elements: Option<Vec<OverpassElement<'input>>>,
}

#[derive(FromJson)]
#[bourne(deny_unknown_fields = false)]
struct OverpassElement<'input> {
    lat: Option<f32>,
    lon: Option<f32>,
    center: Option<OverpassCenter>,
    tags: Option<OverpassTags<'input>>,
}

#[derive(FromJson)]
#[bourne(deny_unknown_fields = false)]
struct OverpassCenter {
    lat: f32,
    lon: f32,
}

#[derive(FromJson)]
#[bourne(deny_unknown_fields = false)]
struct OverpassTags<'input> {
    name: Option<&'input str>,
    population: Option<&'input str>,
}

pub fn decode_cities(body: &str) -> Result<[Option<City>; MAX_CITIES], ()> {
    let response = parse_str::<OverpassResponse<'_>>(body).map_err(|error| {
        warn!("City JSON parse failed: {:?}", error);
    })?;
    let mut cities = [const { None }; MAX_CITIES];
    for city in response.elements.into_iter().flatten().filter_map(|result| {
        let tags = result.tags?;
        let name = tags.name?;
        let (latitude, longitude) = result
            .lat
            .zip(result.lon)
            .or_else(|| result.center.map(|center| (center.lat, center.lon)))?;
        Some(City {
            latitude,
            longitude,
            population: tags.population.and_then(|value| value.parse().ok()).unwrap_or(0),
            name: truncate_string::<24>(name),
        })
    }) {
        if let Some(slot) = cities.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(city);
        } else if let Some((index, least)) = cities
            .iter()
            .enumerate()
            .filter_map(|(index, city)| city.as_ref().map(|city| (index, city)))
            .min_by_key(|(_, city)| city.population)
            && city.population > least.population
        {
            cities[index] = Some(city);
        }
    }
    Ok(cities)
}
