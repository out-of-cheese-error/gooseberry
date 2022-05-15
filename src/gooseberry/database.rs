use std::path::Path;

use hypothesis::annotations::Annotation;

use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;
use crate::utils;
use crate::{EMPTY_TAG, MIN_DATE};

/// If key exists, add value to existing values - join with a semicolon
pub fn merge_index(_key: &[u8], old_indices: Option<&[u8]>, new_index: &[u8]) -> Option<Vec<u8>> {
    let mut ret = old_indices.map_or_else(Vec::new, |old| old.to_vec());
    if !ret.is_empty() {
        ret.extend_from_slice(&[utils::SEMICOLON]);
    }
    ret.extend_from_slice(new_index);
    Some(ret)
}

/// ## Database
/// `sled` database related functions to create, manipulate, and retrieve information in
/// the annotation ID: (tags IDs) tree and the tag ID: (annotation IDs) tree.
/// Also stores and updates the time of the last sync.
impl Gooseberry {
    /// Gets the `sled` database with all gooseberry info.
    /// Makes a new one the first time round
    pub fn get_db(db_dir: &Path) -> color_eyre::Result<sled::Db> {
        Ok(sled::open(db_dir)?)
    }

    /// Merge function for appending items to an existing key, uses semicolons
    pub fn set_merge(&self) -> color_eyre::Result<()> {
        self.tag_to_annotations()?.set_merge_operator(merge_index);
        self.annotation_to_tags()?.set_merge_operator(merge_index);
        Ok(())
    }

    /// (re)sets time of last sync to way in the past
    pub fn reset_sync_time(&self) -> color_eyre::Result<()> {
        self.db.insert("last_sync_time", MIN_DATE.as_bytes())?;
        Ok(())
    }

    /// Update last sync time after sync
    pub fn set_sync_time(&self, datetime: &str) -> color_eyre::Result<()> {
        self.db.insert("last_sync_time", datetime.as_bytes())?;
        Ok(())
    }

    /// Get time of last sync
    pub fn get_sync_time(&self) -> color_eyre::Result<String> {
        match self.db.get("last_sync_time")? {
            Some(date_bytes) => Ok(std::str::from_utf8(&date_bytes)?.to_owned()),
            None => Ok(MIN_DATE.to_owned()),
        }
    }

