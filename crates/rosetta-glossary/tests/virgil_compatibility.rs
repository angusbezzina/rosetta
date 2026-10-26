use std::collections::BTreeSet;
use std::path::Path;

use rosetta_glossary::{
    Glossary, GlossarySource, ValidationContext, render_glossary_for, validate_glossary,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct References {
    rules: BTreeSet<String>,
    principles: BTreeSet<String>,
    docs: BTreeSet<String>,
}

#[test]
fn virgil_glossary_validates_renders_identically_and_keeps_its_schema()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/virgil");
    let text = std::fs::read_to_string(root.join("glossary.json"))?;
    let references: References =
        serde_json::from_str(&std::fs::read_to_string(root.join("references.json"))?)?;
    let context = ValidationContext {
        rule_ids: references.rules,
        principle_ids: references.principles,
        rule_source: Some("whetstone/rulebook.json".to_owned()),
        principle_source: Some("whetstone/rulebook.json".to_owned()),
        doc_exists: Box::new(move |path| references.docs.contains(path)),
        code_names: None,
        known_contexts: std::collections::BTreeSet::new(),
    };
    let source: GlossarySource = serde_json::from_str(&text)?;
    assert!(validate_glossary(&source, &context).is_empty());

    let glossary: Glossary = serde_json::from_str(&text)?;
    assert_eq!(glossary.terms.len(), 83);
    assert_eq!(glossary.schema, "virgil.glossary/v1");
    assert_eq!(rosetta_core::json_text::to_pretty_string(&glossary)?, text);
    assert_eq!(
        render_glossary_for(&glossary, ".virgil/glossary.json"),
        std::fs::read_to_string(root.join("GLOSSARY.md"))?
    );
    Ok(())
}
