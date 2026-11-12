#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use anyhow::Context;
use core_s3::CoreS3;
use core_s3::audio::WavPcm16;
use core_s3::bsp::CoreS3DisplayResources;
use core_s3::power::Axp2101;
use defmt::{Debug2Format, Display2Format, debug, error, info};
use embassy_executor::Spawner;
use embassy_sync::once_lock::OnceLock;
use embassy_time::Timer;
use esp_hal::clock::CpuClock;
use esp_hal::rtc_cntl::{Rtc, reset_reason, wakeup_cause};
use esp_hal::timer::timg::TimerGroup;
use esp_rtos::{self};

use esp_backtrace as _;
use esp_println as _;

mod audio;
mod calendar;
mod dns;
mod ntp;
mod secrets;
mod wifi;

extern crate alloc;

static RTC: OnceLock<Rtc> = OnceLock::new();
static STARTUP_US: OnceLock<u64> = OnceLock::new();

// Print the timestamp since reset on each log line.
// We need to track when the current program started, because
// the RTC shows the time since last power cycle, which might've been a while
// back if running on battery.
// https://defmt.ferrous-systems.com/timestamps
defmt::timestamp!("{=u64:us}s", {
    if let Some(rtc) = RTC.try_get() {
        let current_us = rtc.current_time_us();
        let startup_us = STARTUP_US.try_get().unwrap_or(&0);
        current_us - startup_us
    } else {
        0
    }
});

esp_bootloader_esp_idf::esp_app_desc!();

// Alarm disabled for faster flashing during testing.
//const ALARM_WAV: &[u8] = include_bytes!("../alarm_sounds/alarm.wav");
const ALARM_WAV: &[u8] = &[0u8; 0];

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_hal::main]
async fn main(spawner: Spawner) -> ! {
    // TODO: why does ".expect" not log the panic message in the error handler?
    info!(
        "Reset due to: {:?} ({})",
        defmt::Debug2Format(&reset_reason(esp_hal::system::Cpu::ProCpu)),
        wakeup_cause(),
    );

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    if let Err(_) = RTC.init(Rtc::new(peripherals.RTC_TIMER)) {
        panic!("Failed to init RTC");
    };
    let rtc = RTC.get().await;
    STARTUP_US
        .init(rtc.current_time_us())
        .expect("STARTUP_CYCLES already set");

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sleep = esp_rtos::sleep::configure(peripherals.LPWR);
    esp_rtos::start_with_idle_hook(timg0.timer0, peripherals.FROM_CPU_INTR0, sleep.light_sleep_hook);
    info!("Embassy initialised");

    esp_alloc::heap_allocator!(size: 64 * 1024);
    info!("Heap initialised");

    let secrets = secrets::init();
    debug!("Secrets: {:?}", defmt::Debug2Format(&secrets));

    let stack = wifi::init(&spawner, peripherals.WIFI, secrets.wifi_ssid, secrets.wifi_password);
    info!("WiFi started");

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

    let internal_i2c = audio::init(parts.internal_i2c);
    info!("Audio initialised");

    let mut pmic = Axp2101::new(internal_i2c);
    pmic.set_display_backlight(0).expect("failed to set display backlight");

    let audio_peripherals = audio::AudioPeripherals {
        i2s1: peripherals.I2S1,
        dma_ch1: peripherals.DMA_CH1,
        gpio13: peripherals.GPIO13,
        gpio33: peripherals.GPIO33,
        gpio34: peripherals.GPIO34,
    };
    // Alarm disabled for faster flashing during testing.
    //let alarm = WavPcm16::new(ALARM_WAV).expect("failed to parse alarm sound");
    //audio::play_audio(audio_peripherals, alarm).await;

    let mut ntp = ntp::NTP::new(stack);

    let mut do_loop = async || -> anyhow::Result<()> {
        stack.wait_config_up().await;

        info!("power: {}", pmic.battery_level_percent());

        let walltime = ntp.get_utc(rtc).await.context("Failed to get latest time")?;
        info!("time: {}", Display2Format(&walltime));

        // Only fetch calendar events starting today: this'll misbehave if a wakeup event
        // is around midnight, but that's unlikely.
        let start_date = walltime.truncate_to_day();
        let wakeup_time = calendar::get_next_wakeup_time(stack, start_date, &secrets)
            .await
            .context("next wakeup check failed")?;

        info!("next wakeupevent at: {:?}", Debug2Format(&wakeup_time));
        Ok(())
    };

    loop {
        if let Err(e) = do_loop().await {
            // Use alloc format to trigger the expanded "alternate" anyhow error message
            // which includes details on wrapped errors.
            error!("{}", alloc::format!("{:#}", e));
        }
        Timer::after_secs(5).await;
    }
}
