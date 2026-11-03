use serde;

#[derive(serde::Deserialize, Debug)]
pub struct ClientSecretsFile {
    // The field in JSON can be called either of these aliases.
    #[serde(alias = "installed")]
    #[serde(alias = "web")]
    pub inner: ClientSecrets,
}
#[derive(serde::Deserialize, Debug)]
pub struct ClientSecrets {
    pub client_id: String,
    pub client_secret: String,
}
