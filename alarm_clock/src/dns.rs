use alloc::string::{String, ToString};
use core::net::IpAddr;
use heapless::index_map::FnvIndexMap;

use defmt::{error, warn};

pub struct DNSResolver<'a, const N: usize> {
    stack: embassy_net::Stack<'a>,
    cache: FnvIndexMap<String, heapless::Vec<IpAddr, { embassy_net::DNS_MAX_SERVER_COUNT }>, N>,
}

impl<'a, const N: usize> DNSResolver<'a, N> {
    pub fn new(stack: embassy_net::Stack<'a>) -> Self {
        Self {
            stack,
            cache: FnvIndexMap::new(),
        }
    }

    /// Run a callback against multiple DNS-resolved addresses until one succeeds.
    pub async fn run_against_dns<T, E, F: AsyncFn(IpAddr) -> Result<T, E>>(
        &mut self,
        addr: &str,
        op: F,
    ) -> Result<T, ()> {
        for addr in self.query(addr).await {
            if let Ok(r) = op(addr).await {
                return Ok(r);
            }
        }
        Err(())
    }

    /// Do a DNS query, falling back to cache if it fails.
    /// That's the opposite to a normal DNS client, but I'm prioritising availability
    /// over reducing traffic.
    pub async fn query(&mut self, addr: &str) -> heapless::Vec<IpAddr, { embassy_net::DNS_MAX_SERVER_COUNT }> {
        let results = self.inner_query(addr).await;
        if !results.is_empty() {
            if self.cache.is_full() && !self.cache.contains_key(addr) {
                warn!("DNS cache full, overwriting with {}", addr);
            }
            let _ = self.cache.insert(addr.to_string(), results.clone());
            results
        } else if let Some(results) = self.cache.get(addr) {
            warn!("Fell back to DNS cache for {}", addr);
            results.clone()
        } else {
            error!("DNS cache miss for {}", addr);
            heapless::Vec::new()
        }
    }

    async fn inner_query(&self, addr: &str) -> heapless::Vec<IpAddr, { embassy_net::DNS_MAX_SERVER_COUNT }> {
        let mut addresses = heapless::Vec::<IpAddr, _>::new();

        match self.stack.dns_query(addr, embassy_net::dns::DnsQueryType::A).await {
            Ok(r) => {
                for a in r {
                    let _ = addresses.push(a.into());
                }
            }
            Err(e) => warn!("Failed to resolve A for {}: {}", addr, e),
        }
        match self.stack.dns_query(addr, embassy_net::dns::DnsQueryType::Aaaa).await {
            Ok(r) => {
                for a in r {
                    let _ = addresses.push(a.into());
                }
            }
            Err(e) => warn!("Failed to resolve AAAA for {}: {}", addr, e),
        }
        addresses
    }
}
