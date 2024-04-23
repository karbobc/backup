mod cli;
mod common;
mod notify;

use anyhow::bail;
use chrono::Local;
use clap::Parser;
use std::sync::Arc;
use std::{env, fs};
use tempfile::TempDir;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let start_time = std::time::Instant::now();
  let is_load_dotenv;
  // parse cli and load .env file
  let args = cli::Args::parse();
  if let Some(env_file) = &args.env_file {
    if dotenvy::from_path(env_file).is_err() {
      bail!("can not load .env file from '{env_file}'");
    }
    is_load_dotenv = true;
  } else {
    is_load_dotenv = dotenvy::dotenv().is_ok();
  }

  // reparse cli
  let args = cli::Args::parse();
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
      common::copy_files_by_docker(&src, &temp_data_dir).await?;
    }
  }
  if !non_docker_data_path.is_empty() {
    common::copy_files(&non_docker_data_path, &temp_data_dir).await?;
  }
  // dump database data to temp data directory
  if let (Some(db_type), Some(container_name)) = (args.db_type, args.db_container_name) {
    let db_dump_file_name = format!("dump_{}.sql", now.format("%Y%m%d_%H%M%S"));
    let db_dump_path = format!("{}/{}", temp_data_dir, db_dump_file_name);
    common::dump_db_by_docker(&db_dump_path, &container_name, &db_type).await?;
  }
  // compress and sign with sha256 source data to temp data directory
  common::compress_and_sign(
    &temp_data_dir_name,
    &args.exclude,
    &data_compress_file_name,
    &data_compress_sha256_file_name,
  )
  .await?;
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
      match common::upload_by_rclone(&remote_name, &remote_path, &local_path, &bin_path, &args.rotate) {
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
    vec
      .iter()
      .for_each(|s| message.push_str(format!("{s}:{}\n", &rclone_remote_path).as_str()));
  }
  let vec = Arc::try_unwrap(upload_failed_arc)
    .expect("failed to unwrap upload_failed_arc")
    .into_inner()?;
  if !vec.is_empty() {
    message.push_str("The backup failed as follows:\n");
    vec
      .iter()
      .for_each(|s| message.push_str(format!("{s}:{}\n", &rclone_remote_path).as_str()));
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
