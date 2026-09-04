//! Rotary encoder input for the T-Embed verbosity control.

use core::cell::RefCell;

use critical_section::Mutex;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use esp_hal::{
    gpio::Input,
    pcnt::{
        Pcnt,
        channel::{CtrlMode, EdgeMode},
        unit::Unit,
    },
};
use log::warn;

use crate::app::{APP_EVENT_BUS, Msg};

const VERBOSITY_LEVELS: u8 = 10;
const INITIAL_VERBOSITY: u8 = 8;
const FILTER_THRESHOLD: u16 = 100;
// Full quadrature decoding produces four counts per mechanical detent.
const COUNTS_PER_STEP: i16 = 4;

static PCNT_UNIT: Mutex<RefCell<Option<Unit<'static, 0>>>> = Mutex::new(RefCell::new(None));
static ENCODER_EVENTS: Channel<CriticalSectionRawMutex, i16, 8> = Channel::new();

pub fn start(
    spawner: Spawner,
    pcnt_peripheral: esp_hal::peripherals::PCNT<'static>,
    ina: Input<'static>,
    inb: Input<'static>,
) {
    let mut pcnt = Pcnt::new(pcnt_peripheral);
    pcnt.set_interrupt_handler(pcnt_interrupt_handler);
    let unit = pcnt.unit0;
    let input_a = ina.peripheral_input();
    let input_b = inb.peripheral_input();
    let channel0 = &unit.channel0;
    channel0.set_ctrl_signal(input_a.clone());
    channel0.set_edge_signal(input_b.clone());
    channel0.set_ctrl_mode(CtrlMode::Reverse, CtrlMode::Keep);
    channel0.set_input_mode(EdgeMode::Increment, EdgeMode::Decrement);
    let channel1 = &unit.channel1;
    channel1.set_ctrl_signal(input_b);
    channel1.set_edge_signal(input_a);
    channel1.set_ctrl_mode(CtrlMode::Reverse, CtrlMode::Keep);
    channel1.set_input_mode(EdgeMode::Decrement, EdgeMode::Increment);
    unit.set_filter(Some(FILTER_THRESHOLD)).unwrap();
    unit.set_high_limit(Some(COUNTS_PER_STEP)).unwrap();
    unit.set_low_limit(Some(-COUNTS_PER_STEP)).unwrap();
    unit.clear();
    unit.listen();
    critical_section::with(|cs| PCNT_UNIT.borrow_ref_mut(cs).replace(unit));
    critical_section::with(|cs| PCNT_UNIT.borrow_ref(cs).as_ref().unwrap().resume());

    spawner.spawn(encoder_task().expect("Encoder task pool is full"));
}

#[esp_hal::handler]
fn pcnt_interrupt_handler() {
    critical_section::with(|cs| {
        let mut unit = PCNT_UNIT.borrow_ref_mut(cs);
        let Some(unit) = unit.as_mut() else { return };
        if !unit.interrupt_is_set() {
            return;
        }
        let events = unit.events();
        if events.high_limit {
            let _ = ENCODER_EVENTS.try_send(1);
        }
        if events.low_limit {
            let _ = ENCODER_EVENTS.try_send(-1);
        }
        unit.reset_interrupt();
    });
}

#[embassy_executor::task]
async fn encoder_task() {
    let mut verbosity = INITIAL_VERBOSITY;
    publish_verbosity(verbosity);

    loop {
        let steps = ENCODER_EVENTS.receive().await;
        verbosity = (verbosity as i16 - steps).rem_euclid(VERBOSITY_LEVELS as i16) as u8;
        publish_verbosity(verbosity);
    }
}

fn publish_verbosity(level: u8) {
    if !APP_EVENT_BUS.try_publish(Msg::VerbosityChanged(level)) {
        warn!("App event bus full; dropping verbosity update");
    }
}
