use alloc::string::{String, ToString};
use defmt::{error, info};
use embassy_futures::select::select;
use embassy_net::{DhcpConfig, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::WIFI;
use esp_radio::wifi::{AuthenticationMethodConfig, ControllerConfig, Interface, WifiController, sta::StationConfig};
use static_cell::StaticCell;

static STACK_RESOURCES: StaticCell<StackResources<10>> = StaticCell::new();
static STACK: StaticCell<Stack> = StaticCell::new();

const HOSTNAME: &str = "alarm-clock";

pub fn init(
    spawner: &embassy_executor::Spawner,
    wifi: WIFI<'static>,
    ssid: String,
    password: String,
) -> Stack<'static> {
    let ssid = ssid.try_into().expect("failed to parse SSID");
    let password = password.try_into().expect("failed to parse password");
    let sta_config = esp_radio::wifi::Config::Station(
        StationConfig::default()
            .with_ssid(ssid)
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(password)),
    );
    let controller_config = ControllerConfig::default()
        .with_country_info(*b"GB")
        .with_initial_config(sta_config);
    let controller = WifiController::new(wifi, controller_config).expect("failed to init wifi");
    let stack_resources = STACK_RESOURCES.init(StackResources::new());
    let sta_interface = esp_radio::wifi::Interface::station();

    info!("Radio initialized");

    let mut config = embassy_net::Config::default();
    config.ipv4 = embassy_net::ConfigV4::Dhcp(set_dhcp_hostname(DhcpConfig::default()));
    config.ipv6 = embassy_net::ConfigV6::Slaac;

    let rng = esp_hal::rng::Rng::new();
    let (stack, runner) = embassy_net::new(
        sta_interface,
        config,
        stack_resources,
        ((rng.random() as u64) << 32) | rng.random() as u64,
    );

    spawner.spawn(net_runner(runner).unwrap());
    spawner.spawn(maintain_connection(controller).unwrap());
    spawner.spawn(fix_dns(stack).unwrap());
    return *STACK.init(stack);
}

#[embassy_executor::task]
async fn maintain_connection(mut controller: WifiController<'static>) {
    loop {
        match controller.connect_async().await {
            Ok(info) => info!("Wifi connected: {}", info),
            Err(e) => {
                error!("Failed to connect to wifi: {}", e);
                Timer::after(Duration::from_millis(5000)).await;
                continue;
            }
        }

        let _ = controller.wait_for_disconnect_async().await;
        Timer::after(Duration::from_millis(2000)).await;
    }
}

#[embassy_executor::task]
async fn net_runner(mut runner: embassy_net::Runner<'static, Interface>) {
    runner.run().await
}

/// This is a hack: if the DHCP server doesn't provide an explicit DNS server,
/// the stack doesn't fallback to the gateway.
/// This function injects the gateway to fix that.
#[embassy_executor::task]
async fn fix_dns(stack: Stack<'static>) {
    let mut old_config_v4 = None;
    let mut old_config_v6 = None;
    loop {
        stack.wait_config_up().await;

        if let Some(s) = stack.config_v4()
            && s.dns_servers.is_empty()
            && let Some(gateway) = s.gateway
        {
            let mut s2 = s.clone();
            let _ = s2.dns_servers.push(gateway);
            stack.set_config_v4(embassy_net::ConfigV4::Static(s2));
        }
        if let Some(s) = stack.config_v6()
            && s.dns_servers.is_empty()
            && let Some(gateway) = s.gateway
        {
            let mut s2 = s.clone();
            let _ = s2.dns_servers.push(gateway);
            stack.set_config_v6(embassy_net::ConfigV6::Static(s2));
        }

        if stack.config_v4() != old_config_v4 || stack.config_v6() != old_config_v6 {
            old_config_v4 = stack.config_v4();
            old_config_v6 = stack.config_v6();
            print_ip_configuration(stack);
        }

        // Wait for the IP config to reset, or checks.
        // This is necessary because e.g. IPv6 comes up first, we fix the IPv6 DNS,
        // but then IPv4 comes up and we don't set its DNS.
        select(stack.wait_config_down(), Timer::after(Duration::from_millis(2000))).await;
    }
}

fn set_dhcp_hostname(mut config: DhcpConfig) -> DhcpConfig {
    let mut s = heapless::String::new();
    let _ = s.push_str(HOSTNAME);
    config.hostname = Some(s);
    config
}

fn print_ip_configuration(stack: Stack<'static>) {
    let mut cidrs = heapless::Vec::<String, 2>::new();
    if let Some(c) = stack.config_v4() {
        let _ = cidrs.push(c.address.to_string());
    }
    if let Some(c) = stack.config_v6() {
        let _ = cidrs.push(c.address.to_string());
    }
    info!("IPs: {}", cidrs);

    let mut dns = heapless::Vec::<String, 32>::new();
    if let Some(c) = stack.config_v4() {
        for d in c.dns_servers {
            let _ = dns.push(d.to_string());
        }
    }
    if let Some(c) = stack.config_v6() {
        for d in c.dns_servers {
            let _ = dns.push(d.to_string());
        }
    }
    info!("DNS: {}", dns);
}
