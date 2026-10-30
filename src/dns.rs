use core::net::IpAddr;

use defmt::warn;

pub async fn run_against_dns<T, E, F: AsyncFn(IpAddr) -> Result<T, E>>(
    stack: embassy_net::Stack<'_>,
    addr: &str,
    op: F,
) -> Result<T, ()> {
    for addr in dns_query(stack, addr).await {
        if let Ok(r) = op(addr).await {
            return Ok(r);
        }
    }
    Err(())
}

pub async fn dns_query(
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
