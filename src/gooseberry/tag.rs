use crate::gooseberry::Gooseberry;
use crate::utils;
use hypothesis::annotations::{Annotation, AnnotationMaker};

impl Gooseberry {
    /// Add a tag to list of annotations
    pub async fn add_tag_to_annotations(
        &self,
        annotations: Vec<Annotation>,
        new_tag: &str,
    ) -> color_eyre::Result<()> {
        let mut update_ids = Vec::with_capacity(annotations.len());
        let mut updaters = Vec::with_capacity(annotations.len());
        for annotation in annotations {
            let mut annotation = annotation;
            if annotation
                .tags
                .as_deref()
                .unwrap_or_default()
                .contains(&new_tag.to_string())
            {
                // tag already present
                return Ok(());
            }
            if let Some(ref mut x) = annotation.tags {
                x.push(new_tag.to_owned());
            }
            let annotation_key = annotation.id.as_bytes();
            self.add_to_tag(new_tag.as_bytes(), annotation_key)?;
            update_ids.push(annotation.id);
            updaters.push(AnnotationMaker {
                tags: annotation.tags,
                ..Default::default()
            });
        }
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
            if annotation.tags.is_none() {
                // tag not present
                continue;
            }

            if let Some(ref mut x) = annotation.tags {
                if !x.contains(&remove_tag.to_string()) {
                    // tag not present
                    continue;
                }
                x.retain(|t| t != remove_tag);
            }
            annotation_batch.insert(
                annotation.id.as_bytes(),
                utils::join_ids(&annotation.tags.as_deref().unwrap_or_default())?,
            );
            self.delete_from_tag(remove_tag.as_bytes(), &annotation.id, &mut tag_batch)?;
            update_ids.push(annotation.id);
            updaters.push(AnnotationMaker {
                tags: annotation.tags,
                ..Default::default()
            });
        }
        self.api.update_annotations(&update_ids, &updaters).await?;
        self.annotation_to_tags()?.apply_batch(annotation_batch)?;
        self.tag_to_annotations()?.apply_batch(tag_batch)?;
        Ok(())
    }
}
