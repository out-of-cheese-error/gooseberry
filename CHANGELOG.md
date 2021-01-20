# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

* Fixed Issue #49

## [0.4.0] - 2021-01-19

### Changed

* escape spaces in links with "%20". See https://github.com/rust-lang/mdBook/issues/527
* remove IGNORE_TAG business, delete always deletes from hypothesis
* list available groups on running `gooseberry config group` with the "use existing" option

## [0.3.0] - 2021-01-17

### Changed:

* tokio 1.0 update
* Fixed some bugs
* Added a make test

## [0.2.0-alpha] - 2021-01-16

Switching to Handlebars templates instead of restricting to mdBook-style wiki (Major change). Tests don't work right now.

## [0.1.1] - 2020-11-28

### Changed:

* hypothesis crate points to crates.io version instead of git
* upgraded dependencies (except tokio and directories-next)

### Added:

* badges to README
* link to releases in README
* first crates.io version

## [0.1.0] - 2020-11-28
First somewhat decent release!

Main commands:

* `gooseberry sync` - syncs hypothesis annotations to gooseberry
* `gooseberry search` - opens an interactive search buffer to select annotations. Has keyboard shortcuts to add tags, remove tags and delete
  annotations. This should be the main entrypoint for users while `gooseberry tag`, `gooseberry delete`, and `gooseberry view`
  are more for automating these tasks.
* `gooseberry make` - builds the mdbook knowledge base
* `gooseberry config` - manages configuration, view and edit Hypothesis credentials, the Hypothesis group, and the location of the knowledge base
* `gooseberry move` - move annotations from one group to another (**move** not copy). Useful if you have a bunch of annotations scattered around and
  want to move them into one group for gooseberry.

[0.4.0]: https://github.com/out-of-cheese-error/gooseberry/compare/0.3.0...0.4.0

[0.3.0]: https://github.com/out-of-cheese-error/gooseberry/compare/0.2.0-alpha...0.3.0

[0.2.0-alpha]: https://github.com/out-of-cheese-error/gooseberry/compare/0.1.1...0.2.0-alpha

[0.1.1]: https://github.com/out-of-cheese-error/gooseberry/compare/0.1.0...0.1.1

[0.1.0]: https://github.com/out-of-cheese-error/gooseberry/releases/tag/0.1.0
