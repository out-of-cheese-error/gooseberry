use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::{tempdir, TempDir};

fn make_config_file(temp_dir: &TempDir) -> color_eyre::Result<PathBuf> {
    dotenv::dotenv()?;
    let group_id = dotenv::var("TEST_GROUP_ID").unwrap_or("__world__".into());
    let username = dotenv::var("USERNAME")?;
    let key = dotenv::var("DEVELOPER_KEY")?;

    let db_dir = temp_dir.path().join("db");
    let kb_dir = temp_dir.path().join("kb");

    let config_contents = format!(
        "db_dir = \"{}\"\n\
kb_dir = \"{}\"\n\
hypothesis_username = \"{}\"\n\
hypothesis_key = \"{}\"\n\
hypothesis_group = \"{}\"",
        db_dir.to_str().unwrap(),
        kb_dir.to_str().unwrap(),
        username,
        key,
        group_id
    );
    let config_file = temp_dir.path().join("gooseberry.toml");
    fs::write(&config_file, config_contents)?;
    Ok(config_file.to_path_buf())
}

#[test]
fn it_works() -> color_eyre::Result<()> {
    let temp_dir = tempdir()?;
    let config_file = make_config_file(&temp_dir)?;
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", config_file)
        .arg("view")
        .assert()
        .success();
    temp_dir.close()?;
    Ok(())
}
