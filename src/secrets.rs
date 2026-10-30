// Secrets embedded into the image.
// The files have to have no trailing whitepace/newlines, `trim` is non-const :/

pub const WIFI_SSID: &'static str = include_str!("../secrets/wifi_ssid.txt");
pub const WIFI_PASSWORD: &'static str = include_str!("../secrets/wifi_password.txt");
