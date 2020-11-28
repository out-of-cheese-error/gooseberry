# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Main commands:
* `gooseberry sync` - syncs hypothesis annotations to gooseberry
* `gooseberry search` - opens an interactive search buffer to select annotations. 
   Has keyboard shortcuts to add tags, remove tags and delete annotations. 
   This should be the main entrypoint for users while `gooseberry tag`, `gooseberry delete`, and `gooseberry view` 
   are more for automating these tasks.
* `gooseberry make` - builds the mdbook knowledge base
* `gooseberry config` - manages configuration, view and edit Hypothesis credentials, the Hypothesis group, and the location of the knowledge base 
* `gooseberry move` - move annotations from one group to another (**move** not copy). 
   Useful if you have a bunch of annotations scattered around and want to move them into one group for gooseberry.