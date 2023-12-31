use std::collections::HashSet;
use std::ffi::OsStr;
/// Tests for the CLI
/// Annotations for testing have the "test_tag" tag
/// This is used to delete and clear created annotations after each test
/// MAKE SURE TO RUN SINGLE-THREADED cargo test -- --test-threads=1
use std::fs;
use std::path::PathBuf;
use std::{thread, time};

use assert_cmd::Command;
use color_eyre::eyre::WrapErr;
use eyre::eyre;
use futures::future::{join_all, try_join_all};
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
        r#"
db_dir = '{}'
hypothesis_username = '{}'
hypothesis_key = '{}'
hypothesis_groups = {{'{}' = "test_group"}}
kb_dir = '{}'
hierarchy = ['Tag']
sort = ['Created']
nested_tag = ' : '
annotation_template = '''{}'''
page_template = '''{}'''
index_link_template = '''{}'''
index_name = '{}'
file_extension = '{}'"#,
        db_dir
            .to_str()
            .ok_or(eyre!("Can't convert directory to string"))?,
        username,
        key,
        group_id,
        kb_dir
            .to_str()
            .ok_or(eyre!("Can't convert directory to string"))?,
        gooseberry::configuration::DEFAULT_ANNOTATION_TEMPLATE,
        gooseberry::configuration::DEFAULT_PAGE_TEMPLATE,
        gooseberry::configuration::DEFAULT_INDEX_LINK_TEMPLATE,
        gooseberry::configuration::DEFAULT_INDEX_FILENAME,
        gooseberry::configuration::DEFAULT_FILE_EXTENSION
    );
    let config_file = temp_dir.path().join("gooseberry.toml");
    fs::write(&config_file, config_contents)?;
    Ok(config_file)
}

#[test]
fn it_works() -> color_eyre::Result<()> {
    let temp_dir = tempdir()?;
    dotenv::dotenv()?;
    let username = dotenv::var("HYPOTHESIS_NAME")?;
    let key = dotenv::var("HYPOTHESIS_KEY")?;
    let group_id = dotenv::var("TEST_GROUP_ID")?;
    let config_file = make_config_file(&temp_dir, &username, &key, &group_id)?;
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.arg("-c")
        .arg(config_file)
        .arg("view")
        .assert()
        .success();
    temp_dir.close()?;
    Ok(())
}

struct TestData {
    temp_dir: TempDir,
    config_file: PathBuf,
    hypothesis_client: hypothesis::Hypothesis,
    annotations: Vec<hypothesis::annotations::Annotation>,
}

impl TestData {
    async fn populate() -> color_eyre::Result<Self> {
        dotenv::dotenv()?;
        let temp_dir = tempdir()?;
        let username = dotenv::var("HYPOTHESIS_NAME")?;
        let key = dotenv::var("HYPOTHESIS_KEY")?;
        let group_id = dotenv::var("TEST_GROUP_ID")?;
        let config_file = make_config_file(&temp_dir, &username, &key, &group_id)?;

        // make hypothesis client
        let hypothesis_client = hypothesis::Hypothesis::new(&username, &key);
        assert!(hypothesis_client.is_ok(), "Couldn't authorize");
        let hypothesis_client = hypothesis_client?;

        // make annotations
        let annotation_1 = hypothesis::annotations::InputAnnotation::builder()
            .uri("https://www.example.com")
            .text("this is a test comment")
            .tags(vec!["test_tag".into(), "test_tag1".into()])
            .group(&group_id)
            .build()?;
        let annotation_2 = hypothesis::annotations::InputAnnotation::builder()
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
        Ok(TestData {
            temp_dir,
            config_file,
            hypothesis_client,
            annotations: vec![a1, a2],
        })
    }

