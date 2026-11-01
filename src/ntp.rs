use anyhow::anyhow;
use core::default::Default;
use core::net::SocketAddr;
use defmt::{Debug2Format, error};
use embassy_executor::Spawner;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_sync::mutex::Mutex;
use embassy_sync::once_lock::OnceLock;
use embassy_time::Timer;
use esp_hal::rtc_cntl::Rtc;
use esp_sync::RawMutex;
use sntpc::{NtpContext, NtpResult};
use sntpc_time_embassy::EmbassyTimestampGenerator;
use time::UtcDateTime;

/// Standard NTP port.
const PORT: u16 = 123;
const NTP_POLL_INTERVAL: embassy_time::Duration = embassy_time::Duration::from_secs(60);
const NTP_SERVER: &'static str = "time.google.com.";

#[derive(Debug, Copy, Clone)]
struct NtpState {
    /// The NTP result at the time converted to UTC.
    utc: UtcDateTime,
    /// The time since boot when the NTP result was fetched.
    fetched_time: esp_hal::time::Duration,
}

static LAST_STATE: OnceLock<Mutex<RawMutex, NtpState>> = OnceLock::new();

pub fn start(spawner: Spawner, rtc: &'static Rtc<'static>, stack: embassy_net::Stack<'static>) {
    spawner.spawn(refresh_time(rtc, stack).expect("failed to init ntp"));
}

/// Get the latest approximate wall-clock time, or wait for it to become available.
pub async fn get_time(rtc: &Rtc<'_>) -> UtcDateTime {
    let t = LAST_STATE.get().await.lock().await;
    let now = rtc.time_since_power_up();
    // TODO: use offset?
    return t.utc + time::Duration::microseconds((now - t.fetched_time).as_micros() as i64);
}

#[embassy_executor::task]
async fn refresh_time(rtc: &'static Rtc<'static>, stack: embassy_net::Stack<'static>) {
    loop {
        let Ok(ntp_result) = crate::dns::run_against_dns(stack, NTP_SERVER, async |addr| {
            get_ntp(stack, SocketAddr::new(addr, PORT)).await
        })
        .await
        else {
            error!("get time error");
            Timer::after_secs(5).await;
            continue;
        };
        let fetched_time = rtc.time_since_power_up();
        let utc = match ntp_to_utc(ntp_result) {
            Ok(u) => u,
            Err(e) => {
                error!(
                    "Invalid NTP received ({}): {}",
                    Debug2Format(&e),
                    Debug2Format(&ntp_result)
                );
                continue;
            }
        };
        let new_state = NtpState { utc, fetched_time };
        *LAST_STATE.get_or_init(|| Mutex::new(new_state)).lock().await = new_state;

        Timer::after(NTP_POLL_INTERVAL).await;
    }
}

async fn get_ntp(stack: embassy_net::Stack<'_>, addr: SocketAddr) -> anyhow::Result<NtpResult> {
    let mut rx_meta: [PacketMetadata; 10] = [PacketMetadata::EMPTY; 10];
    let mut rx_buffer: [u8; 200] = [0; 200];
    let mut tx_meta: [PacketMetadata; 10] = [PacketMetadata::EMPTY; 10];
    let mut tx_buffer: [u8; 200] = [0; 200];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buffer, &mut tx_meta, &mut tx_buffer);
    socket.bind(0).map_err(|e| anyhow!("bind error: {:?}", e))?;
    let socket = sntpc_net_embassy::UdpSocketWrapper::from(socket);
    let ntp_context = NtpContext::new(EmbassyTimestampGenerator::default());

    let result = sntpc::get_time(addr, &socket, ntp_context)
        .await
        .map_err(|e| anyhow!("get_time error: {:?}", e))?;
    Ok(result)
}

fn ntp_to_utc(result: NtpResult) -> Result<UtcDateTime, time::error::ComponentRange> {
    let secs = result.sec() as i64;
    let nanos = ((result.sec_fraction() as u64 * 1_000_000_000) >> 32) as u32;
    let total_nanos = (secs as i128 * 1_000_000_000) + nanos as i128;
    UtcDateTime::from_unix_timestamp_nanos(total_nanos)
}
