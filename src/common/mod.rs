use crate::common::model::{DatabaseType, RcloneLs};
use anyhow::bail;
use tracing::{debug, info, warn};

pub mod model;

pub async fn copy_files(src: &Vec<String>, dest: &String) -> anyhow::Result<()> {
  let output = tokio::process::Command::new("cp")
    .arg("-a")
    .args(src)
    .arg(dest)
    .output()
    .await?;
  if !output.status.success() {
    bail!(
      "failed to copy source data, error: {}",
      String::from_utf8(output.stderr)?
    );
  }
  Ok(())
}

pub async fn copy_files_by_docker(src: &String, dest: &String) -> anyhow::Result<()> {
  let output = tokio::process::Command::new("docker")
    .arg("cp")
    .arg(src)
    .arg(dest)
    .arg("-q")
    .output()
    .await?;
  if !output.status.success() {
    bail!(
      "failed to copy source data by docker, error: {}",
      String::from_utf8(output.stderr)?
    );
  }
  Ok(())
}

pub async fn dump_db_by_docker(
  db_dump_path: &String,
  container_name: &String,
  db_type: &DatabaseType,
) -> anyhow::Result<()> {
  let db_dump_cmd = match db_type {
    DatabaseType::Mysql => {
      "exec mysqldump -u$MYSQL_USER -p$MYSQL_PASSWORD --databases $MYSQL_DATABASE --no-tablespaces"
    }
    DatabaseType::Postgres => "pg_dump postgresql://$PG_USER:$PG_PASSWORD@127.0.0.1/$PG_DATABASE --clean",
  };
  let output = tokio::process::Command::new("docker")
    .arg("exec")
    .arg("-i")
    .arg(container_name)
    .arg("/bin/sh")
    .arg("-c")
    .arg(db_dump_cmd)
    .output()
    .await
    .expect("failed to dump database");
  if output.status.success() {
    tokio::fs::write(db_dump_path, &output.stdout)
      .await
      .expect("failed to write database backup data to file");
    debug!("dump database data to [{db_dump_path}] by docker");
  } else {
    bail!("failed to dump database: {}", String::from_utf8(output.stderr)?);
  }
  Ok(())
}

pub async fn compress_and_sign(
  src: &String,
  exclude: &Option<Vec<String>>,
  compress_file_name: &String,
  compress_sha256_file_name: &String,
) -> anyhow::Result<()> {
  // compress
  let mut command = tokio::process::Command::new("tar");
  command.arg("-zcvf").arg(compress_file_name);
  if let Some(pattern_vec) = exclude {
    for pattern in pattern_vec {
      command.arg("--exclude").arg(pattern);
    }
  }
  command.arg(src);
  let output = command.output().await?;
  if output.status.success() {
    debug!(
      "compress file, current_dir: {}\n{}",
      std::env::current_dir()?.display(),
      String::from_utf8(output.stdout)?
    );
  } else {
    bail!("failed to compress: {}", String::from_utf8(output.stderr)?);
  }
  // sign
  let output = tokio::process::Command::new("shasum")
    .arg("--algorithm")
    .arg("256")
    .arg(compress_file_name)
    .output()
    .await?;
  if output.status.success() {
    tokio::fs::write(compress_sha256_file_name, &output.stdout).await?;
    debug!(
      "shasum successfully: {} -> {}",
      compress_file_name, compress_sha256_file_name
    );
  } else {
    bail!("failed to shasum, error: {}", String::from_utf8(output.stderr)?);
  }
  Ok(())
}

pub fn upload_by_rclone(
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
