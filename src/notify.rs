use anyhow::bail;
use std::collections::HashMap;

pub async fn notify_by_nty(
  base_url: &String,
  username: &String,
  password: &String,
  topic: &String,
  message: &String,
) -> anyhow::Result<()> {
  let mut data = HashMap::new();
  data.insert("topic", topic);
  data.insert("message", message);
  let client = reqwest::Client::new();
  let response = client
    .post(base_url)
    .basic_auth(username, Some(password))
    .json(&data)
    .send()
    .await?;
  if !response.status().is_success() {
    bail!("failed to send notification, response: {}", response.text().await?);
  }
  Ok(())
}
