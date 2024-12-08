use crate::common::model::DatabaseType;
use anyhow::bail;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "backup", arg_required_else_help = true, about, version, author)]
pub struct Args {
  /// Backup data directory
  /// Backup data directory from docker when it starts with docker://
  /// e.g. docker://container_name:/path/to/data
  #[arg(long, env = "BACKUP_DATA_PATH", value_delimiter = ',', verbatim_doc_comment)]
  pub data_path: Option<Vec<String>>,

  /// Database arguments
  #[clap(flatten)]
  pub database_args: DatabaseArgs,

  /// Exclude files matching pattern
  #[arg(long)]
  pub exclude: Option<Vec<String>>,

  /// Backup rotate
  #[arg(long, env = "BACKUP_ROTATE", default_value = "30")]
  pub rotate: usize,

  /// Dotenv file path
  #[arg(long)]
  pub env_file: Option<String>,

  /// Rclone arguments
  #[clap(flatten)]
  pub rclone_args: RcloneArgs,

  /// Ntfy arguments
  #[clap(flatten)]
  pub ntfy_args: NtfyArgs,

  /// Enable debug log
  #[arg(long)]
  pub debug: bool,
}

#[derive(Parser, Debug)]
pub struct DatabaseArgs {
  /// Database type
  #[arg(long, env = "DB_TYPE", value_enum)]
  pub db_type: Option<DatabaseType>,

  /// Database Container name
  #[arg(long, env = "DB_CONTAINER_NAME")]
  pub db_container_name: Option<String>,
}

#[derive(Parser, Debug)]
pub struct RcloneArgs {
  /// Rclone remote
  #[arg(long, env = "RCLONE_REMOTE_NAME", value_delimiter = ',')]
  pub rclone_remote_name: Option<Vec<String>>,

  /// Rclone remote path
  #[arg(long, env = "RCLONE_REMOTE_PATH")]
  pub rclone_remote_path: Option<String>,

  /// Rclone binary path
  #[arg(long, env = "RCLONE_BIN_PATH", default_value = "rclone")]
  pub rclone_bin_path: Option<String>,
}

#[derive(Parser, Debug)]
pub struct NtfyArgs {
  /// Ntfy basic url
  #[arg(long, env = "NTFY_BASE_URL")]
  pub ntfy_base_url: Option<String>,

  /// Ntfy username
  #[arg(long, env = "NTFY_USERNAME")]
  pub ntfy_username: Option<String>,

  /// Ntfy password
  #[arg(long, env = "NTFY_PASSWORD")]
  pub ntfy_password: Option<String>,

  /// Ntfy token
  #[arg(long, env = "NTFY_TOKEN")]
  pub ntfy_token: Option<String>,

  /// Ntfy topic
  #[arg(long, env = "NTFY_TOPIC")]
  pub ntfy_topic: Option<String>,
}

impl Args {
  pub fn check_valid(&self) -> anyhow::Result<()> {
    let data_path = self.data_path.as_deref().unwrap_or(&[]);
    if data_path.is_empty() {
      bail!("The backup data path is required");
    }
    if data_path.iter().any(|s| s.is_empty()) {
      bail!("The backup data path can not be empty");
    }

    let exclude = self.exclude.as_deref().unwrap_or(&[]);
    if exclude.iter().any(|s| s.is_empty()) {
      bail!("The exclude pattern can not be empty");
    }

    self.database_args.check_valid()?;
    self.rclone_args.check_valid()?;
    self.ntfy_args.check_valid()?;

    Ok(())
  }
}

impl DatabaseArgs {
  pub fn check_valid(&self) -> anyhow::Result<()> {
    let container_name = self.db_container_name.as_deref().unwrap_or("");
    if self.db_type.is_none() && container_name.is_empty() {
      return Ok(());
    }
    if self.db_type.is_none() && !container_name.is_empty() {
      bail!("The database type is required");
    }
    if self.db_type.is_some() && container_name.is_empty() {
      bail!("The database container name is required");
    }
    Ok(())
  }

  pub fn has_args(&self) -> bool {
    let container_name = self.db_container_name.as_deref().unwrap_or("");
    self.db_type.is_some() && !container_name.is_empty()
  }
}

impl RcloneArgs {
  pub fn check_valid(&self) -> anyhow::Result<()> {
    let remote_name = self.rclone_remote_name.as_deref().unwrap_or(&[]);
    if remote_name.is_empty() {
      bail!("The rclone remote name is required");
    }

    if remote_name.iter().any(|s| s.is_empty()) {
      bail!("The rclone remote name can not be empty");
    }

    let remote_path = self.rclone_remote_path.as_deref().unwrap_or("");
    if remote_path.is_empty() {
      bail!("The rclone remote path is required");
    }
    if remote_path == "/" {
      bail!("The rclone remote path can not equals to /");
    }

    let bin_path = self.rclone_bin_path.as_deref().unwrap_or("");
    if bin_path.is_empty() {
      bail!("The rclone binary path is required");
    }

    Ok(())
  }
}

impl NtfyArgs {
  pub fn check_valid(&self) -> anyhow::Result<()> {
    let base_url = self.ntfy_base_url.as_deref().unwrap_or("");
    if !base_url.is_empty() {
      return Ok(());
    }

    let username = self.ntfy_username.as_deref().unwrap_or("");
    let password = self.ntfy_password.as_deref().unwrap_or("");
    let token = self.ntfy_token.as_deref().unwrap_or("");
    if token.is_empty() && (username.is_empty() || password.is_empty()) {
      bail!("The ntfy credentials are required");
    }

    let topic = self.ntfy_topic.as_deref().unwrap_or("");
    if topic.is_empty() {
      bail!("The ntfy topic is required");
    }

    Ok(())
  }

  pub fn has_args(&self) -> bool {
    self.ntfy_base_url.is_some()
  }
}
