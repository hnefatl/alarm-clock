// Secrets embedded into the image.

const WIFI_SSID: &'static str = include_str!("../../secrets/wifi_ssid.txt");
const WIFI_PASSWORD: &'static str = include_str!("../../secrets/wifi_password.txt");
const CALENDAR_SECRETS: &'static str = include_str!("../../secrets/calendar_secrets.json");
const CALENDAR_REFRESH_TOKEN: &'static str = include_str!("../../secrets/calendar_refresh_token.txt");

#[derive(Debug)]
pub struct Secrets {
    pub wifi_ssid: &'static str,
    pub wifi_password: &'static str,
    pub calendar_secrets: lib::ClientSecrets<'static>,
    pub calendar_refresh_token: &'static str,
}

pub fn init() -> Secrets {
    let calendar_secrets_file = serde_json_core::from_str::<lib::ClientSecretsFile>(CALENDAR_SECRETS)
        .expect("failed to parse calendar secrets file");
    Secrets {
        wifi_ssid: WIFI_SSID.trim(),
        wifi_password: WIFI_PASSWORD.trim(),
        calendar_secrets: calendar_secrets_file.0.inner,
        calendar_refresh_token: CALENDAR_REFRESH_TOKEN.trim(),
    }
}
