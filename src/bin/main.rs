#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those holding buffers for the duration of a data transfer."
)]
#![warn(clippy::large_stack_frames)]

extern crate alloc;

use alloc::boxed::Box;

use device_envoy_esp::{
    button::{Button as _, PressDuration, PressedTo},
    button_watch,
    flash_block::FlashBlockEsp,
    wifi_auto::{WifiAuto as _, WifiAutoEsp, WifiAutoEvent},
};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use esp_hal::{
    Async,
    clock::CpuClock,
    dma::{DmaRxBuf, DmaTxBuf},
    gpio::{Input, InputConfig, Level, Output, Pull},
    spi::master::SpiDmaBus,
    timer::timg::TimerGroup,
};
use esp_println as _;
use esp_radar::{
    app::{APP_EVENT_BUS, App, Msg, publish_system_notice},
    display::{self, BoardDisplay},
    network, power,
    renderer::RadarFramebuffer,
};
use lcd_async::{
    Builder,
    interface::SpiInterface,
    options::{ColorInversion, Orientation, Rotation},
};
use log::{debug, error, info, warn};
use static_cell::StaticCell;

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    error!("{}", panic_info);
    loop {}
}

const WIFI_SETUP_SSID: &str = "ESP-Radar-Setup";
const WIFI_SETUP_IP: &str = "192.168.4.1";

static WIFI_AUTO_CELL: StaticCell<WifiAutoEsp<'static>> = StaticCell::new();
static SPI_BUS: StaticCell<Mutex<NoopRawMutex, SpiDmaBus<'static, Async>>> = StaticCell::new();

button_watch! {
    WifiResetButton {
        pin: GPIO0,
    }
}

async fn initialize_display(
    spi: esp_hal::peripherals::SPI2<'static>,
    dma_channel: esp_hal::peripherals::DMA_CH0<'static>,
    sck: esp_hal::peripherals::GPIO11<'static>,
    mosi: esp_hal::peripherals::GPIO9<'static>,
    chip_select: esp_hal::peripherals::GPIO41<'static>,
    data_command: esp_hal::peripherals::GPIO16<'static>,
) -> Result<BoardDisplay, &'static str> {
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = esp_hal::dma_buffers!(4, 32_000);
    let rx_buffer = DmaRxBuf::new(rx_descriptors, rx_buffer)
        .map_err(|_| "DMA RX buffer initialization failed")?;
    let tx_buffer = DmaTxBuf::new(tx_descriptors, tx_buffer)
        .map_err(|_| "DMA TX buffer initialization failed")?;

    let spi = esp_hal::spi::master::Spi::new(
        spi,
        esp_hal::spi::master::Config::default()
            .with_frequency(esp_hal::time::Rate::from_mhz(80))
            .with_mode(esp_hal::spi::Mode::_0),
    )
    .map_err(|_| "SPI initialization failed")?
    .with_sck(sck)
    .with_mosi(mosi)
    .with_dma(dma_channel)
    .with_buffers(rx_buffer, tx_buffer)
    .into_async();

    let spi_bus = SPI_BUS.init(Mutex::new(spi));
    let di = SpiInterface::new(
        SpiDevice::new(spi_bus, Output::new(chip_select, Level::High, Default::default())),
        Output::new(data_command, Level::Low, Default::default()),
    );
    let mut delay = embassy_time::Delay;
    let display = Builder::new(lcd_async::models::ST7789, di)
        .display_size(display::PANEL_WIDTH, display::PANEL_HEIGHT)
        .orientation(Orientation { rotation: Rotation::Deg270, mirrored: false })
        .display_offset(35, 0)
        .invert_colors(ColorInversion::Inverted)
        .color_order(lcd_async::options::ColorOrder::Rgb)
        .init(&mut delay)
        .await
        .map_err(|_| "Display initialization failed")?;

    Ok(BoardDisplay { display })
}

fn start_encoder_task(
    spawner: Spawner,
    pcnt: esp_hal::peripherals::PCNT<'static>,
    encoder_a: esp_hal::peripherals::GPIO4<'static>,
    encoder_b: esp_hal::peripherals::GPIO5<'static>,
) {
    let encoder_config = InputConfig::default().with_pull(Pull::Up);
    esp_radar::encoder::start(
        spawner,
        pcnt,
        Input::new(encoder_a, encoder_config),
        Input::new(encoder_b, encoder_config),
    );
}

async fn initialize_wifi(
    wifi: esp_hal::peripherals::WIFI<'static>,
    flash: esp_hal::peripherals::FLASH<'static>,
    reset_pin: esp_hal::peripherals::GPIO0<'static>,
    spawner: Spawner,
) -> (&'static WifiAutoEsp<'static>, &'static mut WifiResetButton) {
    let [wifi_flash] = FlashBlockEsp::new_array::<1>(flash)
        .unwrap_or_else(|error| panic!("Wi-Fi flash storage initialization failed: {:?}", error));
    let wifi_reset_button = WifiResetButton::new(reset_pin, PressedTo::Ground, spawner)
        .await
        .unwrap_or_else(|error| panic!("Wi-Fi reset button initialization failed: {:?}", error));
    let wifi_auto = WifiAutoEsp::new(wifi, wifi_flash, WIFI_SETUP_SSID, [], spawner)
        .unwrap_or_else(|error| panic!("Wi-Fi auto-setup initialization failed: {:?}", error));
    (WIFI_AUTO_CELL.init(wifi_auto), wifi_reset_button)
}

