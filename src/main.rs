#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::net::SocketAddr;

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::rtc_cntl::{Rtc, reset_reason, wakeup_cause};
use esp_hal::timer::timg::TimerGroup;
use esp_rtos::{self};

use esp_backtrace as _;
use esp_println as _;

mod dns;
mod ntp;
mod secrets;
mod wifi;

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_hal::main]
async fn main(spawner: Spawner) -> ! {
    info!(
        "Reset due to: {:?} ({})",
        defmt::Debug2Format(&reset_reason(esp_hal::system::Cpu::ProCpu)),
        wakeup_cause(),
    );

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    // TODO: enable light sleep when upgraded to esp-rtos 0.4.0
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    let mut _rtc = Rtc::new(peripherals.RTC_TIMER);
    info!("Embassy initialized");

    esp_alloc::heap_allocator!(size: 64 * 1024);

    let stack = wifi::init(
        &spawner,
        peripherals.WIFI,
        secrets::WIFI_SSID.into(),
        secrets::WIFI_PASSWORD.into(),
    );

    loop {
        Timer::after(Duration::from_millis(2000)).await;

        let Ok(ntp_result) = dns::run_against_dns(stack, "time.google.com.", async |addr| {
            ntp::get_time(stack, SocketAddr::new(addr, ntp::PORT)).await
        })
        .await
        else {
            error!("get time error");
            continue;
        };
        match ntp::ntp_to_utc(ntp_result) {
            Ok(t) => info!("time: {}", defmt::Display2Format(&t)),
            Err(e) => error!("invalid NTP result: {}", defmt::Display2Format(&e)),
        }
    }
}

