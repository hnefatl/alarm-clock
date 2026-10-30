use anyhow::anyhow;
use core::net::SocketAddr;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use sntpc::NtpContext;
use sntpc_time_embassy::EmbassyTimestampGenerator;

/// Standard NTP port.
pub const PORT: u16 = 123;

pub async fn get_time(stack: embassy_net::Stack<'_>, addr: SocketAddr) -> anyhow::Result<sntpc::NtpResult> {
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