#[embassy_executor::task]
async fn wifi_connect_task(
    wifi_auto: &'static WifiAutoEsp<'static>,
    wifi_reset_button: &'static mut WifiResetButton,
    spawner: Spawner,
) {
    info!("Wi-Fi task started");
    info!("Entering device-envoy Wi-Fi connect");
    let stack = match wifi_auto
        .connect(wifi_reset_button, async |event| {
            match event {
                WifiAutoEvent::CaptivePortalReady => {
                    info!("Wi-Fi setup portal ready at {}", WIFI_SETUP_IP);
                    publish_system_notice("SETUP WIFI: 192.168.4.1");
                }
                WifiAutoEvent::Connecting { .. } => {
                    info!("Connecting to Wi-Fi");
                    publish_system_notice("CONNECTING TO WIFI...");
                }
                WifiAutoEvent::ConnectionFailed => {
                    warn!("Wi-Fi connection failed");
                    publish_system_notice("WIFI CONNECTION FAILED");
                }
            }
            Ok::<(), device_envoy_esp::Error>(())
        })
        .await
    {
        Ok(stack) => stack,
        Err(error) => {
            error!("Wi-Fi auto-setup failed: {:?}", error);
            publish_system_notice("WIFI SETUP ERROR");
            return;
        }
    };

    info!("device-envoy Wi-Fi connect returned; starting network services");
    network::start(spawner, stack);
    esp_radar::clock::start(spawner, stack);
    let published = APP_EVENT_BUS.try_publish(Msg::WifiReady);
    info!("Published WifiReady event: {}", published);
    loop {
        match wifi_reset_button.wait_for_press_duration().await {
            PressDuration::Short => debug!("Short GPIO0 press ignored"),
            PressDuration::Long => {
                info!("Long GPIO0 press: resetting Wi-Fi setup");
                if let Err(error) = wifi_auto.reset_to_captive_portal() {
                    error!("Wi-Fi reset failed: {:?}", error);
                    continue;
                }
                info!("Wi-Fi credentials cleared; rebooting into setup portal");
                Timer::after(Duration::from_secs(1)).await;
                esp_hal::system::software_reset();
            }
        }
    }
}

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    esp_println::logger::init_logger(log::LevelFilter::Info);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    info!("Embassy initialized!");
    let mut power_enable = Output::new(peripherals.GPIO15, Level::Low, Default::default());
    let mut backlight = Output::new(peripherals.GPIO21, Level::Low, Default::default());
    power_enable.set_high();
    Timer::after(Duration::from_millis(10)).await;

    let mut display = initialize_display(
        peripherals.SPI2,
        peripherals.DMA_CH0,
        peripherals.GPIO11,
        peripherals.GPIO9,
        peripherals.GPIO41,
        peripherals.GPIO16,
    )
    .await
    .unwrap_or_else(|error| panic!("Display initialization failed: {}", error));
    info!("Display initialized!");
    backlight.set_high();

    let mut framebuffer = Box::new(RadarFramebuffer::new());
    let radar_config = esp_radar::config::current().await;
    let mut app = App::new(radar_config);
    info!("Direct radar renderer initialized!");

    app.set_startup_state(WIFI_SETUP_IP, "[CONNECTING]", "CONNECTING...");
    app.render(&mut framebuffer, 0.016);
    if !display.flush(framebuffer.data()).await {
        error!("Display flush failed");
    }

    start_encoder_task(spawner, peripherals.PCNT, peripherals.GPIO4, peripherals.GPIO5);
    power::start(
        spawner,
        peripherals.GPIO6,
        esp_hal::rtc_cntl::Rtc::new(peripherals.LPWR),
        power_enable,
        backlight,
    );
    let (wifi_auto, wifi_reset_button) =
        initialize_wifi(peripherals.WIFI, peripherals.FLASH, peripherals.GPIO0, spawner).await;
    spawner.spawn(
        wifi_connect_task(wifi_auto, wifi_reset_button, spawner).expect("Wi-Fi task pool is full"),
    );
    let _ = &mut framebuffer;
    let mut last_frame = embassy_time::Instant::now();
    info!("Entering reactive display loop");
    loop {
        let now = embassy_time::Instant::now();
        let delta = now.duration_since(last_frame).as_micros() as f32 / 1_000_000.0;
        last_frame = now;
        app.update(delta);
        app.render(&mut framebuffer, delta);
        if !display.flush(framebuffer.data()).await {
            error!("Display flush failed");
        }
        Timer::after_millis(1).await;
    }
}
