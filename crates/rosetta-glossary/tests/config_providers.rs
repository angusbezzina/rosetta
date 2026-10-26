#[test]
fn configured_names_and_refs_are_available_to_glossary_validation()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::create_dir(root.path().join(".rosetta"))?;
    std::fs::write(
        root.path().join("catalog.json"),
        r#"{"rules":[{"id":"rule.ready"}],"documents":["docs/virtual.md"],"names":{"Widget":{}}}"#,
    )?;
    std::fs::write(
        root.path().join(".rosetta/rosetta.toml"),
        "[[refs.providers]]\nkind = 'rule'\npath = 'catalog.json'\npointer = '/rules'\n[[refs.providers]]\nkind = 'document'\npath = 'catalog.json'\npointer = '/documents'\n[[name_sources]]\nglob = 'catalog.json'\npointer = '/names'\n",
    )?;
    let context = rosetta_glossary::repo_context(root.path())?;
    assert!(context.rule_ids.contains("rule.ready"));
    assert!((context.doc_exists)("docs/virtual.md"));
    assert!(
        context
            .code_names
            .as_ref()
            .is_some_and(|names| names.resolves("Widget"))
    );
    Ok(())
}

#[test]
fn rulebook_uses_the_configured_local_path_and_documents_work_without_it()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::create_dir(root.path().join(".rosetta"))?;
    std::fs::create_dir(root.path().join("policy"))?;
    std::fs::create_dir(root.path().join("docs"))?;
    std::fs::write(root.path().join("docs/guide.md"), "# Guide\n")?;
    std::fs::write(
        root.path().join("policy/named.json"),
        r#"{"principles":[{"id":"principle.ready","statement":"Ready."}],"rules":[{"id":"rule.ready","statement":"Ready."}]}"#,
    )?;
    let config_file = root.path().join(".rosetta/rosetta.toml");
    std::fs::write(&config_file, "[refs]\nrulebook = 'policy/named.json'\n")?;
    let context = rosetta_glossary::repo_context(root.path())?;
    assert!(context.rule_ids.contains("rule.ready"));
    assert!(context.principle_ids.contains("principle.ready"));
    assert_eq!(context.rule_source.as_deref(), Some("policy/named.json"));

    let source = r#"{"schema":"rosetta.glossary/v1","context":"example","purpose":"Shared terms.","sections":[{"id":"main","title":"Main"}],"terms":[{"id":"term.ready","name":"Ready","section":"main","definition":"The reviewed state.","status":"proposed","refs":{"rules":["rule.ready"],"principles":["principle.ready"],"docs":["docs/guide.md"]}}]}"#;
    let loaded = rosetta_glossary::parse_glossary(source, &context);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    drop(context);

    std::fs::write(&config_file, "")?;
    let context = rosetta_glossary::repo_context(root.path())?;
    let loaded = rosetta_glossary::parse_glossary(source, &context);
    assert_eq!(loaded.problems.len(), 2, "{:?}", loaded.problems);
    assert!(
        loaded
            .problems
            .iter()
            .all(|problem| problem.why.contains("set refs.rulebook"))
    );
    assert!((context.doc_exists)("docs/guide.md"));
    assert!(!(context.doc_exists)("../outside.md"));
    Ok(())
}

#[test]
fn a_virgil_schema_file_is_not_an_implicit_code_name_source()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let schema = "packages/virgil-schema/src/generated/schemas.json";
    std::fs::create_dir_all(root.path().join("packages/virgil-schema/src/generated"))?;
    std::fs::create_dir(root.path().join(".rosetta"))?;
    std::fs::write(
        root.path().join(schema),
        r#"{"x-virgil-collections":{"channel":{}}}"#,
    )?;
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = 'sample-package'\nversion = '0.1.0'\n",
    )?;
    let names = rosetta_glossary::CodeNames::from_repo(root.path())?;
    assert!(!names.resolves("channel"));
    assert!(names.resolves("sample-package"));
    assert!(
        names
            .source_kinds()
            .contains("no JSON name source configured")
    );

    std::fs::write(
        root.path().join(".rosetta/rosetta.toml"),
        format!("[[name_sources]]\nglob = '{schema}'\npointer = '/x-virgil-collections'\n"),
    )?;
    let names = rosetta_glossary::CodeNames::from_repo(root.path())?;
    assert!(names.resolves("channel"));
    assert!(names.source_kinds().contains(schema));
    Ok(())
}