    async fn clear(self) -> color_eyre::Result<()> {
        // delete annotations
        let mut cmd = Command::cargo_bin("gooseberry")?;
        cmd.env("GOOSEBERRY_CONFIG", &self.config_file)
            .arg("delete")
            .arg("--tags=test_tag")
            .arg("-f")
            .assert()
            .success();

        let futures: Vec<_> = self
            .annotations
            .iter()
            .map(|a| self.hypothesis_client.fetch_annotation(&a.id))
            .collect();
        assert!(async { join_all(futures).await }
            .await
            .into_iter()
            .all(|x| x.is_err()));

        // clear
        let mut cmd = Command::cargo_bin("gooseberry")?;
        cmd.env("GOOSEBERRY_CONFIG", &self.config_file)
            .arg("clear")
            .arg("-f")
            .assert()
            .success();
        self.temp_dir.close()?;
        Ok(())
    }
}

#[tokio::test]
async fn sync() -> color_eyre::Result<()> {
    // get test_data
    let test_data = TestData::populate().await;
    assert!(test_data.is_ok());
    let mut test_data = test_data?;

    let duration = time::Duration::from_millis(500);

    // check sync add
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("sync")
        .assert()
        .stdout(predicates::str::contains("Added 2 annotations\n"));

    // update annotation
    test_data.annotations[0].text = "Updated test annotation".into();
    test_data
        .hypothesis_client
        .update_annotation(&test_data.annotations[0])
        .await?;

    // check sync update
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("sync")
        .assert()
        .stdout(predicates::str::contains("Updated 1 annotation"));

    // clear
    test_data.clear().await?;
    Ok(())
}

#[tokio::test]
async fn tag_filter() -> color_eyre::Result<()> {
    // get test_data
    let test_data = TestData::populate().await;
    assert!(test_data.is_ok());
    let test_data = test_data?;
    let duration = time::Duration::from_millis(1000);

    // check sync
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("sync")
        .assert()
        .stdout(predicates::str::contains("Added 2 annotations"));

    // add a tag
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--tags=test_tag")
        .arg("test_tag3")
        .assert()
        .success();
    let futures: Vec<_> = test_data
        .annotations
        .iter()
        .map(|a| test_data.hypothesis_client.fetch_annotation(&a.id))
        .collect();
    assert!(async { try_join_all(futures).await }
        .await?
        .iter()
        .all(|x| x.tags.contains(&"test_tag3".to_owned())));

    // add multiple tags
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--tags=test_tag")
        .arg("test_tag4,test_tag5")
        .assert()
        .success();
    let futures: Vec<_> = test_data
        .annotations
        .iter()
        .map(|a| test_data.hypothesis_client.fetch_annotation(&a.id))
        .collect();
    assert!(async { try_join_all(futures).await }
        .await?
        .iter()
        .all(|x| x.tags.contains(&"test_tag4".to_owned())
            && x.tags.contains(&"test_tag5".to_owned())));

    // delete a tag
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--delete")
        .arg("test_tag3")
        .assert()
        .success();

    let futures: Vec<_> = test_data
        .annotations
        .iter()
        .map(|a| test_data.hypothesis_client.fetch_annotation(&a.id))
        .collect();
    assert!(!async { try_join_all(futures).await }
        .await?
        .into_iter()
        .any(|x| x.tags.contains(&"test_tag3".to_owned())));

    // delete multiple tags
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--delete")
        .arg("test_tag4,test_tag5")
        .assert()
        .success();

    let futures: Vec<_> = test_data
        .annotations
        .iter()
        .map(|a| test_data.hypothesis_client.fetch_annotation(&a.id))
        .collect();
    assert!(!async { try_join_all(futures).await }
        .await?
        .into_iter()
        .any(|x| x.tags.contains(&"test_tag4".to_owned())
            || x.tags.contains(&"test_tag5".to_owned())));

    // Testing filters:
    // include tags
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--tags=test_tag2")
        .arg("test_tag4")
        .assert()
        .success();

    // NOT in a1
    assert!(!test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[0].id)
        .await?
        .tags
        .contains(&"test_tag4".to_owned()));
    // in a2
    assert!(test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[1].id)
        .await?
        .tags
        .contains(&"test_tag4".to_owned()));

    // exclude tags
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--exclude-tags=test_tag2,test_tag4")
        .arg("test_tag5")
        .assert()
        .success();

    // in a1
    assert!(test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[0].id)
        .await?
        .tags
        .contains(&"test_tag5".to_owned()));
    // NOT in a2
    assert!(!test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[1].id)
        .await?
        .tags
        .contains(&"test_tag5".to_owned()));

    // search by text
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--text=another")
        .arg("test_tag6")
        .assert()
        .success();

    // NOT in a1
    assert!(!test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[0].id)
        .await?
        .tags
        .contains(&"test_tag6".to_owned()));
    // in a2
    assert!(test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[1].id)
        .await?
        .tags
        .contains(&"test_tag6".to_owned()));

    // NOT
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--text=another")
        .arg("--not")
        .arg("test_tag7")
        .assert()
        .success();

    // in a1
    assert!(test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[0].id)
        .await?
        .tags
        .contains(&"test_tag7".to_owned()));
    // NOT in a2
    assert!(!test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[1].id)
        .await?
        .tags
        .contains(&"test_tag7".to_owned()));

    // AND
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--tags=test_tag,test_tag1,test_tag5,test_tag7")
        .arg("--and")
        .arg("test_tag8")
        .assert()
        .success();

    // in a1
    assert!(test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[0].id)
        .await?
        .tags
        .contains(&"test_tag8".to_owned()));
    // NOT in a2
    assert!(!test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[1].id)
        .await?
        .tags
        .contains(&"test_tag8".to_owned()));
    // clear data
    test_data.clear().await?;
    Ok(())
}

