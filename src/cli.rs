use crate::common::model::DatabaseType;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "backup", arg_required_else_help = true, about, version, author)]
pub struct Args {
  /// Backup data directory
  /// Backup data directory from docker when it starts with docker://
  /// e.g. docker://container_name:/path/to/data
  #[arg(long, env = "BACKUP_DATA_PATH", value_delimiter = ',', verbatim_doc_comment)]
  pub data_path: Option<Vec<String>>,

  /// Rclone remote
  #[arg(long, env = "RCLONE_REMOTE_NAME", value_delimiter = ',')]
  pub rclone_remote_name: Option<Vec<String>>,

  /// Rclone remote path
  #[arg(long, env = "RCLONE_REMOTE_PATH")]
  pub rclone_remote_path: Option<String>,

  /// Rclone binary path
  #[arg(long, env = "RCLONE_BIN_PATH", default_value = "rclone")]
  pub rclone_bin_path: Option<String>,

  /// Backup rotate
  #[arg(long, env = "BACKUP_ROTATE", default_value = "30")]
  pub rotate: usize,

  /// Database type
  #[arg(long, env = "DB_TYPE", value_enum)]
  pub db_type: Option<DatabaseType>,

  /// Database Container name
  #[arg(long, env = "DB_CONTAINER_NAME")]
  pub db_container_name: Option<String>,

  /// Ntfy basic url
  #[arg(long, env = "NTFY_BASE_URL")]
  pub ntfy_base_url: Option<String>,

  /// Ntfy username
  #[arg(long, env = "NTFY_USERNAME")]
  pub ntfy_username: Option<String>,

  /// Ntfy password
  #[arg(long, env = "NTFY_PASSWORD")]
  pub ntfy_password: Option<String>,

  /// Ntfy topic
  #[arg(long, env = "NTFY_TOPIC")]
  pub ntfy_topic: Option<String>,

  /// Dotenv file path
  #[arg(long)]
  pub env_file: Option<String>,

  /// Exclude files matching pattern
  #[arg(long)]
  pub exclude: Option<Vec<String>>,

  /// Enable debug log
  #[arg(long)]
  pub debug: bool,
}
