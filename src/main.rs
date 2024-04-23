mod notify;

use anyhow::bail;
use chrono::Local;
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::sync::Arc;
use std::{env, fs};
use tempfile::TempDir;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "backup", arg_required_else_help = true, about, version, author)]
struct Args {
  /// Backup data directory
  /// Backup data directory from docker when it starts with docker://
  /// e.g. docker://container_name:/path/to/data
  #[arg(long, env = "BACKUP_DATA_PATH", value_delimiter = ',', verbatim_doc_comment)]
  data_path: Option<Vec<String>>,

  /// Rclone remote
  #[arg(long, env = "RCLONE_REMOTE_NAME", value_delimiter = ',')]
  rclone_remote_name: Option<Vec<String>>,

  /// Rclone remote path
  #[arg(long, env = "RCLONE_REMOTE_PATH")]
  rclone_remote_path: Option<String>,

  /// Rclone binary path
  #[arg(long, env = "RCLONE_BIN_PATH", default_value = "rclone")]
  rclone_bin_path: Option<String>,

  /// Backup rotate
  #[arg(long, env = "BACKUP_ROTATE", default_value = "30")]
  rotate: usize,

  /// Database type
  #[arg(long, env = "DB_TYPE", value_enum)]
  db_type: Option<DatabaseType>,

  /// Database Container name
  #[arg(long, env = "DB_CONTAINER_NAME")]
  db_container_name: Option<String>,

  /// Ntfy basic url
  #[arg(long, env = "NTFY_BASE_URL")]
  ntfy_base_url: Option<String>,

  /// Ntfy username
  #[arg(long, env = "NTFY_USERNAME")]
  ntfy_username: Option<String>,

  /// Ntfy password
  #[arg(long, env = "NTFY_PASSWORD")]
  ntfy_password: Option<String>,

  /// Ntfy topic
  #[arg(long, env = "NTFY_TOPIC")]
  ntfy_topic: Option<String>,

  /// Dotenv file path
  #[arg(long)]
  env_file: Option<String>,

  /// Exclude files matching pattern
  #[arg(long)]
  exclude: Option<Vec<String>>,

  /// Enable debug log
  #[arg(long)]
  debug: bool,
}

#[derive(ValueEnum, Copy, Clone, Debug, PartialEq, Eq)]
enum DatabaseType {
  Mysql,
  Postgres,
}

#[derive(Debug, Serialize, Deserialize)]
struct RcloneLs {
  #[serde(rename = "Path")]
  path: String,

  #[serde(rename = "Name")]
  name: String,

  #[serde(rename = "Size")]
  size: usize,

  #[serde(rename = "MimeType")]
  mime_type: String,

  #[serde(rename = "ModTime")]
  mod_time: String,

  #[serde(rename = "IsDir")]
  is_dir: bool,
}

fn copy_files(src: &Vec<String>, dest: &String) -> anyhow::Result<()> {
  let output = std::process::Command::new("cp")
    .arg("-a")
    .args(src)
    .arg(dest)
    .output()?;
  if !output.status.success() {
    bail!(
      "failed to copy source data, error: {}",
      String::from_utf8(output.stderr)?
    );
  }
  Ok(())
}

fn copy_files_by_docker(src: &String, dest: &String) -> anyhow::Result<()> {
  let output = std::process::Command::new("docker")
    .arg("cp")
    .arg(src)
    .arg(dest)
    .arg("-q")
    .output()?;
  if !output.status.success() {
    bail!(
      "failed to copy source data by docker, error: {}",
      String::from_utf8(output.stderr)?
    );
  }
  Ok(())
}

fn dump_db_by_docker(db_dump_path: &String, container_name: &String, db_type: &DatabaseType) -> anyhow::Result<()> {
  let db_dump_cmd = match db_type {
    DatabaseType::Mysql => {
      "exec mysqldump -u$MYSQL_USER -p$MYSQL_PASSWORD --databases $MYSQL_DATABASE --no-tablespaces"
    }
    DatabaseType::Postgres => "pg_dump postgresql://$PG_USER:$PG_PASSWORD@127.0.0.1/$PG_DATABASE --clean",
  };
  let output = std::process::Command::new("docker")
    .arg("exec")
    .arg("-i")
    .arg(container_name)
    .arg("/bin/sh")
    .arg("-c")
    .arg(db_dump_cmd)
    .output()
    .expect("failed to dump database");
  if output.status.success() {
    fs::write(db_dump_path, &output.stdout).expect("failed to write database backup data to file")
  } else {
    bail!("failed to dump database: {}", String::from_utf8(output.stderr)?);
  }
  Ok(())
}

