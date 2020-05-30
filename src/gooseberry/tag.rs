use crate::gooseberry::cli::Filters;
use crate::gooseberry::Gooseberry;
use chrono::{MAX_DATE, MIN_DATE};
use hypothesis::annotations::{Annotation, AnnotationMaker, Selector};
use std::collections::HashSet;

impl Gooseberry {
    pub fn tag(
        &self,
        filters: &Filters,
        delete: bool,
        search: bool,
        tag: &str,
    ) -> color_eyre::Result<()> {
        let date = filters.from.unwrap_or_else(|| MIN_DATE.and_hms(0, 0, 0));
        let mut annotations: Vec<_> = self
            .get_annotations_in_date_range(date, MAX_DATE.and_hms(23, 59, 59), false)?
            .into_iter()
            .filter(|a| match &filters.url {
                Some(pattern) => a.uri.contains(pattern),
                None => true,
            })
            .filter(|a| match &filters.any {
                Some(pattern) => {
                    a.text.contains(pattern)
                        || a.uri.contains(pattern)
                        || a.tags.iter().any(|tag| tag.contains(pattern))
                        || a.target.iter().any(|target| {
                            target.selector.iter().any(|selector| match selector {
                                Selector::TextQuoteSelector(selector) => {
                                    selector.exact.contains(pattern)
                                }
                                _ => true,
                            })
                        })
                }
                None => true,
            })
            .collect();
        if search {
            let annotation_ids: HashSet<String> = Self::search(&annotations)?.collect();
            annotations = annotations
                .into_iter()
                .filter(|a| annotation_ids.contains(&a.id))
                .collect();
        }

        if delete {
            let mut tag_batch = sled::Batch::default();
            let mut annotation_batch = sled::Batch::default();
            for mut annotation in annotations {
                self.delete_tag_from_annotation(
                    &mut annotation,
                    &mut annotation_batch,
                    tag,
                    &mut tag_batch,
                )?;
            }
            self.annotations_tree()?.apply_batch(annotation_batch)?;
            self.tags_tree()?.apply_batch(tag_batch)?;
        } else {
            let mut annotation_batch = sled::Batch::default();
            for mut annotation in annotations {
                self.add_tag_to_annotation(&mut annotation, &mut annotation_batch, tag)?;
            }
            self.annotations_tree()?.apply_batch(annotation_batch)?;
        }

        Ok(())
    }

    /// Add a tag to an existing annotation
    fn add_tag_to_annotation(
        &self,
        annotation: &mut Annotation,
        annotation_batch: &mut sled::Batch,
        new_tag: &str,
    ) -> color_eyre::Result<()> {
        if annotation.tags.contains(&new_tag.to_string()) {
            return Ok(());
        }
        annotation.tags.push(new_tag.to_owned());
        let annotation_key = annotation.id.as_bytes();
        Self::insert_annotation(annotation_key, annotation, annotation_batch)?;
        self.add_to_tag(new_tag, annotation_key)?;
        self.api.update_annotation(
            &annotation.id,
            &AnnotationMaker {
                tags: annotation.tags.clone(),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// Delete a tag from an existing annotation
    fn delete_tag_from_annotation(
        &self,
        annotation: &mut Annotation,
        annotation_batch: &mut sled::Batch,
        remove_tag: &str,
        tag_batch: &mut sled::Batch,
    ) -> color_eyre::Result<()> {
        let new_tags = annotation
            .tags
            .iter()
            .filter(|t| t != &remove_tag)
            .cloned()
            .collect();
        if new_tags == annotation.tags {
            return Ok(());
        }
        annotation.tags = new_tags;
        Self::insert_annotation(annotation.id.as_bytes(), annotation, annotation_batch)?;
        self.delete_from_tag(remove_tag.as_bytes(), &annotation.id, tag_batch)?;
        self.api.update_annotation(
            &annotation.id,
            &AnnotationMaker {
                tags: annotation.tags.clone(),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// Replace an annotation's tags
    fn change_tags_in_annotation(
        &mut self,
        annotation: &mut Annotation,
        annotation_batch: &mut sled::Batch,
        changed_tags: &[String],
        tag_batch: &mut sled::Batch,
    ) -> color_eyre::Result<()> {
        let add_tags: Vec<_> = changed_tags
            .iter()
            .filter(|t| !annotation.tags.contains(t))
            .collect();
        let delete_tags: Vec<_> = annotation
            .tags
            .iter()
            .filter(|t| !changed_tags.contains(t))
            .cloned()
            .collect();
        if add_tags.is_empty() && delete_tags.is_empty() {
            // No change
            return Ok(());
        }
        annotation.tags = changed_tags.to_owned();
        let annotation_key = annotation.id.as_bytes();
        for new_tag in add_tags {
            self.add_to_tag(new_tag, annotation_key)?;
        }
        for remove_tag in delete_tags {
            self.delete_from_tag(remove_tag.as_bytes(), &annotation.id, tag_batch)?;
        }
        Self::insert_annotation(annotation_key, annotation, annotation_batch)?;
        self.api.update_annotation(
            &annotation.id,
            &AnnotationMaker {
                tags: annotation.tags.clone(),
                ..Default::default()
            },
        )?;
        Ok(())
    }
}
