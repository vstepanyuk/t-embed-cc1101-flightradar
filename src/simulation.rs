//! Bevy ECS world for the radar's moving flight targets.

use bevy_ecs::prelude::*;
use micromath::F32Ext;

use crate::{
    config::RadarConfig,
    display::{LOGICAL_HEIGHT, LOGICAL_WIDTH},
    flight::{Aircraft, MAX_AIRCRAFT},
    projection::RadarProjection,
};

const SWEEP_DEGREES_PER_SECOND: f32 = 105.0;
const CONTACT_GLOW_DECAY_PER_SECOND: f32 = 1.8;

#[derive(Component, Clone, Debug)]
pub struct Flight(pub Aircraft);

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ContactGlow(pub f32);

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct DeltaSeconds(pub f32);

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SweepDegrees(pub f32);

pub fn advance_flights(mut flights: Query<&mut Flight>, delta: Res<DeltaSeconds>) {
    for mut flight in &mut flights {
        flight.0.advance(delta.0);
    }
}

pub fn advance_sweep(mut sweep: ResMut<SweepDegrees>, delta: Res<DeltaSeconds>) {
    sweep.0 = (sweep.0 + delta.0 * SWEEP_DEGREES_PER_SECOND) % 360.0;
}

pub fn update_contact_glow(
    mut flights: Query<(&Flight, &mut ContactGlow)>,
    projection: Res<RadarProjection>,
    sweep: Res<SweepDegrees>,
    delta: Res<DeltaSeconds>,
) {
    for (flight, mut glow) in &mut flights {
        let beam_strength = projection
            .project(&flight.0)
            .map_or(0.0, |target| sweep_contact_strength(&projection, sweep.0, target.x, target.y));
        glow.0 = (glow.0 - delta.0 * CONTACT_GLOW_DECAY_PER_SECOND).max(beam_strength);
    }
}

pub fn replace_with_live_data(world: &mut World, aircraft: [Option<Aircraft>; MAX_AIRCRAFT]) {
    let mut flight_entities = [None; MAX_AIRCRAFT];
    let mut query = world.query_filtered::<Entity, With<Flight>>();
    for (slot, entity) in flight_entities.iter_mut().zip(query.iter(world)) {
        *slot = Some(entity);
    }
    drop(query);
    for entity in flight_entities.into_iter().flatten() {
        let _ = world.despawn(entity);
    }
    for aircraft in aircraft.into_iter().flatten() {
        world.spawn((Flight(aircraft), ContactGlow::default()));
    }
}

pub fn new_world(config: RadarConfig) -> (World, Schedule) {
    let mut world = World::new();
    world.insert_resource(DeltaSeconds(0.0));
    world.insert_resource(SweepDegrees(0.0));
    world.insert_resource(RadarProjection::new(
        config.center_latitude,
        config.center_longitude,
        config.radius_m(),
        LOGICAL_WIDTH as f32 / 2.0,
        LOGICAL_HEIGHT as f32 / 2.0,
    ));
    let mut schedule = Schedule::default();
    schedule.add_systems((advance_flights, advance_sweep, update_contact_glow).chain());
    (world, schedule)
}

pub fn set_radar_config(world: &mut World, config: RadarConfig) {
    let mut projection = world.resource_mut::<RadarProjection>();
    projection.latitude = config.center_latitude;
    projection.longitude = config.center_longitude;
    projection.range_m = config.radius_m();
}

fn sweep_contact_strength(projection: &RadarProjection, sweep_degrees: f32, x: f32, y: f32) -> f32 {
    let dx = x - projection.center_x;
    let dy = y - projection.center_y;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < 0.001 {
        return 1.0;
    }

    let angle = sweep_degrees.to_radians();
    let sweep_x = angle.sin();
    let sweep_y = -angle.cos();
    let dot = (dx / distance * sweep_x + dy / distance * sweep_y).clamp(-1.0, 1.0);
    const BEAM_EDGE: f32 = 0.94;
    ((dot - BEAM_EDGE) / (1.0 - BEAM_EDGE)).clamp(0.0, 1.0)
}
