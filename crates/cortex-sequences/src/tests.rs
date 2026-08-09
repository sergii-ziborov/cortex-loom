use std::collections::HashSet;

use super::{instantiate_template, templates};

#[test]
fn a_copy_is_editable_and_detached_from_its_template() {
    let graph = instantiate_template("discover-and-plan", "my-plan", "My plan").unwrap();
    assert_eq!(graph.id, "my-plan");
    assert_eq!(graph.name, "My plan");
    assert_eq!(graph.metadata["sequence.templateId"], "discover-and-plan");
    assert_eq!(graph.metadata["sequence.templateVersion"], "1.0.0");
    assert_eq!(graph.metadata["sequence.editable"], "true");
    assert_eq!(graph.revision, 0);
}

#[test]
fn catalog_ids_and_fingerprints_are_unique_and_stable() {
    let catalog = templates();
    assert_eq!(catalog.len(), 1);
    let ids: HashSet<_> = catalog.iter().map(|template| template.id).collect();
    assert_eq!(ids.len(), catalog.len());

    let first = instantiate_template("discover-and-plan", "one", "One").unwrap();
    let second = instantiate_template("discover-and-plan", "two", "Two").unwrap();
    assert_eq!(
        first.metadata["sequence.templateFingerprint"],
        second.metadata["sequence.templateFingerprint"]
    );
    assert_eq!(first.metadata["sequence.templateFingerprint"].len(), 64);
}

#[test]
fn versions_have_a_total_order() {
    let version = templates()[0].version;
    assert!(version < super::TemplateVersion::new(1, 1, 0));
    assert_eq!(version.to_string(), "1.0.0");
}