    /// Tree storing annotation id: (tags ...)
    /// Referred to as the annotation to tags tree
    pub fn annotation_to_tags(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("annotation_to_tags")?)
    }

    /// Tree storing tag: ( annotation IDs ...)
    /// Referred to as the tags tree
    pub fn tag_to_annotations(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("tag_to_annotations")?)
    }

    /// Tree storing annotation ID: annotation
    /// Referred to as the annotations tree
    pub fn annotations(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("annotations")?)
    }

    /// Add an annotation to all trees
    pub fn add_annotation(
        &self,
        annotation: Annotation,
        annotations_batch: &mut sled::Batch,
        annotation_to_tags_batch: &mut sled::Batch,
    ) -> color_eyre::Result<()> {
        let annotation_key = annotation.id.as_bytes();
        annotation_to_tags_batch.insert(annotation_key, utils::join_ids(&annotation.tags)?);
        if annotation.tags.is_empty() || !annotation.tags.iter().any(|t| !t.trim().is_empty()) {
            self.tag_to_annotations()?
                .merge(EMPTY_TAG.as_bytes(), annotation_key)?;
        } else {
            for tag in &annotation.tags {
                if tag.is_empty() {
                    continue;
                }
                let tag_key = tag.as_bytes();
                self.tag_to_annotations()?.merge(tag_key, annotation_key)?;
            }
        }
        let mut annotation_bytes = Vec::new();
        ciborium::ser::into_writer(&annotation, &mut annotation_bytes)?;
        annotations_batch.insert(annotation.id.as_bytes(), &*annotation_bytes);
        Ok(())
    }

    /// add or update annotations from the Hypothesis API
    pub fn sync_annotations(
        &self,
        annotations: Vec<Annotation>,
    ) -> color_eyre::Result<(usize, usize)> {
        let (mut added, mut updated) = (0, 0);
        let mut annotation_to_tags_batch = sled::Batch::default();
        let mut annotations_batch = sled::Batch::default();
        for annotation in annotations {
            let annotation_key = annotation.id.as_bytes();
            if self.annotation_to_tags()?.contains_key(annotation_key)? {
                self.delete_annotation(&annotation.id)?;
                self.add_annotation(
                    annotation,
                    &mut annotations_batch,
                    &mut annotation_to_tags_batch,
                )?;
                updated += 1;
            } else {
                self.add_annotation(
                    annotation,
                    &mut annotations_batch,
                    &mut annotation_to_tags_batch,
                )?;
                added += 1;
            }
        }
        self.annotation_to_tags()?
            .apply_batch(annotation_to_tags_batch)?;
        self.annotations()?.apply_batch(annotations_batch)?;
        Ok((added, updated))
    }

    /// Delete an annotation index from the tag tree
    pub fn delete_from_tag_to_annotations_tree(
        &self,
        tag_key: &[u8],
        annotation_id: &str,
    ) -> color_eyre::Result<()> {
        let new_indices: Vec<_> =
            utils::split_ids(&self.tag_to_annotations()?.get(tag_key)?.ok_or(
                Apologize::TagNotFound {
                    tag: std::str::from_utf8(tag_key)?.to_owned(),
                },
            )?)?
            .into_iter()
            .filter(|index_i| index_i != annotation_id)
            .collect();
        if new_indices.is_empty() {
            self.tag_to_annotations()?.remove(tag_key)?;
        } else {
            self.tag_to_annotations()?
                .insert(tag_key, utils::join_ids(&new_indices)?)?;
        }
        Ok(())
    }

    /// Delete an annotation ID from the annotation to tags tree
    pub fn delete_from_annotation_to_tags_tree(&self, id: &str) -> color_eyre::Result<Vec<String>> {
        let annotation_key = id.as_bytes();
        utils::split_ids(
            &self
                .annotation_to_tags()?
                .remove(annotation_key)?
                .ok_or(Apologize::AnnotationNotFound { id: id.to_owned() })?,
        )
    }

    /// Delete an annotation ID from the annotations tree
    pub fn delete_from_annotations_tree(&self, id: &str) -> color_eyre::Result<()> {
        let annotation_key = id.as_bytes();
        self.annotations()?.remove(annotation_key)?;
        Ok(())
    }

    /// Delete annotation from database
    pub fn delete_annotation(&self, id: &str) -> color_eyre::Result<Vec<String>> {
        let tags = self.delete_from_annotation_to_tags_tree(id)?;
        for tag in &tags {
            if tag.is_empty() {
                continue;
            }
            self.delete_from_tag_to_annotations_tree(tag.as_bytes(), id)?;
        }
        self.delete_from_annotations_tree(id)?;
        Ok(tags)
    }

    /// Delete multiple annotations
    pub fn delete_annotations(&self, ids: &[String]) -> color_eyre::Result<Vec<Vec<String>>> {
        let mut annotation_to_tags_batch = sled::Batch::default();
        let mut annotation_batch = sled::Batch::default();
        let mut tags_list = Vec::with_capacity(ids.len());
        for id in ids {
            let tags = self.get_annotation_tags(id)?;
            annotation_to_tags_batch.remove(id.as_bytes());
            annotation_batch.remove(id.as_bytes());
            for tag in &tags {
                self.delete_from_tag_to_annotations_tree(tag.as_bytes(), id)?;
            }
            tags_list.push(tags);
        }
        self.annotation_to_tags()?
            .apply_batch(annotation_to_tags_batch)?;
        self.annotations()?.apply_batch(annotation_batch)?;
        Ok(tags_list)
    }

    /// Retrieve annotations tagged with a given tag
    pub fn get_tagged_annotations(&self, tag: &str) -> color_eyre::Result<Vec<String>> {
        utils::split_ids(&self.tag_to_annotations()?.get(tag.as_bytes())?.ok_or(
            Apologize::TagNotFound {
                tag: tag.to_owned(),
            },
        )?)
    }

    /// Retrieve tags associated with an annotation
    pub fn get_annotation_tags(&self, id: &str) -> color_eyre::Result<Vec<String>> {
        let annotation_key = id.as_bytes();
        let tags = utils::split_ids(
            &self
                .annotation_to_tags()?
                .get(annotation_key)?
                .ok_or(Apologize::AnnotationNotFound { id: id.to_owned() })?,
        )?;
        if tags.len() == 1 && tags[0].is_empty() {
            Ok(Vec::new())
        } else {
            Ok(tags)
        }
    }

    /// Retrieve annotation by ID
    pub fn get_annotation(&self, id: &str) -> color_eyre::Result<Annotation> {
        let annotation_key = id.as_bytes();
        let annotation_bytes = self
            .annotations()?
            .get(annotation_key)?
            .ok_or(Apologize::AnnotationNotFound { id: id.to_owned() })?;
        Ok(ciborium::de::from_reader(&*annotation_bytes)?)
    }

    pub fn iter_annotations(
        &self,
    ) -> color_eyre::Result<impl Iterator<Item = color_eyre::Result<Annotation>>> {
        Ok(self.annotations()?.iter().map(|item| {
            let (_, annotation_bytes) = item?;
            Ok(ciborium::de::from_reader(&*annotation_bytes)?)
        }))
    }
}
