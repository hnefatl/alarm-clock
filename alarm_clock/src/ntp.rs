use anyhow::{anyhow, bail};
use core::default::Default;
use core::net::SocketAddr;
use defmt::Display2Format;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_sync::mutex::Mutex;
use esp_hal::rtc_cntl::Rtc;
use esp_sync::RawMutex;
use sntpc::{NtpContext, NtpResult};
use sntpc_time_embassy::EmbassyTimestampGenerator;
use time::UtcDateTime;

/// Standard NTP port.
const PORT: u16 = 123;
const NTP_SERVER: &'static str = "time.google.com.";

#[derive(Debug, Copy, Clone)]
struct NtpState {
    /// The NTP result at the time converted to UTC.
    utc: UtcDateTime,
    /// The time since boot when the NTP result was fetched.
    fetched_time: esp_hal::time::Duration,
}
impl NtpState {
    /// Approximate the time based on how long it's been since we got an NTP update
    fn time_now(&self, rtc: &Rtc<'_>) -> UtcDateTime {
        let now = rtc.time_since_power_up();
        self.utc + time::Duration::microseconds((now - self.fetched_time).as_micros() as i64)
    }
}

pub struct NTP<'s> {
    stack: embassy_net::Stack<'s>,
    dns_resolver: crate::dns::DNSResolver<'s, 2>,
    last_state: Mutex<RawMutex, Option<NtpState>>,
}
impl<'s> NTP<'s> {
    pub fn new(stack: embassy_net::Stack<'s>) -> Self {
        Self {
            stack,
            dns_resolver: crate::dns::DNSResolver::new(stack),
            last_state: Mutex::new(None),
        }
    }

    /// Get the latest approximate wall-clock time.
    pub async fn get_utc(&mut self, rtc: &Rtc<'_>) -> anyhow::Result<UtcDateTime> {
        let ntp_state = match self
            .dns_resolver
            .run_against_dns(NTP_SERVER, async |addr| {
                get_ntp(self.stack, SocketAddr::new(addr, PORT)).await
            })
            .await
            .and_then(ntp_to_utc)
        {
            Ok(result) => {
                let fetched_time = rtc.time_since_power_up();
                let state = NtpState {
                    utc: result,
                    fetched_time,
                };
                *self.last_state.lock().await = Some(state);
                state
            }
            Err(e) => match *self.last_state.lock().await {
                Some(s) => s,
                None => bail!("failed to get latest NTP state: {}", Display2Format(&e)),
            },
        };

        Ok(ntp_state.time_now(rtc))
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

fn ntp_to_utc(result: NtpResult) -> anyhow::Result<UtcDateTime> {
    let mut nanos = result.sec() as i128 * 1_000_000_000;
    nanos += (result.sec_fraction() as i128 * 1_000_000_000) / (u32::MAX as i128);
    // TODO: does this need to use offset?
    Ok(UtcDateTime::from_unix_timestamp_nanos(nanos)?)
}
