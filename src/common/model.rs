use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(ValueEnum, Copy, Clone, Debug, PartialEq, Eq)]
pub enum DatabaseType {
  Mysql,
  Postgres,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RcloneLs {
  #[serde(rename = "Path")]
  pub path: String,

  #[serde(rename = "Name")]
  pub name: String,

  #[serde(rename = "Size")]
  pub size: usize,

  #[serde(rename = "MimeType")]
  pub mime_type: String,

  #[serde(rename = "ModTime")]
  pub mod_time: String,

  #[serde(rename = "IsDir")]
  pub is_dir: bool,
}
