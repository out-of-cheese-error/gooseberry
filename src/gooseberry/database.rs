use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;
use crate::utils;
use chrono::{DateTime, Utc, MIN_DATE};
use hypothesis::annotations::Annotation;
use hypothesis::AnnotationID;
use std::path::Path;
use std::str;

/// If key exists, add value to existing values - join with a semicolon
fn merge_index(_key: &[u8], old_indices: Option<&[u8]>, new_index: &[u8]) -> Option<Vec<u8>> {
    let mut ret = old_indices.map_or_else(Vec::new, |old| old.to_vec());
    if !ret.is_empty() {
        ret.extend_from_slice(&[utils::SEMICOLON]);
    }
    ret.extend_from_slice(new_index);
    Some(ret)
}

pub trait GooseberrySerialize: Sized {
    fn serialize(&self) -> color_eyre::Result<Vec<u8>>;
    fn deserialize(contents: &[u8]) -> color_eyre::Result<Self>;
}

impl GooseberrySerialize for Annotation {
    fn serialize(&self) -> color_eyre::Result<Vec<u8>> {
        Ok(serde_json::to_string(self)?.into_bytes())
    }

    fn deserialize(contents: &[u8]) -> color_eyre::Result<Self> {
        Ok(serde_json::from_str(str::from_utf8(contents)?)?)
    }
}

impl Gooseberry {
    /// Gets the `sled` database with all gooseberry info.
    /// Makes a new one the first time round
    pub fn get_db(db_dir: &Path) -> color_eyre::Result<sled::Db> {
        Ok(sled::open(db_dir)?)
    }

    /// Merge function for appending items to an existing key, uses semicolons
    pub(crate) fn set_merge(&self) -> color_eyre::Result<()> {
        self.tags_tree()?.set_merge_operator(merge_index);
        Ok(())
    }

    /// (re)sets date of last sync to way in the past
    pub fn reset_sync_time(&self) -> color_eyre::Result<()> {
        self.db
            .insert("last_sync_time", MIN_DATE.to_string().as_bytes())?;
        Ok(())
    }

    /// Update last sync date after sync
    pub(crate) fn set_sync_time(&self, date: DateTime<Utc>) -> color_eyre::Result<()> {
        self.db
            .insert("last_sync_time", date.to_string().as_bytes())?;
        Ok(())
    }

    pub(crate) fn get_sync_time(&self) -> color_eyre::Result<DateTime<Utc>> {
        match self.db.get("last_sync_time")? {
            Some(date_bytes) => Ok(std::str::from_utf8(&date_bytes)?.parse()?),
            None => Ok(utils::parse_datetime("1900")?),
        }
    }

