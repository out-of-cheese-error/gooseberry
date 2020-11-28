# Contributing to Gooseberry

First off, thank you for considering contributing to gooseberry.

The following document explains how Gooseberry works and lists some potential improvements (usually with an issue number attached). 
Pick one that seems interesting to work on, or make an issue if you want something added to the list!
If you have any questions about contributing or need help with anything, my nick is ninjani on the [official](https://discord.gg/rust-lang) and [community](https://discord.gg/aVESxV8) Discord servers.
Also, if you don't feel like contributing code but you're interested in the idea, another way to help is to just use Gooseberry and file feature requests (over [here](https://github.com/out-of-cheese-error/gooseberry/issues/11) or in a separate issue) and bug reports.

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
You'll also need to install [mdBook](https://rust-lang.github.io/mdBook/index.html) and [mdbook_mermaid](https://docs.rs/mdbook-mermaid/0.4.2/mdbook_mermaid/index.html) to view the generated markdown files.

## Testing

To run gooseberry's test suite you'll need a `.env` file in the main folder (i.e. next to `Cargo.toml`) with the following keys set
```text
HYPOTHESIS_KEY=<hypothesis API key>
HYPOTHESIS_NAME=<hypothesis username>
TEST_GROUP_ID=<hypothesis test group ID>
```
Set TEST_GROUP_ID to a **new** Hypothesis group without any annotations in it. The tests will create, update, and delete annotations within this group.

Run tests with `cargo test -- --test-threads=1` (THIS IS IMPORTANT).

If a test fails there may be annotations created in the group which are not yet deleted. This can interfere with further test runs.
To fix this, **change the `HYPOTHESIS_GROUP` in your config to the test group ID** and run the following commands
```bash
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
The general idea behind Gooseberry is to pull annotations from Hypothesis (via the [hypothesis](https://github.com/out-of-cheese-error/rust-hypothesis) crate) 
and write them out as markdown files (in an [mdBook](https://rust-lang.github.io/mdBook/index.html) format) to form a personal knowledge base (PKB). 
Tags are used to interlink different annotations to have a more explorable and organized PKB.

Here's the current code structure:
```
.
├── configuration.rs
├── errors.rs
├── gooseberry
│   ├── cli.rs
│   ├── database.rs
│   ├── markdown.rs
│   ├── mod.rs
│   └── search.rs
├── lib.rs
├── main.rs
└── utils.rs
```

### `configuration.rs`
Deals with configuring Gooseberry in terms of:
1. db_dir: the directory in which to store `sled` database files. 
This is automatically set in a platform-specific manner using [directories-next](https://crates.io/crates/directories-next) and will not need to be accessed by the user.
Running `gooseberry clear` will clear the database, empty this directory and reset the sync time.
2. kb_dir: the `mdBook` directory storing the wiki. This is set the first time Gooseberry is run and can be reset with `gooseberry config directory`.
3. hypothesis_username: Username to authorize Hypothesis. This is taken from the `HYPOTHESIS_NAME` environment variable if it exists, or set the first time Gooseberry is run via an input prompt. It can be changed by calling `gooseberry config authorize`. 
4. hypothesis_key: personal API developer key obtained from Hypothesis; queried and set during the first run or taken from the `HYPOTHESIS_KEY` environment variable. It can be changed by calling `gooseberry config authorize`.
5. hypothesis_group: the Hypothesis group from which to sync annotations. On the first run, the user can either choose to create a new group or enter an existing group ID. This can be changed by calling `gooseberry config group`.

The config file location is automatically set but can be changed using the `GOOSEBERRY_CONFIG` environment variable.

#### Possible improvements
~~* Setting the kb_dir somewhere more accessible to the user ([Issue #8](https://github.com/out-of-cheese-error/gooseberry/issues/8): easy)~~
* Using a list of Hypothesis groups instead of just one, for different subjects for instance
    * This would open up the possibility of splitting the Wiki by group with interlinking tags ([Issue #9](https://github.com/out-of-cheese-error/gooseberry/issues/9): medium)

### `errors.rs`
Gooseberry-specific errors with meaningful messages. 
In general the code makes use of `suggestion`s via the [eyre](https://crates.io/crates/eyre) crate: these should inform the user on how to fix something that's broken on their end (e.g wrong credentials)

### `gooseberry/cli.rs`
The StructOpt CLI 

#### Possible improvements
Needs more tests

### `gooseberry/database.rs`
The database has two `sled` Trees (which behave like `BTreeMap`s) and one entry:
1. `annotation_to_tags_tree`: links an annotation ID to the tags it contains.
2. `tag_to_annotations_tree`: links a tag to all the annotation IDs that contain that tag.
3. `last_sync_time`: stores the time of the last Hypothesis sync.

* Calling `gooseberry sync` pulls in annotations from the configured Hypothesis group which were created or updated after the `last_sync_time`. These are then added to the two trees and the sync time is updated.
* Calling `gooseberry tag` (with optional filters) gets a set of annotations from Hypothesis and then adds (/ deletes) a user-specified tag to (/ from) each one. This modification is uploaded back to Hypothesis and the database is re-synced.
Some specific behavior here:
    * Adding the `IGNORE_TAG` (found in `lib.rs`) removes a particular annotation from Gooseberry's consideration: i.e. it's removed from the database and never synced unless the tag is removed.
    * An annotation without any tags is stored in the database trees under the `EMPTY_TAG` key. This is not reflected in Hypothesis.

* Calling `gooseberry delete` (with optional filters) deletes a set of annotations from Gooseberry and optionally also from Hypothesis. If it's just from Gooseberry then the `IGNORE_TAG` is added to each.
* Calling `gooseberry move <group_id>` (with optional filters) moves a set of annotations from the Hypothesis group corresponding to `group_id` to the configured Gooseberry group and then re-syncs the database to add these.

#### Possible improvements
* More flexible tagging behavior. Now the user has to specify each tag one at a time for a set of filtered annotations. May make sense to have an interactive window with multiple selections and tagging on the fly. (Hard)
* Have a fixed set of Gooseberry-specific tags (in an enum or a separate module). Currently there's the `IGNORE_TAG` and `EMPTY_TAG` but it could be possible to have a more flexible linking system with `gooseberry_from:id_to:id` etc. ([Issue #2](https://github.com/out-of-cheese-error/gooseberry/issues/2): question)
* Thoroughly test tagging functionality on edge-cases (no tags, tag exists, tag doesn't exist, tag is changed on Hypothesis but not synced etc.) ([Issue #1](https://github.com/out-of-cheese-error/gooseberry/issues/1): medium)
* Currently depends a lot on an internet connection, would it make sense to also store serialized annotations in the database? 
Note: this functionality was removed because `bincode` doesn't support "tagged" enums and using raw JSON would be both memory-hungry and slow.

### `gooseberry/markdown.rs`
The knowledge base is written out using `mdBook` as a set of flat Markdown files. Annotations can also be printed out in markdown to the terminal, using `bat`.

The format of a single annotation in the terminal is:
```
Jun 1 23:53:30 2020 - *annotation ID*

| tag1 | tag2 |
> Highlighted quote from the website

Comment about quote.
**This** can be *arbitrary* markdown
with LaTeX math $$\pi = 3.14$$.

Source - *www.source_url.com*
```
and in the `mdBook` file is:
```
Jun 1 23:53:30 2020 - *annotation ID*

| [tag1](tag1.md) | [tag2](tag2.md) |
> Highlighted quote from the website

Comment about quote.
**This** can be *arbitrary* markdown
with LaTeX math $$\pi = 3.14$$.

[[see in context at www.source_url.com]](link_to_annotation_in_website.com)

---
```

The index page of the `mdBook` consists of a Mermaid-JS graph of all the tags, detailing the number of notes in each tag and the number shared between every pair.
These graph nodes link to the respective tag pages.

The `mdBook` is constructed by calling `gooseberry make` and can be opened using `mdbook serve kb_dir`

#### Possible improvements
* ~~Add a section to the top or bottom of a tag page that links to related tags (i.e. those with many shared notes). ([Issue #3](https://github.com/out-of-cheese-error/gooseberry/issues/3): easy)~~
* ~~Print progress information and mdbook commands during book building. ([Issue #18](https://github.com/out-of-cheese-error/gooseberry/issues/18): easy)~~
* One major improvement would be to incrementally generate Markdown files instead of regenerating from scratch every time `make` is called. 
This would involve keeping track of each annotation as a Section and only modifying it if its Updated time has changed + appending newly created annotations to the end.
This would theoretically allow a user to modify their markdown files in an editor of choice and store other kinds of (non-annotation) notes in it. 
This would also help integrate Gooseberry into existing Markdown-based PKB tools. ([Issue #10](https://github.com/out-of-cheese-error/gooseberry/issues/10))
* Allow the annotation format to be user-configurable. Also add documentation on configuring the `mdBook` theme to look better for this kind of data? (easy)
* Think about nested folders of markdown files. This could be on the basis of different Hypothesis groups (see [Issue #9](https://github.com/out-of-cheese-error/gooseberry/issues/9)) or more sophisticated tagging with parent-child-sibling relationships.
* Test the speed and performance of Mermaid-JS on a large number of notes with many tags. (easy)
* If annotation-annotation direct links are added, show the linked annotations in footnote style or in a margin on the right (this would require making a custom `mdBook` template, something like [Tufte CSS](https://edwardtufte.github.io/tufte-css/) or [Astrochelys](https://github.com/out-of-cheese-error/astrochelys)). ([Issue #2](https://github.com/out-of-cheese-error/gooseberry/issues/2))

### `gooseberry/search.rs`
This handles the `gooseberry search` and `gooseberry move -s` commands which open a fuzzy/exact search window in the terminal using [skim](https://github.com/lotabout/skim). 
Each annotation's markdown is displayed in the preview wndow and the quote, text, tags, and URI are displayed as a single (very long) line which the user can search against.
There are options to scroll (arrow keys), select multiple hits (TAB) and select all (CTRL-A).
In `gooseberry search` there are options to add a tag to the selected annotations (Enter), delete a tag from the selected annotations (Shift-Left), 
and delete the selected annotations (Shift-Right). Adding `--fuzzy` switches to fuzzy search.

#### Possible improvements
* skim has a crazy number of options, this is a matter of checking them out and seeing which would improve user experience.
* the long lines are not very nice and sometimes lag but skim only seems to allow wrapping via the preview window - this needs some more investigation

## Contributions

Contributions to Gooseberry should be made in the form of GitHub pull requests. Each pull request will
be reviewed by me (Ninjani) and either landed in the main tree or given feedback for changes that would be required.

All code in this repository is under the [Apache-2.0](http://www.apache.org/licenses/LICENSE-2.0>)
or the [MIT](http://opensource.org/licenses/MIT) license.

<!-- adapted from https://github.com/servo/servo/blob/master/CONTRIBUTING.md and https://github.com/rust-github/template/blob/master/CONTRIBUTING.md -->