#[tokio::test]
async fn make() -> color_eyre::Result<()> {
    // get test_data
    let test_data = TestData::populate().await;
    assert!(test_data.is_ok());
    let test_data = test_data?;
    let duration = time::Duration::from_millis(500);

    // sync
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("sync")
        .assert()
        .stdout(predicates::str::contains("Added 2 annotations"));

    // add a tag with spaces
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--tags=test_tag")
        .arg("test tag5")
        .assert()
        .success();
    assert!(test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[0].id)
        .await?
        .tags
        .contains(&"test tag5".to_owned()));

    // add a nested tag
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("tag")
        .arg("--tags=test_tag")
        .arg("test_tag6 : test_tag7")
        .assert()
        .success();
    assert!(test_data
        .hypothesis_client
        .fetch_annotation(&test_data.annotations[0].id)
        .await?
        .tags
        .contains(&"test_tag6 : test_tag7".to_owned()));

    // make
    thread::sleep(duration);
    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("make")
        .arg("-f")
        .arg("-c")
        .arg("--no-index")
        .assert()
        .success();

    // check that the kb folder has tag files and an index file
    let file_names = fs::read_dir(test_data.temp_dir.path().join("kb").as_os_str())?
        .map(|entry| {
            entry.wrap_err("File I/O error").and_then(|e| {
                e.path()
                    .file_name()
                    .ok_or(eyre!("filename ends in ."))
                    .and_then(|f: &OsStr| {
                        f.to_str()
                            .map(String::from)
                            .ok_or(eyre!("non-unicode characters in filename"))
                    })
            })
        })
        .collect::<Result<HashSet<String>, _>>()?;
    // index file shouldn't exist yet
    assert!(!file_names.contains("SUMMARY.md"));

    let mut cmd = Command::cargo_bin("gooseberry")?;
    cmd.env("GOOSEBERRY_CONFIG", &test_data.config_file)
        .arg("index")
        .assert()
        .success();
    // now index file should exist
    assert!(test_data
        .temp_dir
        .path()
        .join("kb")
        .join("SUMMARY.md")
        .exists());

    // check all tag files
    assert!(["test_tag", "test_tag1", "test_tag2", "test tag5",]
        .iter()
        .all(|t| file_names.contains(&format!("{}.md", t))));

    // check nested tags
    assert!(file_names.contains("test_tag6"));
    assert!(test_data
        .temp_dir
        .path()
        .join("kb")
        .join("test_tag6")
        .join("test_tag7.md")
        .exists());
    test_data.clear().await?;
    Ok(())
}
