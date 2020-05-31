use hypothesis::annotations::{Annotation, AnnotationMaker};

use crate::gooseberry::Gooseberry;

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
            annotation.tags.push(new_tag.to_owned());
            update_ids.push(annotation.id);
            updaters.push(AnnotationMaker {
                tags: Some(annotation.tags),
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
        let mut update_ids = Vec::with_capacity(annotations.len());
        let mut updaters = Vec::with_capacity(annotations.len());
        for annotation in annotations {
            let mut annotation = annotation;
            annotation.tags.retain(|t| t != remove_tag);
            update_ids.push(annotation.id);
            updaters.push(AnnotationMaker {
                tags: Some(annotation.tags),
                ..Default::default()
            });
        }
        self.api.update_annotations(&update_ids, &updaters).await?;
        Ok(())
    }
}
