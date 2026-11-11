/// If the Google Calendar refresh token is missing at compile time, do a little dance to
/// guide the process of creating it for embedding into the firmware.
use anyhow::Context;
use reqwest::{Url, blocking::Client};
use std::{
    fs::{self, Permissions},
    os::unix::fs::PermissionsExt,
    path::Path,
    thread::sleep,
    time::Duration,
};

const SECRET_PATH: &str = "../secrets/calendar_secrets.json";
const REFRESH_TOKEN_PATH: &str = "../secrets/calendar_refresh_token.txt";

fn main() -> anyhow::Result<()> {
    let refresh_token_path = Path::new(REFRESH_TOKEN_PATH);
    if refresh_token_path.exists() {
        println!("Refresh token already exists at {}", REFRESH_TOKEN_PATH);
        return Ok(());
    }

    let secrets_data = fs::read(SECRET_PATH).context(format!("failed to read {}", SECRET_PATH))?;
    let secrets: lib::ClientSecretsFile = serde_json::from_slice(&secrets_data)?;

    let client = Client::new();
    let resp = send_device_code_request(&client, &secrets.inner)?;

    println!(
        "Go to {} and enter {} to authorise...",
        resp.verification_url, resp.user_code
    );
    let token_resp = loop {
        match send_token_request(&client, &secrets.inner, &resp.device_code)? {
            TokenResponse::Good(t) => break t,
            TokenResponse::Failure(f) if f.error != "authorization_pending" => println!("{}", f.error_description),
            _ => {}
        }
        sleep(Duration::from_secs(resp.interval));
    };

    println!("Refresh token is: {}", token_resp.refresh_token);
    fs::write(refresh_token_path, &token_resp.refresh_token).context("writing refresh token")?;
    fs::set_permissions(refresh_token_path, Permissions::from_mode(0o700))
        .context("changing refresh token file permissions")?;
    return Ok(());
}

fn send_device_code_request(client: &Client, secrets: &lib::ClientSecrets) -> anyhow::Result<DeviceCodeResponse> {
    let params = [
        ("client_id", secrets.client_id.as_ref()),
        ("scope", "https://www.googleapis.com/auth/calendar.readonly"),
    ];
    let resp = client
        .post("https://oauth2.googleapis.com/device/code")
        .form(&params)
        .send()
        .context("when sending request")?
        .json::<DeviceCodeResponse>()
        .context("when deserialising response")?;
    Ok(resp)
}

fn send_token_request(
    client: &Client,
    secrets: &lib::ClientSecrets,
    device_code: &str,
) -> anyhow::Result<TokenResponse> {
    let params = [
        ("client_id", secrets.client_id.as_ref()),
        ("client_secret", secrets.client_secret.as_ref()),
        ("device_code", device_code),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
    ];
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .context("when sending request")?
        .json::<TokenResponse>()
        .context("when deserialising response")?;
    Ok(resp)
}

#[derive(serde::Deserialize, Debug)]
struct DeviceCodeResponse {
    user_code: String,
    device_code: String,
    interval: u64,
    verification_url: Url,
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum TokenResponse {
    Good(Token),
    Failure(TokenFailure),
}
#[derive(serde::Deserialize, Debug)]
struct Token {
    refresh_token: String,
}
#[derive(serde::Deserialize, Debug)]
struct TokenFailure {
    error: String,
    error_description: String,
}
