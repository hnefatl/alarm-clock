#![no_std]

use serde;

#[derive(serde::Deserialize, Debug)]
pub struct ClientSecretsFile<'a> {
    // The field in JSON can be called either of these aliases.
    #[serde(alias = "installed")]
    #[serde(alias = "web")]
    #[serde(borrow)]
    pub inner: ClientSecrets<'a>,
}
#[derive(serde::Deserialize, Debug)]
pub struct ClientSecrets<'a> {
    #[serde(borrow)]
    pub client_id: &'a str,
    #[serde(borrow)]
    pub client_secret: &'a str,
}
