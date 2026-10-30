use alloc::string::String;
use defmt::{error, info};
use embassy_futures::select::select;
use embassy_net::{DhcpConfig, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::WIFI;
use esp_radio::Controller;
use esp_radio::wifi::{ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent, WifiStaState};
use static_cell::StaticCell;

static RADIO: StaticCell<Controller<'static>> = StaticCell::new();
static STACK_RESOURCES: StaticCell<StackResources<10>> = StaticCell::new();
static STACK: StaticCell<Stack> = StaticCell::new();

const HOSTNAME: &str = "alarm-clock";

pub fn init(
    spawner: &embassy_executor::Spawner,
    wifi: WIFI<'static>,
    ssid: String,
    password: String,
) -> Stack<'static> {
    let radio = RADIO.init(esp_radio::init().expect("failed to init radio"));
    let stack_resources = STACK_RESOURCES.init(StackResources::new());

    let wifi_config = esp_radio::wifi::Config::default().with_country_code(*b"GB");
    let (controller, interfaces) = esp_radio::wifi::new(radio, wifi, wifi_config).expect("failed to init wifi");
    info!("Radio initialized");

    let rng = esp_hal::rng::Rng::new();

    let mut config = embassy_net::Config::default();
    config.ipv4 = embassy_net::ConfigV4::Dhcp(make_dhcp_config());
    config.ipv6 = embassy_net::ConfigV6::Slaac;

    let (stack, runner) = embassy_net::new(
        interfaces.sta,
        config,
        stack_resources,
        ((rng.random() as u64) << 32) | rng.random() as u64,
    );

    spawner.must_spawn(net_runner(runner));
    spawner.must_spawn(maintain_connection(stack, controller, ssid, password));
    return *STACK.init(stack);
}

#[embassy_executor::task]
async fn maintain_connection(
    stack: Stack<'static>,
    mut controller: WifiController<'static>,
    ssid: String,
    password: String,
) {
    loop {
        if esp_radio::wifi::sta_state() == WifiStaState::Connected {
            info!("Waiting until disconnected...");
            // Run periodically and fix missing default DNS servers.
            while select(
                controller.wait_for_event(WifiEvent::StaDisconnected),
                Timer::after(Duration::from_millis(10_000)),
            )
            .await
            .is_second()
            {
                fix_default_dhcp(stack);
            }
            Timer::after(Duration::from_millis(2000)).await
        }
        match controller.is_started() {
            Ok(true) => {}
            Ok(false) => {
                let client_config = ModeConfig::Client(
                    ClientConfig::default()
                        .with_ssid(ssid.clone())
                        .with_password(password.clone()),
                );
                controller.set_config(&client_config).expect("failed to set config");
                info!("Starting wifi");
                controller.start_async().await.expect("failed to start wifi");
                info!("Wifi started!");
            }
            Err(e) => error!("Wifi error: {}", e),
        }

        info!("Connecting...");
        match controller.connect_async().await {
            Ok(()) => info!("Wifi connected!"),
            Err(e) => {
                error!("Failed to connect to wifi: {}", e);
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_runner(mut runner: embassy_net::Runner<'static, WifiDevice<'static>>) {
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
