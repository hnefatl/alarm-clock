#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::net::{IpAddr, SocketAddr};

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::rtc_cntl::{Rtc, reset_reason, wakeup_cause};
use esp_hal::timer::timg::TimerGroup;
use esp_rtos::{self};

use esp_backtrace as _;
use esp_println as _;

mod ntp;
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
        Timer::after(Duration::from_millis(1000)).await;
        info!("ipv4: {}", stack.config_v4());
        info!("ipv6: {}", stack.config_v6());

        let ntp_addrs = dns_query(stack, "time.google.com.").await;
        for ntp_addr in ntp_addrs {
            match ntp::get_time(stack, SocketAddr::new(ntp_addr, 123)).await {
                Ok(r) => {
                    info!("time: {:?}", r);
                    break;
                }
                Err(e) => error!("get time error"),
            }
        }
    }
}

async fn dns_query(
    stack: embassy_net::Stack<'_>,
    addr: &str,
) -> heapless::Vec<IpAddr, { embassy_net::DNS_MAX_SERVER_COUNT }> {
    let mut addresses = heapless::Vec::<IpAddr, _>::new();
    match stack.dns_query(addr, embassy_net::dns::DnsQueryType::A).await {
        Ok(r) => {
            for a in r {
                let _ = addresses.push(a.into());
            }
        }
        Err(e) => warn!("Failed to resolve A for {}: {}", addr, e),
    }
    match stack.dns_query(addr, embassy_net::dns::DnsQueryType::Aaaa).await {
        Ok(r) => {
            for a in r {
                let _ = addresses.push(a.into());
            }
        }
        Err(e) => warn!("Failed to resolve AAAA for {}: {}", addr, e),
    }
    addresses
}
