#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core_s3::CoreS3;
use core_s3::bsp::CoreS3DisplayResources;
use core_s3::power::Axp2101;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::rtc_cntl::{Rtc, reset_reason, wakeup_cause};
use esp_hal::timer::timg::TimerGroup;
use esp_rtos::{self};

use esp_backtrace as _;
use esp_println as _;
use static_cell::StaticCell;

mod dns;
mod ntp;
mod secrets;
mod wifi;

extern crate alloc;

static RTC: StaticCell<Rtc> = StaticCell::new();

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
    info!("Embassy initialised");

    esp_alloc::heap_allocator!(size: 64 * 1024);
    info!("Heap initialised");

    let stack = wifi::init(
        &spawner,
        peripherals.WIFI,
        secrets::WIFI_SSID.into(),
        secrets::WIFI_PASSWORD.into(),
    );
    info!("WiFi started");

    let rtc = RTC.init(Rtc::new(peripherals.RTC_TIMER));
    ntp::start(spawner, rtc, stack);
    info!("NTP started");

    let parts = CoreS3::init_display(CoreS3DisplayResources {
        i2c0: peripherals.I2C0,
        i2c_sda: peripherals.GPIO12,
        i2c_scl: peripherals.GPIO11,
        spi2: peripherals.SPI2,
        lcd_sclk: peripherals.GPIO36,
        lcd_mosi: peripherals.GPIO37,
        lcd_dc: peripherals.GPIO35,
        lcd_cs: peripherals.GPIO3,
        tf_card_cs: peripherals.GPIO4,
    })
    .expect("failed to initialise CoreS3 board");
    info!("CoreS3 initialised");

    let mut pmic = Axp2101::new(parts.internal_i2c);
    pmic.set_display_backlight(0).expect("failed to set display backlight");

    loop {
        info!("time: {}", defmt::Debug2Format(&ntp::get_time(&rtc).await));
        info!("power: {}", pmic.battery_level_percent());

        Timer::after(Duration::from_millis(2000)).await;
    }
}
