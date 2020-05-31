use hypothesis::annotations::{Annotation, AnnotationMaker};

use crate::gooseberry::Gooseberry;
use crate::utils;
use crate::utils::EMPTY_TAG;

impl Gooseberry {
    /// Add a tag to list of annotations
    pub async fn add_tag_to_annotations(
        &self,
        annotations: Vec<Annotation>,
        new_tag: &str,
    ) -> color_eyre::Result<()> {
        let mut update_ids = Vec::with_capacity(annotations.len());
        let mut updaters = Vec::with_capacity(annotations.len());
        let mut tag_batch = sled::Batch::default();
        for annotation in annotations {
            let mut annotation = annotation;
            if annotation.tags.contains(&new_tag.to_string()) {
                // tag already present
                continue;
            }
            let annotation_key = annotation.id.as_bytes();
            if annotation.tags.is_empty() {
                self.delete_from_tag(EMPTY_TAG.as_bytes(), &annotation.id, &mut tag_batch)?;
            }
            annotation.tags.push(new_tag.to_owned());
            self.add_to_tag(new_tag.as_bytes(), annotation_key)?;
            update_ids.push(annotation.id);
            updaters.push(AnnotationMaker {
                tags: Some(annotation.tags),
                ..Default::default()
            });
        }
        self.tag_to_annotations()?.apply_batch(tag_batch)?;
        self.api.update_annotations(&update_ids, &updaters).await?;
        Ok(())
    }

    /// Delete a tag from a list of annotations
    pub async fn delete_tag_from_annotations(
        &self,
        annotations: Vec<Annotation>,
        remove_tag: &str,
    ) -> color_eyre::Result<()> {
        let mut tag_batch = sled::Batch::default();
        let mut annotation_batch = sled::Batch::default();
        let mut update_ids = Vec::with_capacity(annotations.len());
        let mut updaters = Vec::with_capacity(annotations.len());
        for annotation in annotations {
            let mut annotation = annotation;
            if !annotation.tags.contains(&remove_tag.to_string()) {
                // tag not present
                continue;
            }
            let annotation_key = annotation.id.as_bytes();
            annotation.tags.retain(|t| t != remove_tag);
            annotation_batch.insert(annotation_key, utils::join_ids(&annotation.tags)?);
            self.delete_from_tag(remove_tag.as_bytes(), &annotation.id, &mut tag_batch)?;
            if annotation.tags.is_empty() {
                self.add_to_tag(EMPTY_TAG.as_bytes(), annotation_key)?;
            }
            update_ids.push(annotation.id);
            updaters.push(AnnotationMaker {
                tags: Some(annotation.tags),
                ..Default::default()
            });
        }
        self.api.update_annotations(&update_ids, &updaters).await?;
        self.annotation_to_tags()?.apply_batch(annotation_batch)?;
        self.tag_to_annotations()?.apply_batch(tag_batch)?;
        Ok(())
    }
}
