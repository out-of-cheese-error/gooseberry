/// Tests for the CLI
/// Annotations for testing have the "test_tag" tag
/// This is used to delete and clear created annotations after each test
/// MAKE SURE TO RUN SINGLE-THREADED cargo test -- --test-threads=1
use std::fs;
use std::path::PathBuf;
use std::{thread, time};

use assert_cmd::Command;
use tempfile::{tempdir, TempDir};

fn make_config_file(
    temp_dir: &TempDir,
    username: &str,
    key: &str,
    group_id: &str,
) -> color_eyre::Result<PathBuf> {
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
    dotenv::dotenv()?;
    let username = dotenv::var("USERNAME")?;
    let key = dotenv::var("DEVELOPER_KEY")?;
    let group_id = dotenv::var("TEST_GROUP_ID")?;
    let config_file = make_config_file(&temp_dir, &username, &key, &group_id)?;
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", config_file)
        .arg("view")
        .assert()
        .success();
    temp_dir.close()?;
    Ok(())
}

#[tokio::test]
async fn sync() -> color_eyre::Result<()> {
    // config file
    let temp_dir = tempdir()?;
    dotenv::dotenv()?;
    let username = dotenv::var("USERNAME")?;
    let key = dotenv::var("DEVELOPER_KEY")?;
    let group_id = dotenv::var("TEST_GROUP_ID")?;
    let config_file = make_config_file(&temp_dir, &username, &key, &group_id)?;

    // make hypothesis client
    let hypothesis_client = hypothesis::Hypothesis::new(&username, &key);
    assert!(hypothesis_client.is_ok(), "Couldn't authorize");
    let hypothesis_client = hypothesis_client?;

    // make annotations
    let annotation_1 = hypothesis::annotations::InputAnnotationBuilder::default()
        .uri("https://www.example.com")
        .text("this is a test comment")
        .tags(vec!["test_tag".into(), "test_tag1".into()])
        .group(&group_id)
        .build()?;
    let annotation_2 = hypothesis::annotations::InputAnnotationBuilder::default()
        .uri("https://www.example.com")
        .text("this is another test comment")
        .tags(vec![
            "test_tag".into(),
            "test_tag1".into(),
            "test_tag2".into(),
        ])
        .group(&group_id)
        .build()?;
    let mut a1 = hypothesis_client.create_annotation(&annotation_1).await?;
    let a2 = hypothesis_client.create_annotation(&annotation_2).await?;

    let duration = time::Duration::from_millis(500);
    thread::sleep(duration);

    // check sync add
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("sync")
        .assert()
        .stdout(predicates::str::contains("Added 2 notes"));

    // update annotation
    a1.text = "Updated test annotation".into();
    hypothesis_client.update_annotation(&a1).await?;

    thread::sleep(duration);

    // check sync update
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("sync")
        .assert()
        .stdout(predicates::str::contains("Updated 1 note"));

    // delete annotations
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("delete")
        .arg("--tags=test_tag")
        .arg("-a") //also from hypothesis
        .arg("-f")
        .assert()
        .success();
    assert!(hypothesis_client.fetch_annotation(&a1.id).await.is_err());
    assert!(hypothesis_client.fetch_annotation(&a2.id).await.is_err());

    // clear
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("clear")
        .arg("-f")
        .assert()
        .success();
    temp_dir.close()?;
    Ok(())
}

#[tokio::test]
async fn tag() -> color_eyre::Result<()> {
    // config file
    let temp_dir = tempdir()?;
    dotenv::dotenv()?;
    let username = dotenv::var("USERNAME")?;
    let key = dotenv::var("DEVELOPER_KEY")?;
    let group_id = dotenv::var("TEST_GROUP_ID")?;
    let config_file = make_config_file(&temp_dir, &username, &key, &group_id)?;
    let duration = time::Duration::from_millis(500);

    // make hypothesis client
    let hypothesis_client = hypothesis::Hypothesis::new(&username, &key);
    assert!(hypothesis_client.is_ok(), "Couldn't authorize");
    let hypothesis_client = hypothesis_client?;

    // make annotations
    let annotation_1 = hypothesis::annotations::InputAnnotationBuilder::default()
        .uri("https://www.example.com")
        .text("this is a test comment")
        .tags(vec!["test_tag".into(), "test_tag1".into()])
        .group(&group_id)
        .build()?;
    let annotation_2 = hypothesis::annotations::InputAnnotationBuilder::default()
        .uri("https://www.example.com")
        .text("this is another test comment")
        .tags(vec![
            "test_tag".into(),
            "test_tag1".into(),
            "test_tag2".into(),
        ])
        .group(&group_id)
        .build()?;
    let a1 = hypothesis_client.create_annotation(&annotation_1).await?;
    let a2 = hypothesis_client.create_annotation(&annotation_2).await?;

    // check sync
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("sync")
        .assert()
        .stdout(predicates::str::contains("Added 2 notes"));
    // tag
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("tag")
        .arg("--tags=test_tag")
        .arg("test_tag3")
        .assert()
        .stdout(predicates::str::contains("Updated 2 notes"));
    assert!(hypothesis_client
        .fetch_annotation(&a1.id)
        .await?
        .tags
        .contains(&"test_tag3".to_owned()));
    assert!(hypothesis_client
        .fetch_annotation(&a2.id)
        .await?
        .tags
        .contains(&"test_tag3".to_owned()));

    // delete tags
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("tag")
        .arg("--delete")
        .arg("test_tag3")
        .assert()
        .stdout(predicates::str::contains("Updated 2 notes"));
    assert!(!hypothesis_client
        .fetch_annotation(&a1.id)
        .await?
        .tags
        .contains(&"test_tag3".to_owned()));
    assert!(!hypothesis_client
        .fetch_annotation(&a2.id)
        .await?
        .tags
        .contains(&"test_tag3".to_owned()));

    // check tags filtered
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("tag")
        .arg("--tags=test_tag2")
        .arg("test_tag4")
        .assert()
        .stdout(predicates::str::contains("Updated 1 note"));
    // NOT in a1
    assert!(!hypothesis_client
        .fetch_annotation(&a1.id)
        .await?
        .tags
        .contains(&"test_tag4".to_owned()));
    // in a2
    assert!(hypothesis_client
        .fetch_annotation(&a2.id)
        .await?
        .tags
        .contains(&"test_tag4".to_owned()));

    // delete annotations
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("delete")
        .arg("--tags=test_tag") // only test annotations
        .arg("-a") // also from hypothesis
        .arg("-f") // force
        .assert()
        .success();
    assert!(hypothesis_client.fetch_annotation(&a1.id).await.is_err());
    assert!(hypothesis_client.fetch_annotation(&a2.id).await.is_err());

    // clear gooseberry db
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &config_file)
        .arg("clear")
        .arg("-f")
        .assert()
        .success();
    temp_dir.close()?;
    Ok(())
}
