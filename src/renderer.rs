//! Fast full-frame radar renderer.

use core::fmt::Write;

use bevy_ecs::prelude::*;
use embedded_graphics::{
    framebuffer::{Framebuffer, buffer_size},
    geometry::Point,
    mono_font::{MonoTextStyle, iso_8859_1::FONT_6X10},
    pixelcolor::{Rgb565, raw::BigEndian},
    prelude::{DrawTarget, Drawable, Primitive, RgbColor},
    primitives::{Circle, Line, PrimitiveStyle, Rectangle, Triangle},
    text::Text,
};
use heapless::String;
use micromath::F32Ext;

use crate::{
    display::{LOGICAL_HEIGHT, LOGICAL_WIDTH},
    flight::{City, MAX_CITIES},
    projection::{RadarProjection, aircraft_marker},
    simulation::{ContactGlow, Flight, SweepDegrees},
};

pub const WIDTH: usize = LOGICAL_WIDTH as usize;
pub const HEIGHT: usize = LOGICAL_HEIGHT as usize;
pub type RadarFramebuffer = Framebuffer<
    Rgb565,
    <Rgb565 as embedded_graphics::pixelcolor::PixelColor>::Raw,
    BigEndian,
    WIDTH,
    HEIGHT,
    { buffer_size::<Rgb565>(WIDTH, HEIGHT) },
>;

pub struct RadarRenderer {
    fps: f32,
    status: String<32>,
    ip_address: String<24>,
    cities: [Option<City>; MAX_CITIES],
    verbosity: u8,
    clock: String<20>,
    notice: String<48>,
}

impl RadarRenderer {
    pub fn new() -> Self {
        Self {
            fps: 0.0,
            status: String::new(),
            ip_address: String::new(),
            cities: [const { None }; MAX_CITIES],
            verbosity: 3,
            clock: String::new(),
            notice: String::new(),
        }
    }

    pub fn set_ip(&mut self, ip: impl core::fmt::Display) {
        self.ip_address.clear();
        let _ = write!(self.ip_address, "IP: {}", ip);
    }

    pub fn set_status(&mut self, status: &str) {
        self.status.clear();
        let _ = self.status.push_str(status);
    }

    pub fn set_cities(&mut self, cities: [Option<City>; MAX_CITIES]) {
        self.cities = cities;
    }

    pub fn set_verbosity(&mut self, verbosity: u8) {
        self.verbosity = verbosity.min(9);
    }

    pub fn set_clock(&mut self, value: String<20>) {
        self.clock = value;
    }

    pub fn set_notice(&mut self, notice: &str) {
        self.notice.clear();
        let _ = self.notice.push_str(notice);
    }

    pub fn render(&mut self, framebuffer: &mut RadarFramebuffer, world: &mut World, dt: f32) {
        let projection = *world.resource::<RadarProjection>();
        let sweep_degrees = world.resource::<SweepDegrees>().0;
        if dt > 0.0001 {
            let measured_fps = 1.0 / dt;
            self.fps =
                if self.fps == 0.0 { measured_fps } else { self.fps * 0.9 + measured_fps * 0.1 };
        }
        self.render_background(framebuffer, projection, sweep_degrees);
        self.render_sweep(framebuffer, projection, sweep_degrees);
        if self.verbosity >= 2 {
            self.render_cities(framebuffer, projection);
        }
        let flight_count = self.render_aircraft(framebuffer, world, projection);
        self.render_overlay(framebuffer, flight_count);
    }

