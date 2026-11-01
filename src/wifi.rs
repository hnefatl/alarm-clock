use alloc::string::String;
use defmt::{error, info};
use embassy_futures::select::select;
use embassy_net::{DhcpConfig, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::WIFI;
use esp_radio::wifi::{AuthenticationMethodConfig, ControllerConfig, Interface, WifiController, sta::StationConfig};
use static_cell::StaticCell;

static RADIO: StaticCell<WifiController<'static>> = StaticCell::new();
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
    let controller = RADIO.init(WifiController::new(wifi, controller_config).expect("failed to init wifi"));
    let stack_resources = STACK_RESOURCES.init(StackResources::new());
    let sta_interface = esp_radio::wifi::Interface::station();

    info!("Radio initialized");

    let rng = esp_hal::rng::Rng::new();

    let mut config = embassy_net::Config::default();
    config.ipv4 = embassy_net::ConfigV4::Dhcp(make_dhcp_config());
    config.ipv6 = embassy_net::ConfigV6::Slaac;

    let (stack, runner) = embassy_net::new(
        sta_interface,
        config,
        stack_resources,
        ((rng.random() as u64) << 32) | rng.random() as u64,
    );

    spawner.spawn(net_runner(runner).unwrap());
    spawner.spawn(maintain_connection(stack, controller).unwrap());
    return *STACK.init(stack);
}

#[embassy_executor::task]
async fn maintain_connection(
    stack: Stack<'static>,
    // TODO: pass by value?
    controller: &'static mut WifiController<'static>,
) {
    loop {
        match controller.connect_async().await {
            Ok(info) => info!("Wifi connected: {}", info),
            Err(e) => {
                error!("Failed to connect to wifi: {}", e);
                Timer::after(Duration::from_millis(5000)).await;
                continue;
            }
        }

        // Wait for disconnect while periodically fixing DNS.
        // TODO: See if the newer esp-radio version doesn't need DNS fix?
        // TODO: stack.wait_config_up()?
        while select(
            controller.wait_for_disconnect_async(),
            Timer::after(Duration::from_millis(10_000)),
        )
        .await
        .is_second()
        {
            fix_default_dhcp(stack);
        }
        Timer::after(Duration::from_millis(2000)).await
    }
}

#[embassy_executor::task]
async fn net_runner(mut runner: embassy_net::Runner<'static, Interface>) {
    runner.run().await
}

fn make_dhcp_config() -> DhcpConfig {
    let mut s = heapless::String::new();
    let _ = s.push_str(HOSTNAME);
    let mut d = DhcpConfig::default();
    d.hostname = Some(s);
    d
}

/// This is a hack: if the DHCP server doesn't provide an explicit DNS server,
/// the stack doesn't fallback to the gateway.
/// This function injects the gateway to fix that.
fn fix_default_dhcp(stack: Stack<'_>) {
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
}
