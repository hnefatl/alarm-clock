use alloc::{
    format,
    string::{String, ToString},
};
use embassy_net::{
    Stack,
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use embassy_time::{Duration, with_timeout};
use reqwless::{
    client::TlsConfig,
    request::{Method, RequestBuilder},
};
use time::{OffsetDateTime, UtcDateTime};

use crate::secrets::Secrets;

type HttpClient<'a> = reqwless::client::HttpClient<'a, TcpClient<'a, 1, 4096, 4096>, DnsSocket<'a>>;

pub async fn get_next_wakeup_time(
    stack: Stack<'_>,
    start_time: UtcDateTime,
    secrets: &Secrets,
) -> Result<UtcDateTime, Error> {
    let rng = esp_hal::rng::Rng::new();
    let seed = ((rng.random() as u64) << 32) | rng.random() as u64;

    let mut tls_rx_buffer: [u8; _] = [0; 20000];
    let mut tls_tx_buffer: [u8; _] = [0; 2048];
    // TODO: TLS verify?
    let tls_config = TlsConfig::new(
        seed,
        &mut tls_rx_buffer,
        &mut tls_tx_buffer,
        reqwless::client::TlsVerify::None,
    );
    let tcp_state = TcpClientState::new();
    let mut tcp_client = TcpClient::new(stack, &tcp_state);
    tcp_client.set_timeout(Some(Duration::from_secs(5)));
    let dns_socket = DnsSocket::new(stack);

    let mut client = HttpClient::new_with_tls(&tcp_client, &dns_socket, tls_config);

    let t = with_timeout(Duration::from_secs(10), get_authorisation_token(&mut client, &secrets)).await??;
    let events = with_timeout(
        Duration::from_secs(10),
        get_calendar_events(&mut client, start_time, secrets.calendar_id, &t.access_token),
    )
    .await??;
    let &[next_event, ..] = &events.items.as_slice() else {
        return Err(Error("No events returned".into()));
    };
    Ok(next_event.start.date_time.to_utc())
}

async fn get_authorisation_token(client: &mut HttpClient<'_>, secrets: &Secrets) -> Result<TokenResponse, Error> {
    let mut rx_buffer = [0u8; 2048];
    let headers = [("Content-Type", "application/x-www-form-urlencoded")];
    let mut req_body = String::new();
    form_urlencoded::Serializer::new(&mut req_body)
        .append_pair("client_id", secrets.calendar_secrets.client_id)
        .append_pair("client_secret", secrets.calendar_secrets.client_secret)
        .append_pair("refresh_token", secrets.calendar_refresh_token)
        .append_pair("grant_type", "refresh_token")
        .finish();

    let req_builder = client
        .request(Method::POST, "https://oauth2.googleapis.com/token")
        .await?;

    let mut req = req_builder.headers(&headers).body(req_body.as_bytes());

    let resp_body = req.send(&mut rx_buffer).await?.body().read_to_end().await?;

    match serde_json_core::from_slice(&resp_body) {
        Ok((resp, _)) => Ok(resp),
        Err(e) => Err(Error::from_serde(&resp_body, e)),
    }
}

async fn get_calendar_events(
    client: &mut HttpClient<'_>,
    start_time: UtcDateTime,
    calendar_id: &str,
    access_token: &str,
) -> Result<ListEventsResponse, Error> {
    let mut rx_buffer = [0u8; 8192];
    let auth_header = format!("Bearer {}", access_token);
    let headers = [("Authorization", auth_header.as_ref())];

    // Earliest end time to fetch (urlencoded).
    let time_min = format!(
        "{:04}-{:02}-{:02}T{:02}%3A{:02}%3A{:02}Z",
        start_time.year(),
        start_time.month() as u8,
        start_time.day(),
        start_time.hour(),
        start_time.minute(),
        start_time.second(),
    );
    // GET url: request a handful of results after the given start time, in the
    // Europe/London timezone (which is BST-aware).
    // The returned result should be in ISO-8601 so have an explicit timezone
    // offset, which'll be correctly translated into UTC before returned out of this module.
    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/\
        {}%40group.calendar.google.com/events\
        ?eventTypes=default\
        &timeZone=Europe%2FLondon\
        &timeMin={}\
        &maxResults={}\
        &orderBy=startTime&\
        q=Wakeup&\
        singleEvents=true",
        calendar_id,
        time_min,
        MAX_RESULTS
    );
    let request = client.request(Method::GET, &url).await?;

    let resp_body = request
        .headers(&headers)
        .send(&mut rx_buffer)
        .await?
        .body()
        .read_to_end()
        .await?;

    match serde_json_core::from_slice(&resp_body) {
        Ok((resp, _)) => Ok(resp),
        Err(e) => Err(Error::from_serde(&resp_body, e)),
    }
}

#[derive(serde::Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
}

const MAX_RESULTS: usize = 2;
/// https://developers.google.com/workspace/calendar/api/v3/reference/events/list#response
#[derive(serde::Deserialize, Debug)]
struct ListEventsResponse {
    items: heapless::Vec<Event, { MAX_RESULTS }>,
}

#[derive(serde::Deserialize, Debug)]
struct Event {
    start: EventTime,
}

#[derive(serde::Deserialize, Debug)]
struct EventTime {
    #[serde(rename = "dateTime", deserialize_with="time::serde::iso8601::deserialize")]
    date_time: OffsetDateTime,
}

#[derive(Debug)]
pub struct Error(String);
impl Error {
    fn from_serde(input: &[u8], error: serde_json_core::de::Error) -> Self {
        let input_str = match str::from_utf8(input) {
            Ok(s) => s,
            Err(e) => &e.to_string(),
        };
        Self(format!("Failed to deserialize ({}): {}", error, input_str))
    }
}
impl core::error::Error for Error {}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}
impl defmt::Format for Error {
    fn format(&self, fmt: defmt::Formatter) {
        self.0.format(fmt)
    }
}
impl From<embassy_time::TimeoutError> for Error {
    fn from(_: embassy_time::TimeoutError) -> Self {
        Self("timed out".into())
    }
}
impl From<reqwless::Error> for Error {
    fn from(value: reqwless::Error) -> Self {
        Self(value.to_string())
    }
}
impl From<serde_json_core::de::Error> for Error {
    fn from(value: serde_json_core::de::Error) -> Self {
        Self(value.to_string())
    }
}
