# Contributing to Gooseberry

First off, thank you for considering contributing to gooseberry.

The following document explains how Gooseberry works and lists some potential improvements (usually with an issue number attached). 
Pick one that seems interesting to work on, or make an issue if you want something added to the list!
If you have any questions about contributing or need help with anything, my nick is ninjani on the [official](https://discord.gg/rust-lang) and [community](https://discord.gg/aVESxV8) Discord servers.
Also, if you don't feel like contributing code, but you're interested in the idea, another way to help is to just use Gooseberry and file feature requests and bug reports.

Gooseberry welcomes contributions from everyone. All contributors are expected to follow the [Rust Code of Conduct](http://www.rust-lang.org/conduct.html).

## Reporting issues

Before reporting an issue on the
[issue tracker](https://github.com/out-of-cheese-error/gooseberry/issues),
please check that it has not already been reported by searching for some related
keywords.

## Pull requests

Try to do one pull request per change.

### Updating the changelog

Update the changes you have made in
[CHANGELOG](https://github.com/out-of-cheese-error/gooseberry/blob/master/CHANGELOG.md)
file under the **Unreleased** section.

Add the changes of your pull request to one of the following subsections,
depending on the types of changes defined by
[Keep a changelog](https://keepachangelog.com/en/1.0.0/):

- `Added` for new features.
- `Changed` for changes in existing functionality.
- `Deprecated` for soon-to-be removed features.
- `Removed` for now removed features.
- `Fixed` for any bug fixes.
- `Security` in case of vulnerabilities.

If the required subsection does not exist yet under **Unreleased**, create it!

## Developing

## Getting started
Clone this repository and explore the code via `cargo doc --open --no-deps`. 

## Testing

To run gooseberry's test suite you'll need a `.env` file in the main folder (i.e. next to `Cargo.toml`) with the following keys set
```text
HYPOTHESIS_KEY=<hypothesis API key>
HYPOTHESIS_NAME=<hypothesis username>
TEST_GROUP_ID=<hypothesis test group ID>
```
Set TEST_GROUP_ID to a **new** Hypothesis group without any annotations in it. The tests will create, update, and delete annotations within this group.

Run tests with `cargo test -- --test-threads=1` (THIS IS IMPORTANT).

If a test fails there may be annotations created in the group which are not yet deleted. This can interfere with future test runs.
To fix this, **change the `HYPOTHESIS_GROUP` to the test group ID (first line below)** and run the following commands
```bash
gooseberry config group <TEST_GROUP_ID>
gooseberry clear -f
gooseberry sync
gooseberry delete --tags=test_tag -a -f
```
Make sure this is done on the test group as this deletes annotations from Hypothesis!

When creating new tests, make sure to tag each created annotation with "test_tag" to make cleanup easier.

### Useful Commands

- Build and run release version:

  ```shell
  cargo build --release && cargo run --release
  ```

- Run Clippy:

  ```shell
  cargo clippy --all
  ```

- Run all tests:

  ```shell
  cargo test --all -- --test-threads=1
  ```

- Check to see if there are code formatting issues

  ```shell
  cargo fmt --all -- --check
  ```

- Format the code in the project

  ```shell
  cargo fmt --all
  ```

## How Gooseberry works
The general idea behind Gooseberry is to pull annotations from Hypothesis (via
the [hypothesis](https://github.com/out-of-cheese-error/rust-hypothesis) crate), store them in a local database,
and write them out as plaintext files to form a personal knowledge base (PKB). 
Tags are used to interlink different annotations to have a more explorable and organized PKB. 
Hypothesis annotations are stored locally in binary format for fast searching and retrieval. 
Changes (with `tag` or `delete`) are pushed to Hypothesis and resynced to the local database.

Here's the current code structure:
```
.
├── configuration.rs
├── errors.rs
├── gooseberry
│   ├── cli.rs
│   ├── database.rs
│   ├── knowledge_base.rs
│   ├── mod.rs
│   └── search.rs
├── lib.rs
├── main.rs
└── utils.rs
```

## Contributions

Contributions to Gooseberry should be made in the form of GitHub pull requests. Each pull request will be reviewed by me (Ninjani) and either landed
in the main tree or given feedback for changes that would be required.

All code in this repository is under the [Apache-2.0](http://www.apache.org/licenses/LICENSE-2.0>)
or the [MIT](http://opensource.org/licenses/MIT) license.

<!-- adapted from https://github.com/servo/servo/blob/master/CONTRIBUTING.md and https://github.com/rust-github/template/blob/master/CONTRIBUTING.md -->
