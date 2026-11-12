use alloc::string::{String, ToString};
use anyhow::bail;
use core::net::IpAddr;
use heapless::index_map::FnvIndexMap;

use defmt::{error, warn};

/// A resolver with a cache of `N` hostname->IP sets.
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
    pub async fn run_against_dns<T, E: core::fmt::Display, F: AsyncFn(IpAddr) -> Result<T, E>>(
        &mut self,
        addr: &str,
        op: F,
    ) -> anyhow::Result<T> {
        let mut last_error = None;
        for addr in self.query(addr).await {
            match op(addr).await {
                Ok(r) => return Ok(r),
                Err(e) => last_error = Some(e),
            }
        }
        match last_error {
            Some(e) => bail!("no resolved IPs for {} succeeded, last error: {}", addr, e),
            None => bail!("failed to resolve any IPs for {}", addr),
        }
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
            error!("DNS failure for {}", addr);
            heapless::Vec::new()
        }
    }

    async fn inner_query(&self, addr: &str) -> heapless::Vec<IpAddr, { embassy_net::DNS_MAX_SERVER_COUNT }> {
        let mut addresses = heapless::Vec::<IpAddr, _>::new();

        // TODO: timeout
        match self.stack.dns_query(addr, embassy_net::dns::DnsQueryType::A).await {
            Ok(r) => {
                for a in r {
                    let _ = addresses.push(a.into());
                }
            }
            Err(e) => warn!("Failed to resolve A for {}: {}", addr, e),
        }
        addresses
    }
}
