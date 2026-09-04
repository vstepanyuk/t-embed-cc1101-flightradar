//! Low-power controls for the board buttons.

use device_envoy_esp::button::{Button as _, ButtonEsp, PressDuration, PressedTo};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::{
    gpio::Output,
    peripherals::GPIO6,
    rtc_cntl::{
        Rtc,
        sleep::{Ext0WakeupSource, WakeupLevel},
    },
};
use log::info;

/// Starts the GPIO6 long-press sleep controller.
pub fn start(
    spawner: Spawner,
    button: GPIO6<'static>,
    rtc: Rtc<'static>,
    power_enable: Output<'static>,
    backlight: Output<'static>,
) {
    spawner.spawn(
        sleep_button_task(button, rtc, power_enable, backlight)
            .expect("Sleep button task pool is full"),
    );
}

#[embassy_executor::task]
async fn sleep_button_task(
    button: GPIO6<'static>,
    mut rtc: Rtc<'static>,
    mut power_enable: Output<'static>,
    mut backlight: Output<'static>,
) {
    info!("GPIO6 sleep button task started");
    let mut button_pin = button;
    let mut button = ButtonEsp::new(button_pin.reborrow(), PressedTo::Ground);

    loop {
        match button.wait_for_press_duration().await {
            PressDuration::Short => continue,
            PressDuration::Long => {
                info!("GPIO6 long press; waiting for release before sleep");
                while button.is_pressed() {
                    Timer::after(Duration::from_millis(20)).await;
                }
                break;
            }
        }
    }

    backlight.set_low();
    power_enable.set_low();
    info!("Entering deep sleep; GPIO6 wakes the device");
    let ext0 = Ext0WakeupSource::new(button_pin, WakeupLevel::Low);
    rtc.sleep_deep(&[&ext0]);
}
