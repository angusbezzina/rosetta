use std::collections::BTreeSet;

use rosetta_glossary::{
    Glossary, GlossarySource, ValidationContext, render_glossary, validate_glossary,
};

#[test]
fn an_empty_starter_is_valid_and_does_not_claim_domain_knowledge()
-> Result<(), Box<dyn std::error::Error>> {
    let text = r#"{
        "schema": "rosetta.glossary/v1",
        "context": "this repository",
        "purpose": "Terms in this repository await human review.",
        "sections": [],
        "terms": []
    }"#;
    let source: GlossarySource = serde_json::from_str(text)?;
    let context = ValidationContext {
        rule_ids: BTreeSet::new(),
        principle_ids: BTreeSet::new(),
        rule_source: None,
        principle_source: None,
        doc_exists: Box::new(|_| false),
        code_names: None,
        known_contexts: BTreeSet::new(),
    };
    assert!(validate_glossary(&source, &context).is_empty());
    let glossary: Glossary = serde_json::from_str(text)?;
    let markdown = render_glossary(&glossary);
    assert!(markdown.contains("No domain terms have been reviewed yet"));
    assert!(!markdown.contains("_Avoid_"));
    Ok(())
}
