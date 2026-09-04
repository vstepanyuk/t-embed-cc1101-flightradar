//! NTP-backed local clock integration.

use core::fmt::Write;

use device_envoy_esp::clock_sync::{ClockSync as _, ClockSyncEsp, ClockSyncStaticEsp, ONE_SECOND};
use embassy_executor::Spawner;
use embassy_net::Stack;
use heapless::String;
use log::{error, warn};

use crate::app::{APP_EVENT_BUS, Msg};

pub fn start(spawner: Spawner, stack: &'static Stack<'static>) {
    spawner.spawn(clock_task(stack, spawner).expect("Clock task pool is full"));
}

#[embassy_executor::task]
async fn clock_task(stack: &'static Stack<'static>, spawner: Spawner) {
    static CLOCK_SYNC_STATIC: ClockSyncStaticEsp = ClockSyncEsp::new_static();
    let config = crate::config::current().await;
    let clock_sync = match ClockSyncEsp::new(
        &CLOCK_SYNC_STATIC,
        stack,
        config.timezone_offset_minutes,
        Some(ONE_SECOND),
        spawner,
    ) {
        Ok(clock_sync) => clock_sync,
        Err(error) => {
            error!("Clock synchronization initialization failed: {:?}", error);
            return;
        }
    };

    loop {
        clock_sync.set_offset_minutes(crate::config::current().await.timezone_offset_minutes);
        let tick = clock_sync.wait_for_tick().await;
        let local_time = tick.local_time;
        let mut formatted: String<20> = String::new();
        let _ = write!(
            formatted,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            local_time.year(),
            local_time.month() as u8,
            local_time.day(),
            local_time.hour(),
            local_time.minute(),
            local_time.second()
        );
        if !APP_EVENT_BUS.try_publish(Msg::ClockTime(formatted)) {
            warn!("App event bus full; dropping clock update");
        }
    }
}
