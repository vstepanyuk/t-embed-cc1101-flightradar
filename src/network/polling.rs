//! Wi-Fi, DHCP, and periodic ADSB polling.

use alloc::boxed::Box;
use core::{fmt::Write, sync::atomic::Ordering};

use embassy_executor::Spawner;
use embassy_net::{Stack, tcp::client::TcpClientState};
use embassy_time::{Duration, Timer};
use log::{debug, info, warn};

use super::{
    ADSB_REQUEST_ACTIVE, CONFIGURATION_MODE,
    api_client::{ApiClient, RESPONSE_BUFFER_SIZE},
    decoder, places, server,
};
use crate::{
    app::{APP_EVENT_BUS, Msg},
    config::RadarConfig,
    heap_buffer, make_static,
};

pub fn start(spawner: Spawner, stack: &'static Stack<'static>) {
    info!("Starting ADSB launcher task");
    spawner.spawn(adsb_launcher(*stack, spawner).expect("ADSB launcher task pool is full"));
    server::start(spawner, *stack);
}

#[embassy_executor::task]
async fn adsb_launcher(stack: Stack<'static>, spawner: Spawner) {
    info!("ADSB launcher waiting for IPv4 configuration");
    stack.wait_config_up().await;
    info!("ADSB launcher received IPv4 configuration");
    spawner.spawn(adsb_task(stack).expect("ADSB task pool is full"));
    info!("ADSB task spawned");
}

#[embassy_executor::task]
async fn adsb_task(stack: Stack<'static>) {
    info!("ADSB task entered");
    let tcp_state = make_static!(
        TcpClientState<1, 16384, 16384>,
        TcpClientState::<1, 16384, 16384>::new()
    );
    let tls_read = heap_buffer!(16384, "ADSB TLS read buffer allocation failed");
    let tls_write = heap_buffer!(16384, "ADSB TLS write buffer allocation failed");
    let response_buffer =
        heap_buffer!(RESPONSE_BUFFER_SIZE, "ADSB response buffer allocation failed");
    info!("ADSB buffers initialized");
    let mut api_client = ApiClient::new(stack, tcp_state, tls_read, tls_write, response_buffer);
    let mut cities_config = None;

    loop {
        if CONFIGURATION_MODE.load(Ordering::Acquire) {
            Timer::after(Duration::from_millis(250)).await;
            continue;
        }
        debug!("ADSB waiting for IPv4 configuration");
        stack.wait_config_up().await;
        debug!("ADSB IPv4 configuration available");
        if let Some(ip_config) = stack.config_v4() {
            let octets = ip_config.address.address().octets();
            let _ = APP_EVENT_BUS.try_publish(Msg::WifiIp(octets));
            info!("Wi-Fi IP: {:?}", ip_config.address);
        }
        let config = crate::config::current().await;

        ADSB_REQUEST_ACTIVE.store(true, Ordering::Release);
        fetch_aircraft(&mut api_client, config).await;
        if cities_config != Some(config) && fetch_cities(&mut api_client, config).await {
            cities_config = Some(config);
        }
        ADSB_REQUEST_ACTIVE.store(false, Ordering::Release);
        Timer::after(Duration::from_secs(60)).await;
    }
}

async fn fetch_aircraft(api_client: &mut ApiClient, config: RadarConfig) {
    let mut url: heapless::String<256> = heapless::String::new();
    let _ = write!(
        url,
        "https://opendata.adsb.fi/api/v3/lat/{:.4}/lon/{:.4}/dist/{:.0}",
        config.center_latitude, config.center_longitude, config.radius_km
    );
    debug!("Fetching ADSB data from {}", url.as_str());

    let (status, body) = match api_client.get(url.as_str()).await {
        Ok(response) => response,
        Err(error) => {
            warn!("ADSB HTTP request failed: {:?}", error);
            return;
        }
    };
    debug!("ADSB HTTP status: {}, response: {} bytes", status, body.len());
    if status >= 400 {
        warn!("ADSB HTTP request returned status {}", status);
        return;
    }
    let text = match core::str::from_utf8(body) {
        Ok(text) => text,
        Err(error) => {
            warn!("ADSB response was not UTF-8: {:?}", error);
            return;
        }
    };
    let batch = decoder::decode_aircraft(text);
    let count = batch.iter().filter(|item| item.is_some()).count();
    debug!("ADSB.fi aircraft states: {}", count);
    if !APP_EVENT_BUS.try_publish(Msg::AircraftBatch(Box::new(batch))) {
        warn!("App event bus full; dropping aircraft update");
    }
}

async fn fetch_cities(api_client: &mut ApiClient, config: RadarConfig) -> bool {
    let (south, north, west, east) = config.bounds();
    let mut query: heapless::String<512> = heapless::String::new();
    let _ = write!(
        query,
        "[out:json][timeout:10];(node[\"place\"~\"city|town\"]({:.4},{:.4},{:.4},{:.4});way[\"place\"~\"city|town\"]({:.4},{:.4},{:.4},{:.4});relation[\"place\"~\"city|town\"]({:.4},{:.4},{:.4},{:.4}););out center;",
        south, west, north, east, south, west, north, east, south, west, north, east
    );
    info!("Fetching city names from Overpass");

    let (status, body) = match api_client
        .post(
            "https://overpass-api.de/api/interpreter",
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Accept", "application/json"),
                ("User-Agent", "esp-radar/0.1"),
            ],
            query.as_bytes(),
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            warn!("City HTTP request failed: {:?}", error);
            return false;
        }
    };
    debug!("City HTTP status: {}, response: {} bytes", status, body.len());
    if status >= 400 {
        warn!("City HTTP request returned status {}", status);
        return false;
    }
    let text = match core::str::from_utf8(body) {
        Ok(text) => text,
        Err(error) => {
            warn!("City response was not UTF-8: {:?}", error);
            return false;
        }
    };
    let cities = match places::decode_cities(text) {
        Ok(cities) => cities,
        Err(()) => {
            warn!("City update rejected; retrying next cycle");
            return false;
        }
    };
    let count = cities.iter().filter(|city| city.is_some()).count();
    info!("City names received: {}", count);
    if !APP_EVENT_BUS.try_publish(Msg::Cities(Box::new(cities))) {
        warn!("App event bus full; dropping city update");
    }
    true
}
