//! Application-wide events exchanged by the independent Embassy tasks.

use alloc::boxed::Box;

use bevy_ecs::prelude::{Schedule, World};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use heapless::String;
use log::{debug, info, warn};

use crate::{
    config::RadarConfig,
    flight::{Aircraft, City, MAX_AIRCRAFT, MAX_CITIES},
    network,
    renderer::{RadarFramebuffer, RadarRenderer},
    simulation::{self, replace_with_live_data},
};

pub enum Msg {
    WifiReady,
    WifiIp([u8; 4]),
    AircraftBatch(Box<[Option<Aircraft>; MAX_AIRCRAFT]>),
    Cities(Box<[Option<City>; MAX_CITIES]>),
    VerbosityChanged(u8),
    ClockTime(String<20>),
    ConfigChanged(RadarConfig),
    SystemNotice(String<48>),
}

pub struct AppEventBus {
    channel: Channel<CriticalSectionRawMutex, Msg, 8>,
}

impl AppEventBus {
    pub const fn new() -> Self {
        Self { channel: Channel::new() }
    }

    pub fn try_publish(&self, message: Msg) -> bool {
        self.channel.try_send(message).is_ok()
    }

    pub fn try_receive(&self) -> Result<Msg, embassy_sync::channel::TryReceiveError> {
        self.channel.try_receive()
    }
}

impl Default for AppEventBus {
    fn default() -> Self {
        Self::new()
    }
}

pub static APP_EVENT_BUS: AppEventBus = AppEventBus::new();

pub fn publish_system_notice(text: &str) {
    let mut notice = String::<48>::new();
    let _ = notice.push_str(text);
    if !APP_EVENT_BUS.try_publish(Msg::SystemNotice(notice)) {
        warn!("System notice dropped: {}", text);
    }
}

pub struct App {
    renderer: RadarRenderer,
    world: World,
    schedule: Schedule,
}

impl App {
    pub fn new(config: RadarConfig) -> Self {
        let (world, schedule) = simulation::new_world(config);
        Self { renderer: RadarRenderer::new(), world, schedule }
    }

    pub fn update(&mut self, delta: f32) {
        self.process_messages();
        self.world.resource_mut::<simulation::DeltaSeconds>().0 = delta.min(0.1);
        self.schedule.run(&mut self.world);
    }

    pub fn set_startup_state(&mut self, ip: &str, status: &str, notice: &str) {
        self.renderer.set_ip(ip);
        self.renderer.set_status(status);
        self.renderer.set_notice(notice);
    }

    pub fn render(&mut self, framebuffer: &mut RadarFramebuffer, delta: f32) {
        self.renderer.render(framebuffer, &mut self.world, delta.min(0.1));
    }

    fn process_messages(&mut self) {
        while let Ok(message) = APP_EVENT_BUS.try_receive() {
            self.handle_message(message);
        }
    }

    fn handle_message(&mut self, message: Msg) {
        match message {
            Msg::WifiReady => {
                debug!("Main received WifiReady event");
                self.renderer.set_status("WIFI CONNECTED");
                self.renderer.set_notice("");
            }
            Msg::WifiIp(ip) => {
                debug!("Main received WifiIp event");
                self.renderer.set_ip(embassy_net::Ipv4Address::new(ip[0], ip[1], ip[2], ip[3]));
            }
            Msg::AircraftBatch(aircraft) => {
                debug!("Main received AircraftBatch event");
                replace_with_live_data(&mut self.world, *aircraft);
            }
            Msg::Cities(cities) => self.renderer.set_cities(*cities),
            Msg::VerbosityChanged(level) => {
                info!("Display verbosity changed to {}", level);
                self.renderer.set_verbosity(level);
                let configuration_mode = level == 9;
                network::set_configuration_mode(configuration_mode);
                if configuration_mode {
                    self.renderer.set_status("[CONFIGURE]");
                    self.renderer.set_notice("CONFIGURATION MODE");
                } else {
                    self.renderer.set_status("WIFI CONNECTED");
                    self.renderer.set_notice("");
                }
            }
            Msg::ClockTime(value) => self.renderer.set_clock(value),
            Msg::ConfigChanged(config) => {
                info!("Main received radar configuration update");
                simulation::set_radar_config(&mut self.world, config);
            }
            Msg::SystemNotice(notice) => {
                debug!("Main received system notice: {}", notice);
                self.renderer.set_notice(notice.as_str());
            }
        }
    }
}