    fn render_background(
        &self,
        framebuffer: &mut RadarFramebuffer,
        projection: RadarProjection,
        sweep_degrees: f32,
    ) {
        // Keep the panel almost black so the sweep and contacts have a strong
        // phosphor-like contrast.
        let _ = framebuffer.clear(Rgb565::new(0, 2, 0));
        let center = Point::new(projection.center_x as i32, projection.center_y as i32);
        let breathing = 0.5 + 0.5 * (sweep_degrees * 0.2).to_radians().sin();
        let ring_green = 7 + (breathing * 7.0) as u8;

        for radius in (25..=220).step_by(25) {
            let style = PrimitiveStyle::with_stroke(Rgb565::new(0, ring_green, 2), 1);
            let _ = Circle::with_center(center, radius * 2).into_styled(style).draw(framebuffer);
        }
        for angle in (0..360).step_by(30) {
            let endpoint = radial_point(center, 220.0, angle as f32);
            let _ = Line::new(center, endpoint)
                .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(0, 8, 1), 1))
                .draw(framebuffer);
        }
    }

    fn render_sweep(
        &self,
        framebuffer: &mut RadarFramebuffer,
        projection: RadarProjection,
        sweep_degrees: f32,
    ) {
        let center = Point::new(projection.center_x as i32, projection.center_y as i32);
        // RGB565 has no alpha channel. Approximate transparency by blending
        // each trail segment with the background before it reaches the panel.
        const TAIL_SEGMENTS: usize = 14;
        for index in 0..TAIL_SEGMENTS {
            let progress = index as f32 / TAIL_SEGMENTS as f32;
            let alpha = 0.76 * (1.0 - progress).powf(1.35);
            let green = blend_channel(2, 63, alpha);
            let blue = blend_channel(0, 18, alpha);
            let angle = sweep_degrees - (index as f32 + 1.0) * 1.6;
            let sweep_end = radial_point(center, 240.0, angle);
            let _ = Line::new(center, sweep_end)
                .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(0, green, blue), 1))
                .draw(framebuffer);
        }

        let sweep_end = radial_point(center, 240.0, sweep_degrees);
        let _ = Line::new(center, sweep_end)
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(0, 63, 18), 1))
            .draw(framebuffer);
    }

    fn render_cities(&self, framebuffer: &mut RadarFramebuffer, projection: RadarProjection) {
        let mut label_bounds = [(0, 0, 0, 0); MAX_CITIES];
        let mut label_count = 0;
        for city in self.cities.iter().flatten() {
            let Some((x, y)) = projection.project_location(city.latitude, city.longitude) else {
                continue;
            };
            let name = city.name.as_str();
            let point = Point::new(x as i32, y as i32);
            let _ = Circle::with_center(point, 2)
                .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0, 25, 5)))
                .draw(framebuffer);
            let width = (name.chars().count() as i32 * 6).min(WIDTH as i32 - 2);
            let label_x = (point.x + 4).clamp(1, WIDTH as i32 - width - 1);
            let label_y = (point.y + 3).clamp(10, HEIGHT as i32 - 1);
            let bounds = (label_x, label_y - 10, label_x + width, label_y);
            let overlaps = label_bounds[..label_count].iter().any(|(left, top, right, bottom)| {
                bounds.0 < *right && bounds.2 > *left && bounds.1 < *bottom && bounds.3 > *top
            });
            if !overlaps && label_count < label_bounds.len() {
                label_bounds[label_count] = bounds;
                label_count += 1;
                let _ = Text::new(
                    name,
                    Point::new(label_x, label_y),
                    MonoTextStyle::new(&FONT_6X10, Rgb565::new(0, 25, 5)),
                )
                .draw(framebuffer);
            }
        }
    }

    fn render_aircraft(
        &self,
        framebuffer: &mut RadarFramebuffer,
        world: &mut World,
        projection: RadarProjection,
    ) -> usize {
        let mut flight_count = 0;
        let mut query = world.query::<(&Flight, &ContactGlow)>();
        for (flight, glow) in query.iter(world) {
            flight_count += 1;
            let Some(target) = projection.project(&flight.0) else {
                continue;
            };
            let marker = aircraft_marker(&target);
            let base_color = aircraft_color(target.category, target.id);
            let marker_color = brighten_color(base_color, glow.0 * 0.9);

            if glow.0 > 0.0 {
                let point = Point::new(target.x as i32, target.y as i32);
                let glow_color = brighten_color(base_color, glow.0 * 0.65);
                let _ = Circle::with_center(point, 4 + target.size as u32)
                    .into_styled(PrimitiveStyle::with_stroke(glow_color, 1))
                    .draw(framebuffer);
                let _ = Circle::with_center(point, 2 + target.size as u32 / 2)
                    .into_styled(PrimitiveStyle::with_stroke(marker_color, 1))
                    .draw(framebuffer);
            }

            let _ = Triangle::new(marker.tip, marker.left, marker.right)
                .into_styled(PrimitiveStyle::with_fill(marker_color))
                .draw(framebuffer);

            if self.verbosity >= 3 {
                let name = flight.0.name.as_str();
                if !name.is_empty() {
                    let label = Point::new(
                        marker.tip.x + (target.direction_x * 4.0) as i32,
                        marker.tip.y + (target.direction_y * 4.0) as i32,
                    );
                    let _ = Text::new(
                        name,
                        label,
                        MonoTextStyle::new(&FONT_6X10, aircraft_color(target.category, target.id)),
                    )
                    .draw(framebuffer);
                }
            }
        }
        flight_count
    }

    fn render_overlay(&self, framebuffer: &mut RadarFramebuffer, flight_count: usize) {
        let mut verbosity_text: String<8> = String::new();
        let _ = write!(verbosity_text, "V:{}", self.verbosity);
        let _ = Text::new(
            verbosity_text.as_str(),
            Point::new(296, 13),
            MonoTextStyle::new(&FONT_6X10, Rgb565::new(0, 45, 12)),
        )
        .draw(framebuffer);

        let mut line_y = 25;
        let bright_style = MonoTextStyle::new(&FONT_6X10, Rgb565::new(0, 63, 18));
        let compact_style = MonoTextStyle::new(&FONT_6X10, Rgb565::new(0, 45, 12));
        let connecting = flight_count == 0 && self.status.as_str() == "[CONNECTING]...";

        let _ = Text::new(self.clock.as_str(), Point::new(5, 13), bright_style).draw(framebuffer);

        if self.verbosity >= 6 && !connecting {
            let _ = Text::new(self.status.as_str(), Point::new(5, line_y), compact_style)
                .draw(framebuffer);
            line_y += 12;
        }

        if self.verbosity >= 7 {
            let _ = Text::new(self.ip_address.as_str(), Point::new(5, line_y), compact_style)
                .draw(framebuffer);
            line_y += 12;
        }

        if self.verbosity >= 8 {
            let mut fps_text: String<16> = String::new();
            let _ = write!(fps_text, "FPS: {}", self.fps as u32);
            let _ = Text::new(
                fps_text.as_str(),
                Point::new(5, line_y),
                MonoTextStyle::new(&FONT_6X10, Rgb565::new(0, 45, 12)),
            )
            .draw(framebuffer);
            line_y += 12;
        }

        let status_point = if connecting { Point::new(118, 85) } else { Point::new(5, line_y) };
        if connecting {
            let _ = Text::new(self.status.as_str(), status_point, compact_style).draw(framebuffer);
        }

        self.render_notice(framebuffer);
    }

    fn render_notice(&self, framebuffer: &mut RadarFramebuffer) {
        if self.notice.is_empty() {
            return;
        }
        let width = (self.notice.chars().count() as u32 * 6 + 16).min(WIDTH as u32 - 8);
        let left = ((WIDTH as u32 - width) / 2) as i32;
        let _ =
            Rectangle::new(Point::new(left, 68), embedded_graphics::geometry::Size::new(width, 34))
                .into_styled(PrimitiveStyle::with_fill(Rgb565::new(63, 63, 0)))
                .draw(framebuffer);
        let _ = Text::new(
            self.notice.as_str(),
            Point::new(left + 8, 89),
            MonoTextStyle::new(&FONT_6X10, Rgb565::new(0, 0, 0)),
        )
        .draw(framebuffer);
    }
}

