use std::path::Path;
use std::str;

use hypothesis::annotations::Annotation;
use hypothesis::AnnotationID;

use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;
use crate::utils;
use crate::{EMPTY_TAG, MIN_DATE};

/// If key exists, add value to existing values - join with a semicolon
fn merge_index(_key: &[u8], old_indices: Option<&[u8]>, new_index: &[u8]) -> Option<Vec<u8>> {
    let mut ret = old_indices.map_or_else(Vec::new, |old| old.to_vec());
    if !ret.is_empty() {
        ret.extend_from_slice(&[utils::SEMICOLON]);
    }
    ret.extend_from_slice(new_index);
    Some(ret)
}

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

    /// (re)sets date of last sync to way in the past
    pub fn reset_sync_time(&self) -> color_eyre::Result<()> {
        self.db.insert("last_sync_time", MIN_DATE.as_bytes())?;
        Ok(())
    }

    /// Update last sync date after sync
    pub fn set_sync_time(&self, date: &str) -> color_eyre::Result<()> {
        self.db.insert("last_sync_time", date.as_bytes())?;
        Ok(())
    }

    pub fn get_sync_time(&self) -> color_eyre::Result<String> {
        match self.db.get("last_sync_time")? {
            Some(date_bytes) => Ok(std::str::from_utf8(&date_bytes)?.to_owned()),
            None => Ok(crate::MIN_DATE.to_owned()),
        }
    }

    /// Tree storing annotation id: (tags ...)
    pub fn annotation_to_tags(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("annotation_to_tags")?)
    }

    /// Tree storing tag: ( annotation IDs ...)
    pub fn tag_to_annotations(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("tag_to_annotations")?)
    }

    /// Add a tag to an annotation it's associated with
    pub fn add_to_annotation(
        &self,
        annotation_key: &[u8],
        tag_key: &[u8],
    ) -> color_eyre::Result<()> {
        self.annotation_to_tags()?
            .merge(annotation_key.to_vec(), tag_key.to_vec())?;
        Ok(())
    }

    /// Add an annotation index to a tag it's associated with
    pub fn add_to_tag(&self, tag_key: &[u8], annotation_key: &[u8]) -> color_eyre::Result<()> {
        self.tag_to_annotations()?
            .merge(tag_key.to_vec(), annotation_key.to_vec())?;
        Ok(())
    }

    /// Add an annotation to both trees
    fn add_annotation(
        &self,
        annotation: &Annotation,
        annotation_batch: &mut sled::Batch,
    ) -> color_eyre::Result<()> {
        let annotation_key = annotation.id.as_bytes();
        annotation_batch.insert(annotation_key, utils::join_ids(&annotation.tags)?);
        if annotation.tags.is_empty()
            || annotation
                .tags
                .iter()
                .filter(|t| !t.trim().is_empty())
                .next()
                .is_none()
        {
            self.add_to_tag(EMPTY_TAG.as_bytes(), annotation_key)?;
        } else {
            for tag in &annotation.tags {
                if tag.is_empty() {
                    continue;
                }
                let tag_key = tag.as_bytes();
                self.add_to_tag(tag_key, annotation_key)?;
            }
        }
        Ok(())
    }

    /// add or update annotations from the Hypothesis API
    pub fn sync_annotations(
        &self,
        annotations: &[Annotation],
    ) -> color_eyre::Result<(usize, usize, usize)> {
        let mut added = 0;
        let mut updated = 0;
        let mut ignored = 0;
        let mut annotation_batch = sled::Batch::default();
        let mut tag_batch = sled::Batch::default();
        for annotation in annotations {
            if annotation.tags.contains(&crate::IGNORE_TAG.to_string()) {
                if self.annotation_to_tags()?.contains_key(&annotation.id)? {
                    self.delete_annotation(&annotation.id, &mut tag_batch)?;
                }
                ignored += 1;
                continue;
            }
            let annotation_key = annotation.id.as_bytes();
            if self.annotation_to_tags()?.contains_key(annotation_key)? {
                self.delete_annotation(&annotation.id, &mut tag_batch)?;
                self.add_annotation(annotation, &mut annotation_batch)?;
                updated += 1;
            } else {
                self.add_annotation(annotation, &mut annotation_batch)?;
                added += 1;
            }
        }
        self.tag_to_annotations()?.apply_batch(tag_batch)?;
        self.annotation_to_tags()?.apply_batch(annotation_batch)?;
        Ok((added, updated, ignored))
    }

    /// Delete an annotation index from the tag tree
    pub fn delete_from_tag(
        &self,
        tag_key: &[u8],
        annotation_id: &AnnotationID,
        batch: &mut sled::Batch,
    ) -> color_eyre::Result<()> {
        let new_indices: Vec<_> =
            utils::split_ids(&self.tag_to_annotations()?.get(tag_key)?.ok_or(
                Apologize::TagNotFound {
                    tag: utils::u8_to_str(tag_key)?,
                },
            )?)?
            .into_iter()
            .filter(|index_i| index_i != annotation_id)
            .collect();
        if new_indices.is_empty() {
            batch.remove(tag_key);
        } else {
            batch.insert(tag_key.to_vec(), utils::join_ids(&new_indices)?);
        }
        Ok(())
    }

    /// Delete annotation from the annotation tree
    fn delete_from_annotations(&self, id: &AnnotationID) -> color_eyre::Result<Vec<String>> {
        let annotation_key = id.as_bytes();
        Ok(utils::split_ids(
            &self
                .annotation_to_tags()?
                .remove(annotation_key)?
                .ok_or(Apologize::AnnotationNotFound { id: id.to_owned() })?,
        )?)
    }

    /// Delete annotation from database
    pub fn delete_annotation(
        &self,
        id: &AnnotationID,
        tag_batch: &mut sled::Batch,
    ) -> color_eyre::Result<Vec<String>> {
        let tags = self.delete_from_annotations(id)?;
        for tag in &tags {
            if tag.is_empty() {
                continue;
            }
            self.delete_from_tag(tag.as_bytes(), id, tag_batch)?;
        }
        Ok(tags)
    }

    /// Delete multiple annotations
    pub fn delete_annotations(&self, ids: &[AnnotationID]) -> color_eyre::Result<Vec<Vec<String>>> {
        let mut tag_batch = sled::Batch::default();
        let mut annotation_batch = sled::Batch::default();
        let mut tags_list = Vec::with_capacity(ids.len());
        for id in ids {
            let tags = self.get_annotation_tags(id)?;
            annotation_batch.remove(id.as_bytes());
            for tag in &tags {
                self.delete_from_tag(tag.as_bytes(), id, &mut tag_batch)?;
            }
            tags_list.push(tags);
        }
        self.tag_to_annotations()?.apply_batch(tag_batch)?;
        self.annotation_to_tags()?.apply_batch(annotation_batch)?;
        Ok(tags_list)
    }

    /// Retrieve annotations tagged with a given tag
    pub fn get_tagged_annotations(&self, tag: &str) -> color_eyre::Result<Vec<AnnotationID>> {
        utils::split_ids(&self.tag_to_annotations()?.get(tag.as_bytes())?.ok_or(
            Apologize::TagNotFound {
                tag: tag.to_owned(),
            },
        )?)
    }

    /// Retrieve tags associated with an annotation
    pub fn get_annotation_tags(&self, id: &AnnotationID) -> color_eyre::Result<Vec<String>> {
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
}
