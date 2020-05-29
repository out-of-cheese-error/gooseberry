use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;
use crate::utils;
use chrono::{DateTime, Utc, MIN_DATE};
use hypothesis::annotations::Annotation;
use hypothesis::AnnotationID;
use std::path::Path;

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
    pub(crate) fn set_merge(&self) -> color_eyre::Result<()> {
        self.tags_tree()?.set_merge_operator(merge_index);
        Ok(())
    }

    /// (re)sets date of last sync to way in the past
    pub fn reset_sync_date(&self) -> color_eyre::Result<()> {
        self.db
            .insert("last_sync_date", MIN_DATE.to_string().as_bytes())?;
        Ok(())
    }

    /// Update last sync date after sync
    fn set_sync_date(&self, date: DateTime<Utc>) -> color_eyre::Result<()> {
        self.db
            .insert("last_sync_date", date.to_string().as_bytes())?;
        Ok(())
    }

    /// Tree storing annotation id: annotation
    fn annotations_tree(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("annotations")?)
    }

    /// Tree storing tag: ( annotation IDs ...)
    fn tags_tree(&self) -> color_eyre::Result<sled::Tree> {
        Ok(self.db.open_tree("tags")?)
    }

    /// Add an annotation index to each of the tags it's associated with
    pub fn add_to_tags(&self, tags: &[String], annotation_key: &[u8]) -> color_eyre::Result<()> {
        for tag in tags {
            let tag_key = tag.as_bytes();
            self.tags_tree()?
                .merge(tag_key.to_vec(), annotation_key.to_vec())?;
        }
        Ok(())
    }

    /// Add an annotation to the annotations tree
    fn add_annotation(&self, annotation: &Annotation) -> color_eyre::Result<()> {
        let annotation_bytes = bincode::serialize(annotation)?;
        let annotation_key = annotation.id.as_bytes();
        self.add_to_tags(&annotation.tags, annotation_key)?;
        self.annotations_tree()?
            .insert(annotation_key, annotation_bytes)?;
        Ok(())
    }

    /// Delete an annotation index from the tag tree
    fn delete_from_tag(
        &mut self,
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
    fn delete_from_annotations_tree(
        &mut self,
        id: &AnnotationID,
    ) -> color_eyre::Result<Annotation> {
        let index_key = id.as_bytes();
        Ok(bincode::deserialize(
            &self
                .annotations_tree()?
                .remove(index_key)?
                .ok_or(Apologize::AnnotationNotFound { id: id.to_owned() })?,
        )?)
    }

    /// Delete snippet from database
    pub fn delete_annotation(&mut self, id: &AnnotationID) -> color_eyre::Result<Annotation> {
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
        Ok(bincode::deserialize(
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
    pub fn list_annotations_in_date_range(
        &self,
        from_date: DateTime<Utc>,
        to_date: DateTime<Utc>,
        include_updated: bool,
    ) -> color_eyre::Result<Vec<Annotation>> {
        Ok(self
            .annotations_tree()?
            .iter()
            .filter_map(|x| x.ok())
            .map(|(_, annotation)| bincode::deserialize::<Annotation>(&annotation))
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