impl Default for RadarRenderer {
    fn default() -> Self {
        Self::new()
    }
}

fn radial_point(center: Point, radius: f32, degrees: f32) -> Point {
    let angle = degrees.to_radians();
    Point::new(center.x + (angle.sin() * radius) as i32, center.y - (angle.cos() * radius) as i32)
}

fn blend_channel(background: u8, foreground: u8, alpha: f32) -> u8 {
    (background as f32 + (foreground - background) as f32 * alpha) as u8
}

fn aircraft_color(category: u8, id: u32) -> Rgb565 {
    let palette = [
        rgb565(0xB0B0B0), // unknown
        rgb565(0x00FF50), // no category
        rgb565(0x00C8FF), // light
        rgb565(0x2878FF), // small
        rgb565(0xFF8C00), // large
        rgb565(0xFF4000), // high-vortex large
        rgb565(0xDC00A0), // heavy
        rgb565(0xFFE000), // high performance
        rgb565(0x00E0B0), // rotorcraft
    ];
    let base = palette.get(category as usize).copied().unwrap_or(palette[0]);
    let variation = (id & 0x03) as u8;
    Rgb565::new(
        base.r().saturating_sub(variation),
        base.g().saturating_sub(variation),
        base.b().saturating_sub(variation),
    )
}

fn brighten_color(color: Rgb565, amount: f32) -> Rgb565 {
    let amount = amount.clamp(0.0, 1.0);
    Rgb565::new(
        color.r() + (((31 - color.r()) as f32 * amount) as u8),
        color.g() + (((63 - color.g()) as f32 * amount) as u8),
        color.b() + (((31 - color.b()) as f32 * amount) as u8),
    )
}

/// Convert a standard `0xRRGGBB` color into the panel's RGB565 format.
pub const fn rgb565(hex: u32) -> Rgb565 {
    let red = ((hex >> 16) & 0xff) as u8;
    let green = ((hex >> 8) & 0xff) as u8;
    let blue = (hex & 0xff) as u8;
    Rgb565::new(red >> 3, green >> 2, blue >> 3)
}
