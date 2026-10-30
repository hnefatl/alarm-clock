#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::rtc_cntl::{Rtc, reset_reason, wakeup_cause};
use esp_hal::timer::timg::TimerGroup;
use esp_rtos;

use esp_backtrace as _;
use esp_println as _;

mod secrets;
mod wifi;

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    info!(
        "Reset due to: {:?} ({})",
        defmt::Debug2Format(&reset_reason(esp_hal::system::Cpu::ProCpu)),
        wakeup_cause(),
    );

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    //esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73000);
    esp_rtos::start(timg0.timer0);
    let mut _rtc = Rtc::new(peripherals.LPWR);
    info!("Embassy initialized");

    esp_alloc::heap_allocator!(size: 64 * 1024);

    let stack = wifi::init(
        &spawner,
        peripherals.WIFI,
        secrets::WIFI_SSID.into(),
        secrets::WIFI_PASSWORD.into(),
    );

    loop {
        info!("ipv4: {}", stack.config_v4());
        info!("ipv6: {}", stack.config_v6());
        Timer::after(Duration::from_millis(1000)).await
    }
}
