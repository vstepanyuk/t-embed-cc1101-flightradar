//! Device configuration web server.

use core::sync::atomic::Ordering;

use embassy_executor::task;
use embassy_net::{Stack, tcp::TcpSocket};
use log::{info, warn};
use picoserve::{
    AppBuilder, Router,
    extract::Form,
    response::{File, Json, Redirect},
    routing::{get, get_service},
};
use serde::Deserialize;
use static_cell::StaticCell;

use crate::{
    app::{APP_EVENT_BUS, Msg, publish_system_notice},
    config::RadarConfig,
};

const TCP_BUFFER_SIZE: usize = 4096;
const HTTP_BUFFER_SIZE: usize = 4096;
const CONFIG_PAGE: &[u8] = include_bytes!("../../web/config.html");

#[derive(Deserialize)]
struct ConfigForm {
    lat: f32,
    lon: f32,
    radius: f32,
    timezone: i32,
}

struct ConfigApp;

impl AppBuilder for ConfigApp {
    type PathRouter = impl picoserve::routing::PathRouter;

    fn build_app(self) -> Router<Self::PathRouter> {
        Router::new()
            .route("/api/config", get(async || Json(crate::config::current().await)))
            .route(
                "/",
                get_service(File::with_content_type(File::MIME_HTML, CONFIG_PAGE)).post(
                    async |Form(form): Form<ConfigForm>| match RadarConfig::try_new(
                        form.lat,
                        form.lon,
                        form.radius,
                        form.timezone,
                    ) {
                        Ok(config) => {
                            crate::config::update(config).await;
                            let published = APP_EVENT_BUS.try_publish(Msg::ConfigChanged(config));
                            info!("Radar configuration updated; event published: {}", published);
                            Redirect::to("/")
                        }
                        Err(error) => {
                            warn!("Rejected invalid radar configuration: {:?}", error);
                            Redirect::to("/")
                        }
                    },
                ),
            )
    }
}

pub fn start(spawner: embassy_executor::Spawner, stack: Stack<'static>) {
    spawner.spawn(config_server_task(stack).expect("Configuration server task pool is full"));
}

#[task]
async fn config_server_task(stack: Stack<'static>) {
    static APP: StaticCell<picoserve::AppRouter<ConfigApp>> = StaticCell::new();
    static CONFIG: picoserve::Config = picoserve::Config::new(picoserve::Timeouts {
        start_read_request: embassy_time::Duration::from_secs(5),
        persistent_start_read_request: embassy_time::Duration::from_secs(1),
        read_request: embassy_time::Duration::from_secs(3),
        write: embassy_time::Duration::from_secs(5),
    });
    static HTTP_BUFFER: StaticCell<[u8; HTTP_BUFFER_SIZE]> = StaticCell::new();
    static TCP_RX: StaticCell<[u8; TCP_BUFFER_SIZE]> = StaticCell::new();
    static TCP_TX: StaticCell<[u8; TCP_BUFFER_SIZE]> = StaticCell::new();

    let app = APP.init(ConfigApp.build_app());
    let http_buffer = HTTP_BUFFER.init([0; HTTP_BUFFER_SIZE]);
    let tcp_rx = TCP_RX.init([0; TCP_BUFFER_SIZE]);
    let tcp_tx = TCP_TX.init([0; TCP_BUFFER_SIZE]);
    info!("Configuration server listening on http://<device-ip>/");
    loop {
        if !super::CONFIGURATION_MODE.load(Ordering::Acquire) {
            embassy_time::Timer::after_millis(250).await;
            continue;
        }
        info!("Configuration mode enabled; preparing HTTP socket");
        publish_system_notice("CONFIG SERVER STARTING");
        while super::ADSB_REQUEST_ACTIVE.load(core::sync::atomic::Ordering::Acquire) {
            embassy_time::Timer::after_millis(10).await;
        }
        info!("Configuration server waiting for TCP connection on port 80");
        let mut socket = TcpSocket::new(stack, tcp_rx, tcp_tx);
        if let Err(error) = socket.accept(80).await {
            warn!("Configuration server accept failed: {:?}", error);
            publish_system_notice("CONFIG SERVER ACCEPT FAILED");
            continue;
        }
        info!("Configuration server accepted TCP connection");
        publish_system_notice("CONFIGURATION CLIENT CONNECTED");
        socket.set_timeout(Some(embassy_time::Duration::from_secs(5)));
        let server = picoserve::Server::new(app, &CONFIG, http_buffer);
        if let Err(error) = server.serve(socket).await {
            warn!("Configuration HTTP request failed: {:?}", error);
            publish_system_notice("CONFIGURATION REQUEST FAILED");
        } else {
            info!("Configuration HTTP request completed");
            publish_system_notice("CONFIGURATION SAVED");
        }
    }
}
