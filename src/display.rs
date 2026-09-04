use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use esp_hal::{Async, gpio::Output};
use lcd_async::interface::SpiInterface;

/// Logical framebuffer dimensions after display rotation.
pub const LOGICAL_WIDTH: u16 = 320;
pub const LOGICAL_HEIGHT: u16 = 170;

/// Physical ST7789 panel dimensions before rotation.
pub const PANEL_WIDTH: u16 = 170;
pub const PANEL_HEIGHT: u16 = 320;

type DisplaySpi = SpiDevice<
    'static,
    NoopRawMutex,
    esp_hal::spi::master::SpiDmaBus<'static, Async>,
    Output<'static>,
>;
type LcdDisplay = lcd_async::Display<
    SpiInterface<DisplaySpi, Output<'static>>,
    lcd_async::models::ST7789,
    lcd_async::NoResetPin,
>;

pub struct BoardDisplay {
    pub display: LcdDisplay,
}

impl BoardDisplay {
    /// Send one complete logical screen to the panel. The ST7789 orientation
    /// configured by the board setup maps the logical 320x170 framebuffer to
    /// the physical 170x320 panel.
    pub async fn flush(&mut self, data: &[u8]) -> bool {
        self.display.show_raw_data(0, 0, LOGICAL_WIDTH, LOGICAL_HEIGHT, data).await.is_ok()
    }
}