fn compress_and_sign(
  src: &String,
  exclude: &Option<Vec<String>>,
  compress_file_name: &String,
  compress_sha256_file_name: &String,
) -> anyhow::Result<()> {
  // compress
  let mut command = std::process::Command::new("tar");
  command.arg("-zcvf").arg(compress_file_name);
  if let Some(pattern_vec) = exclude {
    for pattern in pattern_vec {
      command.arg("--exclude").arg(pattern);
    }
  }
  command.arg(src);
  let output = command.output()?;
  if output.status.success() {
    debug!(
      "compress file, current_dir: {}\n{}",
      env::current_dir()?.display(),
      String::from_utf8(output.stdout)?
    );
  } else {
    bail!("failed to compress: {}", String::from_utf8(output.stderr)?);
  }
  // sign
  let output = std::process::Command::new("shasum")
    .arg("--algorithm")
    .arg("256")
    .arg(compress_file_name)
    .output()?;
  if output.status.success() {
    fs::write(compress_sha256_file_name, &output.stdout)?;
    debug!(
      "shasum successfully: {} -> {}",
      compress_file_name, compress_sha256_file_name
    );
  } else {
    bail!("failed to shasum, error: {}", String::from_utf8(output.stderr)?);
  }
  Ok(())
}

fn upload_by_rclone(
  remote_name: &String,
  remote_path: &String,
  local_path: &Vec<String>,
  bin_path: &String,
  rotate: &usize,
) -> anyhow::Result<()> {
  let remote = format!("{remote_name}:{remote_path}");
  for local in local_path {
    let output = std::process::Command::new(bin_path)
      .arg("copy")
      .arg(local)
      .arg(&remote)
      .output()?;
    if output.status.success() {
      info!("copy file from [{}] to [{}] by rclone", local, remote);
    } else {
      warn!(
        "failed to copy file from [{}] to [{}] by rclone, error: {}",
        local,
        remote,
        String::from_utf8(output.stderr)?
      );
    }
  }
  let output = std::process::Command::new(bin_path)
    .arg("lsjson")
    .arg(&remote)
    .output()?;
  if !output.status.success() {
    bail!(
      "failed to ls json by rclone, error: {}",
      String::from_utf8(output.stderr)?
    );
  }
  let mut rclone_ls_vec: Vec<RcloneLs> = serde_json::from_str(&String::from_utf8(output.stdout)?)?;
  debug!("rclone ls vec: {:?}", rclone_ls_vec);
  let cut_count = rclone_ls_vec.len() as isize - (rotate * 2) as isize;
  if cut_count > 0 {
    rclone_ls_vec.sort_by(|o1, o2| o2.mod_time.cmp(&o1.mod_time));
    debug!("sort rclone ls vec: {:?}", rclone_ls_vec);
    debug!("cut count: {cut_count}");
    for _ in 0..cut_count {
      if let Some(rclone_ls) = rclone_ls_vec.pop() {
        let remote = format!("{}:{}/{}", remote_name, remote_path, rclone_ls.name);
        let output = std::process::Command::new(bin_path)
          .arg("deletefile")
          .arg(&remote)
          .output()?;
        if output.status.success() {
          info!("delete [{}] by rclone", remote);
        } else {
          warn!(
            "failed to delete [{}] by rclone, error: {}",
            remote,
            String::from_utf8(output.stderr)?
          );
        }
      }
    }
  }
  Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let start_time = std::time::Instant::now();
  let is_load_dotenv;
  // parse cli and load .env file
  let args = Args::parse();
  if let Some(env_file) = &args.env_file {
    if dotenvy::from_path(env_file).is_err() {
      bail!("can not load .env file from '{env_file}'");
    }
    is_load_dotenv = true;
  } else {
    is_load_dotenv = dotenvy::dotenv().is_ok();
  }

  // reparse cli
  let args = Args::parse();
  if env::var("RUST_LOG").is_err() {
    if args.debug {
      env::set_var("RUST_LOG", "backup=debug,reqwest=debug");
    } else {
      env::set_var("RUST_LOG", "backup=info,reqwest=warn");
    }
  }

  // tracing
  tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .with_timer(tracing_subscriber::fmt::time::time())
    .init();

  // check
  if !is_load_dotenv {
    info!("can not detect .env file");
  }
  let data_path = args.data_path.expect("data path can not be empty");
  let rclone_remote_name = args.rclone_remote_name.expect("rclone remote name can not be empty");
  let rclone_remote_path = args.rclone_remote_path.expect("rclone remote path can not be empty");
  let rclone_bin_path = args.rclone_bin_path.expect("rclone bin path can not be empty");
  if data_path.is_empty() {
    bail!("data path can not be empty");
  }
  if data_path.iter().any(|s| s.is_empty()) {
    bail!("data path can not contain empty path");
  }
  if rclone_remote_name.is_empty() {
    bail!("rclone remote name can not be empty");
  }
  if rclone_remote_name.iter().any(|s| s.is_empty()) {
    bail!("rclone remote name can not contain empty name");
  }
  if rclone_remote_path.is_empty() {
    bail!("rclone remote path can not be empty");
  }
  if rclone_remote_path == "/" {
    bail!("rclone remote path can not equals to /");
  }

  let temp_dir = TempDir::new()?;
  let temp_data_dir_name = String::from("backup_data");
  let temp_data_dir = format!("{}/{temp_data_dir_name}", temp_dir.path().to_string_lossy());
  let now = Local::now();
  let data_compress_file_name = format!("backup_{}.tar.gz", now.format("%Y%m%d_%H%M%S"));
  let data_compress_sha256_file_name = format!("{}.sha256", &data_compress_file_name);

  fs::create_dir_all(&temp_data_dir)?;
  env::set_current_dir(temp_dir.path())?;
  info!("backup in temp file: {}", temp_dir.path().to_string_lossy());

  // copy source data to temp data directory
  let (docker_data_path, non_docker_data_path): (Vec<String>, Vec<String>) =
    data_path.into_iter().partition(|s| s.starts_with("docker://"));
  if !docker_data_path.is_empty() {
    for path in docker_data_path.iter() {
      let src = path.strip_prefix("docker://").unwrap().to_string();
      copy_files_by_docker(&src, &temp_data_dir)?;
    }
  }
  if !non_docker_data_path.is_empty() {
    copy_files(&non_docker_data_path, &temp_data_dir)?;
  }
  // dump database data to temp data directory
  if let (Some(db_type), Some(container_name)) = (args.db_type, args.db_container_name) {
    let db_dump_file_name = format!("dump_{}.sql", now.format("%Y%m%d_%H%M%S"));
    let db_dump_path = format!("{}/{}", temp_data_dir, db_dump_file_name);
    dump_db_by_docker(&db_dump_path, &container_name, &db_type)?;
  }
  // compress and sign with sha256 source data to temp data directory
  compress_and_sign(
    &temp_data_dir_name,
    &args.exclude,
    &data_compress_file_name,
    &data_compress_sha256_file_name,
  )?;
  // upload
  let mut handles = Vec::new();
  let upload_success_arc = Arc::new(std::sync::Mutex::new(Vec::new()));
  let upload_failed_arc = Arc::new(std::sync::Mutex::new(Vec::new()));
  for remote_name in rclone_remote_name.into_iter() {
    let remote_path = rclone_remote_path.to_string();
    let bin_path = rclone_bin_path.to_string();
    let local_path = vec![
      data_compress_file_name.to_string(),
      data_compress_sha256_file_name.to_string(),
    ];
    let upload_success_arc_clone = upload_success_arc.clone();
    let upload_failed_arc_clone = upload_failed_arc.clone();
    let handle = std::thread::spawn(move || {
      match upload_by_rclone(&remote_name, &remote_path, &local_path, &bin_path, &args.rotate) {
        Ok(_) => {
          let mut vec = upload_success_arc_clone.lock().unwrap();
          vec.push(remote_name);
        }
        Err(err) => {
          error!("failed to upload to remote: [{remote_name}], error: {err}");
          let mut vec = upload_failed_arc_clone.lock().unwrap();
          vec.push(remote_name);
        }
      }
    });
    handles.push(handle);
  }
  // waiting for all threads done
  for handle in handles {
    handle.join().unwrap();
  }
  // notification
  let mut message = String::new();
  let vec = Arc::try_unwrap(upload_success_arc)
    .expect("failed to unwrap upload_success_arc")
    .into_inner()?;
  if !vec.is_empty() {
    message.push_str("The backup was successful as follows:\n");
    vec.iter().for_each(|s| message.push_str(format!("{s}\n").as_str()));
  }
  let vec = Arc::try_unwrap(upload_failed_arc)
    .expect("failed to unwrap upload_failed_arc")
    .into_inner()?;
  if !vec.is_empty() {
    message.push_str("The backup failed as follows:\n");
    vec.iter().for_each(|s| message.push_str(format!("{s}\n").as_str()));
  }
  if let (Some(base_url), Some(username), Some(password), Some(topic)) = (
    &args.ntfy_base_url,
    &args.ntfy_username,
    &args.ntfy_password,
    &args.ntfy_topic,
  ) {
    if [base_url, username, password, topic].into_iter().all(|s| !s.is_empty()) {
      notify::notify_by_nty(base_url, username, password, topic, &message).await?;
    }
  }
  let duration = format!("{:.2}", (std::time::Instant::now() - start_time).as_secs_f64());
  info!("all backups completed in {} seconds", duration);
  Ok(())
}