    /// Tree storing annotation id: annotation
    pub(crate) fn annotations_tree(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("annotations")?)
    }

    /// Tree storing tag: ( annotation IDs ...)
    pub(crate) fn tags_tree(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("tags")?)
    }

    /// Add an annotation index to a tag it's associated with
    pub fn add_to_tag(&self, tag: &str, annotation_key: &[u8]) -> color_eyre::Result<()> {
        let tag_key = tag.as_bytes();
        self.tags_tree()?
            .merge(tag_key.to_vec(), annotation_key.to_vec())?;
        Ok(())
    }

    pub fn insert_annotation(
        annotation_key: &[u8],
        annotation: &Annotation,
        batch: &mut sled::Batch,
    ) -> color_eyre::Result<()> {
        let annotation_bytes = annotation.serialize()?;
        batch.insert(annotation_key, annotation_bytes);
        Ok(())
    }

    /// Add an annotation to the annotations tree
    fn add_annotation(
        &self,
        annotation: &Annotation,
        batch: &mut sled::Batch,
    ) -> color_eyre::Result<()> {
        let annotation_key = annotation.id.as_bytes();
        for tag in &annotation.tags {
            self.add_to_tag(tag, annotation_key)?;
        }
        Self::insert_annotation(annotation_key, annotation, batch)?;
        Ok(())
    }

    /// add or update annotations from the Hypothesis API
    pub(crate) fn sync_annotations(
        &self,
        annotations: &[Annotation],
    ) -> color_eyre::Result<(usize, usize)> {
        let mut added = 0;
        let mut updated = 0;
        let mut batch = sled::Batch::default();
        for annotation in annotations {
            if self
                .annotations_tree()?
                .contains_key(annotation.id.as_bytes())?
            {
                self.delete_annotation(&annotation.id)?;
                self.add_annotation(annotation, &mut batch)?;
                updated += 1;
            } else {
                self.add_annotation(annotation, &mut batch)?;
                added += 1;
            }
        }
        self.annotations_tree()?.apply_batch(batch)?;
        Ok((added, updated))
    }

    /// Delete an annotation index from the tag tree
    pub fn delete_from_tag(
        &self,
        tag_key: &[u8],
        annotation_id: &AnnotationID,
        batch: &mut sled::Batch,
    ) -> color_eyre::Result<()> {
        let tag = utils::u8_to_str(tag_key)?;
        let new_indices: Vec<_> = utils::split_ids(
            &self
                .tags_tree()?
                .get(tag_key)?
                .ok_or(Apologize::TagNotFound { tag })?,
        )?
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
    fn delete_from_annotations_tree(&self, id: &AnnotationID) -> color_eyre::Result<Annotation> {
        let index_key = id.as_bytes();
        Ok(Annotation::deserialize(
            &self
                .annotations_tree()?
                .remove(index_key)?
                .ok_or(Apologize::AnnotationNotFound { id: id.to_owned() })?,
        )?)
    }

    /// Delete snippet from database
    pub fn delete_annotation(&self, id: &AnnotationID) -> color_eyre::Result<Annotation> {
        let annotation = self.delete_from_annotations_tree(id)?;
        let mut tag_batch = sled::Batch::default();
        for tag in &annotation.tags {
            self.delete_from_tag(tag.as_bytes(), id, &mut tag_batch)?;
        }
        self.tags_tree()?.apply_batch(tag_batch)?;
        Ok(annotation)
    }

    /// Retrieve annotations tagged with a given tag
    pub fn get_tagged_annotations(&self, tag: &str) -> color_eyre::Result<Vec<AnnotationID>> {
        utils::split_ids(
            &self
                .tags_tree()?
                .get(tag.as_bytes())?
                .ok_or(Apologize::TagNotFound {
                    tag: tag.to_owned(),
                })?,
        )
    }

    /// Retrieve an annotation by ID
    pub fn get_annotation(&self, id: &AnnotationID) -> color_eyre::Result<Annotation> {
        let index_key = id.as_bytes();
        Ok(Annotation::deserialize(
            &self
                .annotations_tree()?
                .get(index_key)?
                .ok_or(Apologize::AnnotationNotFound { id: id.to_owned() })?,
        )?)
    }

    /// Retrieve annotations by IDs
    pub(crate) fn get_annotations(
        &self,
        ids: &[AnnotationID],
    ) -> color_eyre::Result<Vec<Annotation>> {
        ids.iter().map(|i| self.get_annotation(i)).collect()
    }
    /// Retrieve annotations within a certain date range
    /// If `include_updated` is true, looks at the Updated date rather than the Created date
    pub fn get_annotations_in_date_range(
        &self,
        from_date: DateTime<Utc>,
        to_date: DateTime<Utc>,
        include_updated: bool,
    ) -> color_eyre::Result<Vec<Annotation>> {
        Ok(self
            .annotations_tree()?
            .iter()
            .filter_map(|x| x.ok())
            .map(|(_, annotation)| Annotation::deserialize(&annotation))
            .filter_map(|x| x.ok())
            .filter(|annotation| {
                if include_updated {
                    from_date <= annotation.updated && annotation.updated < to_date
                } else {
                    from_date <= annotation.created && annotation.created < to_date
                }
            })
            .collect())
    }
}
