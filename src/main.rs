mod cli;
mod common;
mod notify;

use anyhow::bail;
use chrono::Local;
use clap::Parser;
use std::io::IsTerminal;
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
      bail!("Can not load .env file from '{env_file}'");
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
  args.check_valid()?;

  // tracing
  tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .with_ansi(std::io::stdout().is_terminal())
    .with_timer(tracing_subscriber::fmt::time::time())
    .init();

  if !is_load_dotenv {
    info!("Can not detect .env file");
  }

  let temp_dir = TempDir::new()?;
  let temp_data_dir_name = String::from("backup_data");
  let temp_data_dir = format!("{}/{temp_data_dir_name}", temp_dir.path().to_string_lossy());
  let now = Local::now();
  let data_compress_file_name = format!("backup_{}.tar.gz", now.format("%Y%m%d_%H%M%S"));
  let data_compress_sha256_file_name = format!("{}.sha256", &data_compress_file_name);

  fs::create_dir_all(&temp_data_dir)?;
  env::set_current_dir(temp_dir.path())?;
  info!("Backup in temp file: {}", temp_dir.path().to_string_lossy());

  // copy source data to temp data directory
  let data_path = args.data_path.unwrap();
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
  let database_args = args.database_args;
  if database_args.has_args() {
    let db_type = database_args.db_type.unwrap();
    let container_name = database_args.db_container_name.unwrap();
    let db_dump_file_name = format!("dump_{}.sql", now.format("%Y%m%d_%H%M%S"));
    let db_dump_path = format!("{temp_data_dir}/{db_dump_file_name}");
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
  let rclone_args = args.rclone_args;
  let rclone_remote_name = rclone_args.rclone_remote_name.unwrap();
  let rclone_remote_path = rclone_args.rclone_remote_path.unwrap();
  let rclone_bin_path = rclone_args.rclone_bin_path.unwrap();
  for remote_name in rclone_remote_name.into_iter() {
    let remote_path = rclone_remote_path.clone();
    let bin_path = rclone_bin_path.clone();
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
          error!("Failed to upload to remote: [{remote_name}], error: {err}");
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

  // build upload message
  let mut message = String::new();
  let vec = Arc::try_unwrap(upload_success_arc)
    .expect("Failed to unwrap upload_success_arc")
    .into_inner()?;
  if !vec.is_empty() {
    message.push_str("The backup was successful as follows:\n");
    vec
      .iter()
      .for_each(|s| message.push_str(format!("{s}:{}\n", &rclone_remote_path).as_str()));
  }
  let vec = Arc::try_unwrap(upload_failed_arc)
    .expect("Failed to unwrap upload_failed_arc")
    .into_inner()?;
  if !vec.is_empty() {
    message.push_str("The backup failed as follows:\n");
    vec
      .iter()
      .for_each(|s| message.push_str(format!("{s}:{}\n", &rclone_remote_path).as_str()));
  }
  info!("{}", message);

  // notification
  let ntfy_args = args.ntfy_args;
  if ntfy_args.has_args() {
    notify::notify_by_ntfy(
      &ntfy_args.ntfy_base_url.unwrap(),
      &ntfy_args.ntfy_username,
      &ntfy_args.ntfy_password,
      &ntfy_args.ntfy_token,
      &ntfy_args.ntfy_topic.unwrap(),
      &message,
    )
    .await?;
  }

  let duration = format!("{:.2}", (std::time::Instant::now() - start_time).as_secs_f64());
  info!("All backups completed in {} seconds", duration);
  Ok(())
}
