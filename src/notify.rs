use std::collections::HashMap;
use tracing::error;

pub async fn notify_by_ntfy(
  base_url: &String,
  username: &Option<String>,
  password: &Option<String>,
  token: &Option<String>,
  topic: &String,
  message: &String,
) -> anyhow::Result<()> {
  let client = reqwest::Client::new();
  let mut data = HashMap::new();
  let auth_username = username.as_deref().unwrap_or("");
  let auth_password = token.as_ref().or(password.as_ref());
  data.insert("topic", topic);
  data.insert("message", message);
  let response = client
    .post(base_url)
    .basic_auth(auth_username, auth_password)
    .json(&data)
    .send()
    .await?;
  if !response.status().is_success() {
    error!("failed to send notification, response: {}", response.text().await?);
  }
  Ok(())
}
