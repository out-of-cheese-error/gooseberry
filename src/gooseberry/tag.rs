use crate::gooseberry::Gooseberry;
use crate::utils;
use hypothesis::annotations::{Annotation, AnnotationMaker};

impl Gooseberry {
    /// Add a tag to an existing annotation
    pub fn add_tag_to_annotation(
        &self,
        annotation: Annotation,
        new_tag: &str,
    ) -> color_eyre::Result<bool> {
        let mut annotation = annotation;
        if annotation
            .tags
            .as_deref()
            .unwrap_or_default()
            .contains(&new_tag.to_string())
        {
            // tag already present
            return Ok(false);
        }
        if let Some(ref mut x) = annotation.tags {
            x.push(new_tag.to_owned());
        }
        let annotation_key = annotation.id.as_bytes();
        self.add_to_tag(new_tag.as_bytes(), annotation_key)?;
        self.api.update_annotation(
            &annotation.id,
            &AnnotationMaker {
                tags: annotation.tags,
                ..Default::default()
            },
        )?;
        Ok(true)
    }

    /// Delete a tag from an existing annotation
    pub fn delete_tag_from_annotation(
        &self,
        annotation: Annotation,
        annotation_batch: &mut sled::Batch,
        remove_tag: &str,
        tag_batch: &mut sled::Batch,
    ) -> color_eyre::Result<bool> {
        let mut annotation = annotation;
        if annotation.tags.is_none() {
            // tag not present
            return Ok(false);
        }

        if let Some(ref mut x) = annotation.tags {
            if !x.contains(&remove_tag.to_string()) {
                return Ok(false);
            }
            x.retain(|t| t != remove_tag);
        }
        annotation_batch.insert(
            annotation.id.as_bytes(),
            utils::join_ids(&annotation.tags.as_deref().unwrap_or_default())?,
        );
        self.delete_from_tag(remove_tag.as_bytes(), &annotation.id, tag_batch)?;
        self.api.update_annotation(
            &annotation.id,
            &AnnotationMaker {
                tags: annotation.tags,
                ..Default::default()
            },
        )?;
        Ok(true)
    }
}
