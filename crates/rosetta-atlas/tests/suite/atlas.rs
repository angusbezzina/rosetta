//! The atlas: a red fixture per validator rule and per check finding,
//! derivation and sync on the fixture repo, and the golden bundle and
//! Mermaid sources the viewer's parse test reads.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::common::{
    TestResult, edit, fixture_copy, fixture_copy_of, fixture_repo, fixtures, repo_root,
};
use rosetta_atlas::atlas::assertions::{self, ASSERTIONS_SCHEMA, Rule, RuleSet, Status};
use rosetta_atlas::atlas::change::{self, INTENT_SCHEMA, Intent, IntentProposal};
use rosetta_atlas::atlas::focus::{self, Level, Query};
use rosetta_atlas::atlas::model::{Capability, CapabilityStatus, EdgeKind, ImplementationTarget};
use rosetta_atlas::atlas::{self, Atlas, AtlasContext, derive, validate_atlas};
use rosetta_atlas::cli::{AtlasCommand, Host, Outcome, RenderFormat, run_atlas};
use rosetta_atlas::curation::{self, Change, PROPOSAL_SCHEMA, Proposal};
use rosetta_atlas::json_text;

fn fixture_context(root: &Path) -> Result<AtlasContext<'_>, Box<dyn std::error::Error>> {
    Ok(atlas::repo_context(root)?)
}

fn read_atlas(path: &Path) -> Result<Atlas, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

#[test]
fn the_fixture_atlas_is_valid() -> TestResult {
    let root = fixture_repo();
    let errors = validate_atlas(&atlas::load_atlas(&root)?, &fixture_context(&root)?);
    assert_eq!(errors, vec![]);
    Ok(())
}

#[test]
fn focused_depth_two_follows_a_called_service_to_its_read_store() -> TestResult {
    let dir = fixture_copy()?;
    let mut bundle = atlas::bundle_repo(dir.path())?;
    let mut read = bundle
        .edges
        .iter()
        .find(|edge| edge.from == "door" && edge.to == "store")
        .cloned()
        .ok_or("door to store edge missing")?;
    read.kind = EdgeKind::Reads;
    bundle.edges.push(read);
    let view = focus::select(
        &bundle,
        Query {
            roots: vec!["web".to_owned()],
            depth: 2,
            kinds: vec![EdgeKind::Calls, EdgeKind::Reads],
            ..Query::default()
        },
    )?;
    assert!(view.nodes.iter().any(|node| node.id == "store"));
    assert!(
        view.edges
            .iter()
            .any(|edge| edge.from == "door" && edge.to == "store" && edge.kind == EdgeKind::Reads)
    );
    let text = focus::render_context(&view, 1500)?;
    assert!(text.chars().count() <= 1500);
    assert!(text.contains(&view.reference));
    assert!(text.contains("Omitted by view:"));
    Ok(())
}

#[test]
fn assertions_separate_broken_paths_forbidden_and_missing_dependencies_from_unknown_calls()
-> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let source = root.join(".rosetta/atlas.json");
    let mut model = atlas::load_atlas(root)?;
    model.capabilities.push(Capability {
        id: "explain".into(),
        name: "Explain a request".into(),
        purpose: "Show a request's path.".into(),
        status: CapabilityStatus::Settled,
        terms: vec!["term.door".into()],
        parts: vec!["web".into()],
        implementation: vec![ImplementationTarget {
            part: "web".into(),
            path: "apps/web/src/missing.ts".into(),
            symbol: None,
        }],
    });
    std::fs::write(source, json_text::to_pretty_string(&model)?)?;
    let rules = RuleSet {
        schema: ASSERTIONS_SCHEMA.into(),
        rules: vec![
            Rule::ImplementationExists {
                id: "link".into(),
                capability: "explain".into(),
                part: Some("web".into()),
                term: Some("term.door".into()),
            },
            Rule::Dependencies {
                id: "forbidden".into(),
                part: "web".into(),
                allowed: None,
                forbidden: vec!["packages-ui".into()],
                term: None,
            },
            Rule::RequiredRelationship {
                id: "removed".into(),
                from: "packages-ui".into(),
                to: "apps-web".into(),
                relation: atlas::model::EdgeKind::Depends,
                term: None,
            },
            Rule::RequiredRelationship {
                id: "present".into(),
                from: "apps-web".into(),
                to: "packages-ui".into(),
                relation: atlas::model::EdgeKind::Depends,
                term: None,
            },
            Rule::RequiredRelationship {
                id: "runtime".into(),
                from: "web".into(),
                to: "door".into(),
                relation: atlas::model::EdgeKind::Calls,
                term: Some("term.door".into()),
            },
        ],
    };
    std::fs::write(
        root.join(".rosetta/assertions.json"),
        serde_json::to_vec_pretty(&rules)?,
    )?;
    let report = assertions::verify(root, ".rosetta")?;
    assert_eq!(
        report
            .findings
            .iter()
            .map(|finding| finding.status)
            .collect::<Vec<_>>(),
        vec![
            Status::Violation,
            Status::Violation,
            Status::Violation,
            Status::Pass,
            Status::Unknown
        ]
    );
    assert_eq!(report.findings[0].term.as_deref(), Some("term.door"));
    assert!(
        report.findings[1]
            .evidence
            .iter()
            .any(|source| source == "apps/web/package.json")
    );
    assert!(
        report.findings[2]
            .repair
            .as_deref()
            .is_some_and(|repair| repair.contains("packages/ui/package.json"))
    );
    assert!(report.findings[4].message.contains("no completeness proof"));
    let human = assertions::render(&report);
    for finding in &report.findings {
        for evidence in &finding.evidence {
            assert!(human.contains(evidence));
        }
    }
    let host = Host {
        cwd: root.to_path_buf(),
        path_env: None,
    };
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run_atlas(
            &AtlasCommand::Verify {
                strict: false,
                json: false
            },
            &host,
            &mut out,
            &mut err
        )?,
        Outcome::Passed
    );
    assert_eq!(
        run_atlas(
            &AtlasCommand::Verify {
                strict: true,
                json: true
            },
            &host,
            &mut out,
            &mut err
        )?,
        Outcome::Failed
    );
    Ok(())
}

#[test]
fn bundle_rejects_stale_manifest_evidence_until_sync() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let manifest = root.join("apps/web/package.json");
    let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&manifest)?)?;
    value["dependencies"]
        .as_object_mut()
        .ok_or("dependencies")?
        .remove("@fixture/ui");
    std::fs::write(&manifest, serde_json::to_string_pretty(&value)?)?;

    let error = atlas::bundle_repo(root)
        .err()
        .ok_or("stale bundle should fail")?
        .to_string();
    assert!(error.contains("run rosetta atlas sync"), "{error}");
    assert!(
        atlas::check_atlas(root)?
            .iter()
            .any(|finding| finding.contains("derived"))
    );
    atlas::sync_atlas(root)?;
    let refreshed = atlas::bundle_repo(root)?;
    assert!(
        !refreshed
            .edges
            .iter()
            .any(|edge| edge.from == "apps-web" && edge.to == "packages-ui")
    );
    Ok(())
}

#[test]
fn changed_separates_observed_matching_unexpected_and_unverifiable_intent() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let committed = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.com",
            "commit",
            "-qm",
            "baseline",
        ])
        .status()?;
    assert!(committed.success());
    std::fs::create_dir(root.join("packages/helper"))?;
    std::fs::write(
        root.join("packages/helper/package.json"),
        "{\"name\":\"@fixture/helper\",\"private\":true}",
    )?;
    for (path, dependency) in [
        ("apps/web/package.json", "@fixture/helper"),
        ("packages/ui/package.json", "@fixture/web"),
    ] {
        let file = root.join(path);
        let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file)?)?;
        value["dependencies"][dependency] = serde_json::json!("workspace:*");
        std::fs::write(file, serde_json::to_string_pretty(&value)?)?;
    }
    let atlas_source = root.join(".rosetta/atlas.json");
    let mut model = atlas::load_atlas(root)?;
    model
        .nodes
        .iter_mut()
        .find(|node| node.id == "door")
        .ok_or("door")?
        .summary = Some("Answers one request at a time.".into());
    std::fs::write(&atlas_source, json_text::to_pretty_string(&model)?)?;

    let first = change::changed(root, ".rosetta", "HEAD", None)?;
    let proposal = IntentProposal {
        schema: INTENT_SCHEMA.into(),
        base_commit: first.base_commit.clone(),
        base_model_revision: first.base_model_revision.clone(),
        intents: vec![
            Intent::Relationship {
                from: "apps-web".into(),
                to: "packages-helper".into(),
                relation: atlas::model::EdgeKind::Depends,
                present: true,
                rationale: "Use shared helper".into(),
            },
            Intent::Relationship {
                from: "web".into(),
                to: "door".into(),
                relation: atlas::model::EdgeKind::Calls,
                present: true,
                rationale: "Runtime request path".into(),
            },
            Intent::Claim {
                text: "Replies in under 100 ms".into(),
                rationale: "Desired experience".into(),
            },
            Intent::Responsibility {
                part: "door".into(),
                value: "Answers one request at a time.".into(),
                rationale: "Correct the model".into(),
            },
        ],
    };
    let report = change::changed(root, ".rosetta", "HEAD", Some(&proposal))?;
    assert!(
        report
            .changed_paths
            .contains(&"packages/helper/package.json".into())
    );
    assert!(
        report
            .impacts
            .iter()
            .any(|impact| impact.path == "apps/web/package.json"
                && impact.parts.iter().any(|part| part.id == "web"))
    );
    assert_eq!(
        report
            .proposed
            .iter()
            .map(|item| item.status.as_str())
            .collect::<Vec<_>>(),
        vec!["implemented", "unknown", "unknown", "declared_in_model"]
    );
    assert_eq!(
        report.proposed[0].evidence.as_deref(),
        Some("apps/web/package.json")
    );
    assert!(
        report
            .unexpected
            .iter()
            .any(|delta| delta.category == "relationship"
                && delta.id.contains("packages-ui -> apps-web"))
    );
    assert!(report.semantic.iter().any(|delta| delta.category == "part"
        && delta.id == "door"
        && delta.field == "responsibility"));
    let markdown = change::render_changed(&report, true);
    assert!(markdown.contains("```mermaid"));
    for detail in [
        "apps-web depends on packages-helper",
        "web calls door",
        "Replies in under 100 ms",
        "responsibility of door = \"Answers one request at a time.\"",
    ] {
        assert!(markdown.contains(detail), "missing {detail} in {markdown}");
        assert!(change::render_changed(&report, false).contains(detail));
    }
    let stale = IntentProposal {
        base_commit: "incorrect".into(),
        ..proposal
    };
    assert!(change::changed(root, ".rosetta", "HEAD", Some(&stale)).is_err());
    Ok(())
}

#[test]
fn changed_does_not_credit_a_relationship_that_predates_a_stale_baseline_model() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let file = root.join("packages/ui/package.json");
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file)?)?;
    manifest["dependencies"]["@fixture/web"] = serde_json::json!("workspace:*");
    std::fs::write(file, serde_json::to_string_pretty(&manifest)?)?;
    let committed = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.com",
            "commit",
            "-qam",
            "baseline with stale Atlas",
        ])
        .status()?;
    assert!(committed.success());
    let first = change::changed(root, ".rosetta", "HEAD", None)?;
    let proposal = IntentProposal {
        schema: INTENT_SCHEMA.into(),
        base_commit: first.base_commit,
        base_model_revision: first.base_model_revision,
        intents: vec![Intent::Relationship {
            from: "packages-ui".into(),
            to: "apps-web".into(),
            relation: atlas::model::EdgeKind::Depends,
            present: true,
            rationale: "Introduce web dependency".into(),
        }],
    };
    let report = change::changed(root, ".rosetta", "HEAD", Some(&proposal))?;
    assert_ne!(report.proposed[0].status, "implemented");
    assert!(!report.semantic.iter().any(|delta| {
        delta.origin == "derived" && delta.id.contains("packages-ui -> apps-web")
    }));
    Ok(())
}

#[test]
fn curation_previews_applies_and_undoes_only_without_intervening_edits() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let source = root.join(".rosetta/atlas.json");
    let original = std::fs::read(&source)?;
    let baseline = atlas::bundle_repo(root)?
        .snapshot
        .ok_or("snapshot")?
        .model_revision;
    let proposal = Proposal {
        schema: PROPOSAL_SCHEMA.into(),
        base_model_revision: baseline,
        rationale: "Correct the service responsibility".into(),
        changes: vec![Change::SetNodeSummary {
            id: "door".into(),
            value: "Answers requests and returns one response.".into(),
        }],
    };
    let preview = curation::preview(root, ".rosetta", &proposal)?;
    assert_eq!(preview.changes.len(), 1);
    assert_eq!(std::fs::read(&source)?, original);
    let (_applied, undo) = curation::apply(root, ".rosetta", &proposal)?;
    assert!(
        std::fs::read_to_string(&source)?.contains("Answers requests and returns one response.")
    );
    assert!(
        std::fs::read_to_string(root.join("ATLAS.md"))?
            .contains("Answers requests and returns one response.")
    );
    curation::undo(root, undo).map_err(|(error, _)| error)?;
    assert_eq!(std::fs::read(&source)?, original);

    let (_, undo) = curation::apply(root, ".rosetta", &proposal)?;
    std::fs::write(&source, "external editor change")?;
    let (error, _) = curation::undo(root, undo)
        .err()
        .ok_or("undo should conflict")?;
    assert!(error.to_string().contains("changed"));
    assert_eq!(std::fs::read_to_string(&source)?, "external editor change");
    Ok(())
}

#[test]
fn curation_validates_term_edits_and_rejects_a_stale_baseline() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let baseline = atlas::bundle_repo(root)?
        .snapshot
        .ok_or("snapshot")?
        .model_revision;
    let proposal = Proposal {
        schema: PROPOSAL_SCHEMA.into(),
        base_model_revision: baseline,
        rationale: String::new(),
        changes: vec![Change::SetTermDefinition {
            id: "term.door".into(),
            value: "The service that answers a request.".into(),
        }],
    };
    let (_, undo) = curation::apply(root, ".rosetta", &proposal)?;
    assert!(
        std::fs::read_to_string(root.join("GLOSSARY.md"))?
            .contains("The service that answers a request.")
    );
    assert!(curation::apply(root, ".rosetta", &proposal).is_err());
    curation::undo(root, undo).map_err(|(error, _)| error)?;
    let bad = Proposal {
        changes: vec![Change::SetTermDefinition {
            id: "term.door".into(),
            value: "x".repeat(300),
        }],
        ..proposal
    };
    assert!(curation::preview(root, ".rosetta", &bad).is_err());
    Ok(())
}

#[test]
fn a_curated_capability_spans_packages_and_reports_missing_implementation() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let mut model = atlas::load_atlas(root)?;
    let capability = Capability {
        id: "explain-a-request".to_owned(),
        name: "Explain a request".to_owned(),
        purpose: "Show where a request enters and how it reaches the door.".to_owned(),
        status: CapabilityStatus::Settled,
        terms: vec!["term.door".to_owned()],
        parts: vec!["web".to_owned(), "door".to_owned()],
        implementation: vec![
            ImplementationTarget {
                part: "web".to_owned(),
                path: "apps/web/src/index.ts".to_owned(),
                symbol: None,
            },
            ImplementationTarget {
                part: "door".to_owned(),
                path: "crates/door/src/main.rs".to_owned(),
                symbol: Some("main".to_owned()),
            },
        ],
    };
    model.capabilities.push(capability.clone());
    let path = root.join(".rosetta/atlas.json");
    std::fs::write(&path, json_text::to_pretty_string(&model)?)?;
    assert!(atlas::check_atlas(root)?.is_empty());
    assert_eq!(
        atlas::bundle_repo(root)?.capabilities,
        vec![capability.clone()]
    );
    let revision = atlas::bundle_repo(root)?
        .snapshot
        .ok_or("snapshot")?
        .model_revision;
    let link = Proposal {
        schema: PROPOSAL_SCHEMA.into(),
        base_model_revision: revision,
        rationale: "Add the package owner".into(),
        changes: vec![Change::SetCapabilityPart {
            capability: capability.id.clone(),
            part: "packages-ui".into(),
            present: true,
        }],
    };
    assert_eq!(curation::preview(root, ".rosetta", &link)?.changes.len(), 1);
    assert!(atlas::bundle_repo(root)?.target_exists["crates/door/src/main.rs"]);
    assert!(!atlas::sync_atlas(root)?);

    model.capabilities[0].name = "Trace a request".to_owned();
    std::fs::write(&path, json_text::to_pretty_string(&model)?)?;
    assert!(!atlas::sync_atlas(root)?);
    assert_eq!(
        atlas::load_atlas(root)?.capabilities[0].name,
        "Trace a request"
    );

    std::fs::remove_file(root.join("crates/door/src/main.rs"))?;
    let findings = atlas::check_atlas(root)?;
    assert!(
        findings.iter().any(|finding| finding
            .contains("capabilities[explain-a-request].implementation[1].path")
            && finding.contains("repair the link")),
        "{findings:?}"
    );
    assert!(!atlas::bundle_repo(root)?.target_exists["crates/door/src/main.rs"]);
    Ok(())
}

#[test]
fn a_focused_view_reopens_with_the_same_selection_and_rejects_a_dirty_change() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let mut model = atlas::load_atlas(root)?;
    model.capabilities.push(Capability {
        id: "request-path".to_owned(),
        name: "Request path".to_owned(),
        purpose: "Connect the surface to the door.".to_owned(),
        status: CapabilityStatus::Settled,
        terms: vec!["term.door".to_owned()],
        parts: vec!["web".to_owned(), "door".to_owned()],
        implementation: vec![ImplementationTarget {
            part: "door".to_owned(),
            path: "crates/door/src/main.rs".to_owned(),
            symbol: None,
        }],
    });
    std::fs::write(
        root.join(".rosetta/atlas.json"),
        json_text::to_pretty_string(&model)?,
    )?;
    let bundle = atlas::bundle_repo(root)?;
    let query = Query {
        capability: Some("request-path".to_owned()),
        depth: 2,
        max_nodes: 3,
        ..Query::default()
    };
    let view = focus::select(&bundle, query.clone())?;
    golden("focus.json", &json_text::to_pretty_string(&view)?)?;
    assert!(view.nodes.iter().any(|node| node.id == "web"));
    assert!(view.nodes.iter().any(|node| node.id == "door"));
    assert!(view.terms.iter().any(|term| term.id == "term.door"));
    assert!(view.omissions.nodes_over_budget > 0);
    let reopened = focus::reopen(&bundle, &view.reference)?;
    assert_eq!(view.id, reopened.id);
    assert_eq!(view.nodes, reopened.nodes);
    assert_eq!(view.edges, reopened.edges);

    // An edit outside this focus changes the snapshot, but not its placement
    // or the identities of the parts still in the selected diagram.
    let mut unrelated = model.clone();
    unrelated
        .nodes
        .iter_mut()
        .find(|node| node.id == "edge")
        .ok_or("unrelated fixture node")?
        .summary = Some("The unrelated live transport.".to_owned());
    std::fs::write(
        root.join(".rosetta/atlas.json"),
        json_text::to_pretty_string(&unrelated)?,
    )?;
    let changed_model = atlas::bundle_repo(root)?;
    let same_focus = focus::select(&changed_model, query)?;
    assert_eq!(view.diagram, same_focus.diagram);
    assert_eq!(
        view.nodes.iter().map(|node| &node.id).collect::<Vec<_>>(),
        same_focus
            .nodes
            .iter()
            .map(|node| &node.id)
            .collect::<Vec<_>>()
    );

    let implementation = focus::select(
        &bundle,
        Query {
            capability: Some("request-path".to_owned()),
            level: Level::Implementation,
            depth: 0,
            ..Query::default()
        },
    )?;
    assert_eq!(implementation.implementation_links.len(), 1);
    assert_eq!(implementation.implementation_links[0].part, "door");
    assert!(implementation.diagram.contains(" -.-> "));

    let path_view = focus::select(
        &bundle,
        Query {
            paths: vec!["crates/door/src/main.rs".to_owned()],
            level: Level::Implementation,
            ..Query::default()
        },
    )?;
    assert!(
        path_view
            .nodes
            .iter()
            .any(|node| node.path.as_deref() == Some("crates/door/src/main.rs"))
    );
    assert!(
        focus::select(
            &bundle,
            Query {
                roots: vec!["not-a-node".to_owned()],
                ..Query::default()
            }
        )
        .is_err()
    );
    std::fs::write(
        root.join("crates/door/src/main.rs"),
        "// changed while the view was open\n",
    )?;
    let changed = atlas::bundle_repo(root)?;
    assert!(matches!(
        focus::reopen(&changed, &view.reference),
        Err(focus::FocusError::Stale { .. })
    ));
    Ok(())
}

#[test]
fn markdown_export_uses_the_bundle_and_check_catches_a_hand_edit() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    assert!(atlas::export::export_repo(root)?);
    let path = root.join("ATLAS.md");
    let rendered = std::fs::read_to_string(&path)?;
    assert!(rendered.starts_with("<!-- Generated by rosetta atlas export"));
    assert!(rendered.contains("```mermaid\n"));
    assert!(rendered.contains("| ID | Part | Family | Tier | Path | Summary | Last change |"));
    assert!(!rendered.contains("\nclick "));
    assert!(atlas::check_atlas(root)?.is_empty());
    assert!(!atlas::export::export_repo(root)?);
    std::fs::write(&path, format!("{rendered}\nHand edit.\n"))?;
    let findings = atlas::check_atlas(root)?;
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("differs from the generated atlas"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("--- ATLAS.md (committed)"))
    );
    Ok(())
}

#[test]
fn the_legacy_source_and_schema_remain_readable() -> TestResult {
    let root = fixtures().join("legacy");
    let atlas_path = root.join(".virgil/atlas.json");
    assert_eq!(
        rosetta_atlas::repo::find_root_in(&atlas_path, rosetta_atlas::repo::VIRGIL_DIR),
        Some(root.clone())
    );
    let model = read_atlas(&atlas_path)?;
    assert_eq!(model.schema, atlas::model::VIRGIL_ATLAS_SCHEMA);
    assert!(validate_atlas(&model, &fixture_context(&root)?).is_empty());
    Ok(())
}

#[test]
fn the_virgil_atlas_round_trips_byte_for_byte() -> TestResult {
    let path = fixtures().join("virgil/atlas.json");
    let text = std::fs::read_to_string(path)?;
    let model: Atlas = serde_json::from_str(&text)?;
    assert_eq!(model.schema, atlas::model::VIRGIL_ATLAS_SCHEMA);
    assert_eq!(json_text::to_pretty_string(&model)?, text);
    Ok(())
}

#[test]
fn each_validator_rule_has_a_red_fixture() -> TestResult {
    let cases = [
        (
            "schema",
            "schema: must be \"rosetta.atlas/v1\", got \"virgil.atlas/v0\"",
        ),
        (
            "node-id-not-kebab",
            "nodes[21].id: \"Odd_Node\" must be kebab-case",
        ),
        (
            "duplicate-node",
            "nodes[door]: the id is used by more than one node",
        ),
        (
            "missing-parent",
            "nodes[orphan].parent: \"nowhere\" is not a node",
        ),
        (
            "parent-cycle",
            "nodes[loop-a].parent: the parent chain loops back to loop-a",
        ),
        (
            "committed-file",
            "nodes[a-file].kind: files are walked live and never committed",
        ),
        (
            "missing-path",
            "nodes[gone].path: \"crates/gone\" does not exist",
        ),
        (
            "missing-term",
            "nodes[termless].refs.term: \"term.gone\" is not in .rosetta/glossary.json",
        ),
        (
            "missing-rule",
            "nodes[ruleless].refs.rules: \"rule.gone\" is not in whetstone/rulebook.json",
        ),
        (
            "missing-principle",
            "nodes[principleless].refs.principles: \"principle.gone\" is not in whetstone/rulebook.json",
        ),
        (
            "missing-doc",
            "nodes[docless].refs.docs: \"docs/gone.md\" does not exist",
        ),
        ("missing-edge-end", "edges[7].to: \"ghost\" is not a node"),
        (
            "bad-view-id",
            "views[1].id: \"send\" must be kebab-case, unique and not the id of a built view (overview, infra, records, data or connections)",
        ),
        (
            "missing-include",
            "views[parts].include: a custom view lists the nodes it shows",
        ),
        (
            "missing-steps",
            "views[send].steps: a flow view needs at least one step",
        ),
        (
            "declared-built",
            "views[map].kind: the overview view is built from the nodes and edges; remove it, and for a picture of your own declare a view of kind \"custom\" with an include list",
        ),
        (
            "missing-records-file",
            "records.file: \"packages/virgil-schema/gone.json\" does not exist",
        ),
        (
            "bad-area-id",
            "records.areas[2].id: \"other\" must be kebab-case, unique and not \"other\"",
        ),
        (
            "missing-view-node",
            "views[parts].include[5]: \"ghost\" is not a node",
        ),
    ];
    let root = fixture_repo();
    let context = fixture_context(&root)?;
    let files: BTreeSet<String> = std::fs::read_dir(fixtures().join("bad"))?
        .map(|entry| {
            entry.map(|e| {
                e.file_name()
                    .to_string_lossy()
                    .trim_end_matches(".json")
                    .to_owned()
            })
        })
        .collect::<Result<_, _>>()?;
    assert_eq!(
        files,
        cases.iter().map(|(name, _)| (*name).to_owned()).collect()
    );
    for (name, expected) in cases {
        let atlas = read_atlas(&fixtures().join(format!("bad/{name}.json")))?;
        let errors: Vec<String> = validate_atlas(&atlas, &context)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            errors.iter().any(|error| error == expected),
            "{name}: expected {expected:?} in {errors:?}"
        );
    }
    Ok(())
}

#[test]
fn an_unknown_field_is_refused() {
    let text = r#"{"schema":"virgil.atlas/v1","project":{"name":"x","repo":"y"},"nodes":[],"views":[],"note":"x"}"#;
    let parsed: Result<Atlas, _> = serde_json::from_str(text);
    assert!(parsed.is_err());
}

#[test]
fn each_check_finding_has_a_red_fixture() -> TestResult {
    let cases = [
        (
            "member-without-node",
            "nodes: crates/core (fixture-core) is a workspace member in Cargo.toml with no node \"crates-core\"; run task atlas:sync",
        ),
        (
            "stale-derived-node",
            "nodes[crates-retired].derived: Cargo.toml no longer declares it, so it is stale; run task atlas:sync or remove it",
        ),
        (
            "stale-derived-edge",
            "edges[7].derived: crates-core depends on crates-door is no longer declared; run task atlas:sync",
        ),
        (
            "missing-derived-edge",
            "edges: crates-door depends on crates-core is missing; run task atlas:sync",
        ),
        (
            "wrong-derived-source",
            "nodes[crates-door].derived: must name source \"Cargo.toml\"; run task atlas:sync",
        ),
    ];
    for (name, expected) in cases {
        let dir = fixture_copy()?;
        std::fs::copy(
            fixtures().join(format!("stale/{name}.json")),
            dir.path().join(".rosetta/atlas.json"),
        )?;
        let findings = atlas::check_atlas(dir.path())?;
        assert!(
            findings.contains(&expected.to_owned()),
            "{name}: {findings:?}"
        );
    }
    Ok(())
}

#[test]
fn check_passes_on_the_fixture() -> TestResult {
    let dir = fixture_copy()?;
    assert_eq!(atlas::check_atlas(dir.path())?, Vec::<String>::new());
    Ok(())
}

#[test]
fn check_names_a_path_that_is_gone() -> TestResult {
    let dir = fixture_copy()?;
    std::fs::remove_file(dir.path().join("docs/shipping.md"))?;
    let findings = atlas::check_atlas(dir.path())?;
    assert!(
        findings.contains(&"nodes[door].refs.docs: \"docs/shipping.md\" does not exist".to_owned())
    );
    Ok(())
}

#[test]
fn sync_adds_a_new_member_and_is_idempotent() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    std::fs::create_dir_all(root.join("crates/extra/src"))?;
    std::fs::write(
        root.join("crates/extra/Cargo.toml"),
        "[package]\nname = \"fixture-extra\"\nversion = \"0.1.0\"\n\n[dependencies]\nfixture-core = { path = \"../core\" }\n",
    )?;
    edit(
        &root.join("Cargo.toml"),
        "\"crates/door\"]",
        "\"crates/door\", \"crates/extra\"]",
    )?;
    let findings = atlas::check_atlas(root)?;
    assert!(findings.contains(&"nodes: crates/extra (fixture-extra) is a workspace member in Cargo.toml with no node \"crates-extra\"; run task atlas:sync".to_owned()));

    assert!(atlas::sync_atlas(root)?);
    let once = std::fs::read_to_string(root.join(".rosetta/atlas.json"))?;
    assert!(!atlas::sync_atlas(root)?);
    assert_eq!(
        std::fs::read_to_string(root.join(".rosetta/atlas.json"))?,
        once
    );
    assert_eq!(atlas::check_atlas(root)?, Vec::<String>::new());
    let synced = atlas::load_atlas(root)?;
    assert!(synced.node("crates-extra").is_some());
    assert!(
        synced
            .edges
            .iter()
            .any(|e| e.from == "crates-extra" && e.to == "crates-core")
    );
    Ok(())
}

#[test]
fn sync_starts_an_atlas_the_checks_accept_where_there_is_none() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    std::fs::remove_file(root.join(".rosetta/atlas.json"))?;

    assert!(atlas::sync_atlas(root)?);
    assert_eq!(atlas::check_atlas(root)?, Vec::<String>::new());
    let started = atlas::load_atlas(root)?;
    assert_eq!(started.project.name, "fixture-repo");
    assert!(started.node(derive::ROOT_ID).is_some());
    assert!(started.node("crates-core").is_some());
    assert!(started.nodes.iter().all(|node| node.derived.is_some()));
    assert!(!atlas::sync_atlas(root)?);
    atlas::bundle_repo(root)?;
    Ok(())
}

#[test]
fn atlas_sync_enables_a_checkout_without_a_virgil_directory() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    std::fs::remove_dir_all(root.join(".rosetta"))?;
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "add", "origin", "git@github.com:acme/widgets.git"])
        .status()?;
    assert!(status.success());
    let host = Host {
        cwd: root.join("crates"),
        path_env: None,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());

    let synced = run_atlas(&AtlasCommand::Sync, &host, &mut out, &mut err)?;
    assert_eq!(synced, Outcome::Passed);
    assert_eq!(String::from_utf8(out)?, "Synced .rosetta/atlas.json\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let checked = run_atlas(
        &AtlasCommand::Check {
            strict: false,
            json: false,
        },
        &host,
        &mut out,
        &mut err,
    )?;
    assert_eq!(String::from_utf8(err)?, "");
    assert_eq!(checked, Outcome::Passed);
    assert_eq!(atlas::load_atlas(root)?.project.repo, "acme/widgets");
    Ok(())
}

#[test]
fn strict_check_requires_curated_part_details_without_changing_default_check() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let path = root.join(".rosetta/atlas.json");
    let mut source: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    let web = source["nodes"]
        .as_array_mut()
        .ok_or("nodes array")?
        .iter_mut()
        .find(|node| node["id"] == "web")
        .ok_or("web part")?;
    web.as_object_mut().ok_or("web object")?.remove("summary");
    web.as_object_mut().ok_or("web object")?.remove("tier");
    std::fs::write(&path, serde_json::to_vec_pretty(&source)?)?;

    let normal = atlas::check_atlas_from(root, ".rosetta")?;
    assert!(!normal.iter().any(|line| line.contains("a part needs")));
    let strict = atlas::check_atlas_with_mode_from(root, ".rosetta", true)?;
    assert!(
        strict
            .iter()
            .any(|line| line == "nodes[web].summary: a part needs a summary")
    );
    assert!(
        strict
            .iter()
            .any(|line| line == "nodes[web].tier: a part needs a tier")
    );
    assert!(
        strict
            .iter()
            .any(|line| line == "nodes[web].refs.term: a named part needs a glossary term")
    );
    assert!(
        strict
            .iter()
            .any(|line| line == "nodes[cells].refs.term: a named part needs a glossary term")
    );
    Ok(())
}

#[test]
fn sync_keeps_curated_label_summary_family_and_refs() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let path = root.join(".rosetta/atlas.json");
    let mut model = atlas::load_atlas(root)?;
    let node = model
        .nodes
        .iter_mut()
        .find(|node| node.id == "crates-core")
        .ok_or("crates-core")?;
    node.label = "Core".to_owned();
    node.summary = Some("The records core.".to_owned());
    node.family = atlas::Family::Store;
    node.refs.term = Some("term.store".to_owned());
    std::fs::write(&path, json_text::to_pretty_string(&model)?)?;
    atlas::sync_atlas(root)?;
    let synced = atlas::load_atlas(root)?;
    let node = synced.node("crates-core").ok_or("crates-core")?;
    assert_eq!(node.label, "Core");
    assert_eq!(node.summary.as_deref(), Some("The records core."));
    assert_eq!(node.family, atlas::Family::Store);
    assert_eq!(node.refs.term.as_deref(), Some("term.store"));
    Ok(())
}

#[test]
fn sync_drops_a_stale_derived_node_nothing_keeps() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    std::fs::remove_file(root.join("infra/src/compute/door.ts"))?;
    let findings = atlas::check_atlas(root)?;
    assert!(findings.iter().any(|f| f.starts_with(
        "nodes[infra-src-compute-door-serviceaccount-account].derived: infra/src/compute/door.ts no longer declares it"
    )));
    atlas::sync_atlas(root)?;
    let synced = atlas::load_atlas(root)?;
    assert!(
        synced
            .node("infra-src-compute-door-serviceaccount-account")
            .is_none()
    );
    // A curated edge still names the Cloud Run service, so it stays and check says so.
    assert!(
        synced
            .node("infra-src-compute-door-cloudrunv2-service")
            .is_some()
    );
    assert!(!atlas::check_atlas(root)?.is_empty());
    Ok(())
}

#[test]
fn derive_reads_both_workspaces_and_the_pulumi_program() -> TestResult {
    let dir = fixture_copy()?;
    let model = derive::derive(dir.path(), &atlas::load_atlas(dir.path())?)?;
    let ids: Vec<&str> = model.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "repo",
            "apps",
            "apps-edge",
            "apps-web",
            "crates",
            "crates-core",
            "crates-door",
            "crates-outside",
            "infra",
            "packages",
            "packages-ui",
            "packages-virgil-schema",
            "infra-src-compute-door-cloudrunv2-service",
            "infra-src-compute-door-serviceaccount-account",
            "infra-src-database-sql-database",
            "infra-src-database-sql-database-instance",
        ]
    );
    Ok(())
}

/// A repository with no atlas and nothing curated: sync alone gives it an
/// Overview and Infra pictures, without inventing a Data picture. The Dockerfile and the electron
/// dependency make parts, the sqlx `postgres` feature (inherited from the
/// workspace entry, turned on by the member) a Postgres store, the
/// Anthropic SDK an outside service, and the Pulumi program and the
/// Terraform file the resources.
#[test]
fn sync_on_a_fresh_repo_derives_structure_without_inventing_data_flow() -> TestResult {
    let dir = fixture_copy_of(&fixtures().join("fresh"))?;
    let root = dir.path();
    assert!(atlas::sync_atlas(root)?);
    assert_eq!(atlas::check_atlas(root)?, Vec::<String>::new());
    assert!(!atlas::sync_atlas(root)?);

    let model = atlas::load_atlas(root)?;
    assert!(model.views.is_empty());
    let parts: Vec<(&str, &str, Option<&str>, &str)> = model
        .nodes
        .iter()
        .filter(|node| node.kind.is_part() || node.kind == atlas::NodeKind::Resource)
        .map(|node| {
            (
                node.id.as_str(),
                node.family.as_str(),
                node.tier.map(atlas::Tier::as_str),
                node.derived.as_ref().map_or("", |d| d.source.as_str()),
            )
        })
        .collect();
    assert_eq!(
        parts,
        vec![
            (
                "infra-index-lambda-function",
                "service",
                Some("cloud"),
                "infra/index.ts"
            ),
            (
                "infra-index-lambda-permission",
                "external",
                Some("cloud"),
                "infra/index.ts"
            ),
            (
                "infra-index-s3-bucket",
                "store",
                Some("cloud"),
                "infra/index.ts"
            ),
            (
                "terraform-main-google-sql-database-instance",
                "store",
                Some("cloud"),
                "terraform/main.tf"
            ),
            (
                "terraform-main-google-storage-bucket-iam-member",
                "external",
                Some("cloud"),
                "terraform/main.tf"
            ),
            (
                "part-apps-desktop",
                "surface",
                Some("device"),
                "apps/desktop/package.json"
            ),
            (
                "part-crates-api",
                "service",
                Some("cloud"),
                "crates/api/Dockerfile"
            ),
            (
                "external-anthropic",
                "external",
                Some("external"),
                "apps/desktop/package.json"
            ),
            ("store-postgres", "store", None, "crates/api/Cargo.toml"),
        ]
    );
    let edges: Vec<(&str, &str, &str, Option<&str>)> = model
        .edges
        .iter()
        .filter(|edge| edge.kind != atlas::model::EdgeKind::Calls || edge.label.is_some())
        .map(|edge| {
            (
                edge.from.as_str(),
                edge.kind.as_str(),
                edge.to.as_str(),
                edge.label.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        edges,
        vec![
            (
                "part-apps-desktop",
                "depends on",
                "external-anthropic",
                Some("@anthropic-ai/sdk")
            ),
            (
                "part-crates-api",
                "depends on",
                "store-postgres",
                Some("sqlx")
            ),
        ]
    );

    let bundle = atlas::bundle_repo(root)?;
    let revision = &bundle
        .snapshot
        .as_ref()
        .ok_or("missing snapshot")?
        .fingerprint;
    for edge in bundle
        .edges
        .iter()
        .filter(|edge| edge.kind == atlas::model::EdgeKind::Depends)
    {
        let evidence = edge.evidence.as_ref().ok_or("missing edge evidence")?;
        assert_eq!(
            evidence.basis,
            atlas::model::EvidenceBasis::ManifestDependency
        );
        assert_eq!(evidence.extractor, "workspace manifest");
        assert_eq!(evidence.revision.as_ref(), Some(revision));
        assert!(root.join(&evidence.source).is_file(), "{}", evidence.source);
    }
    let ids: Vec<&str> = bundle.views.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, vec!["overview", "infra", "connections"]);
    let overview = &bundle.views[0];
    assert_eq!(
        overview.description,
        "What fresh is made of and how the parts talk."
    );
    for view in &bundle.views[..2] {
        assert!(view.sources[""].contains("click "), "{}", view.id);
    }
    let infra = &bundle.views[1].sources[""];
    let groups: Vec<&str> = infra
        .lines()
        .filter(|l| l.trim_start().starts_with("subgraph "))
        .collect();
    assert_eq!(
        groups,
        vec![
            "  subgraph g1[\"infra/index\"]",
            "  subgraph g2[\"terraform/main\"]",
        ]
    );
    // Wiring is reached through Connections, never drawn in Infra.
    assert!(!infra.contains("permission"), "{infra}");
    assert!(!infra.contains("iam_member"), "{infra}");
    Ok(())
}

/// Every Cargo member `cargo metadata` lists is a derived node of this repo.
#[test]
fn derive_on_this_repo_lists_every_cargo_member() -> TestResult {
    let root = repo_root();
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()?;
    #[derive(serde::Deserialize)]
    struct Metadata {
        packages: Vec<Package>,
    }
    #[derive(serde::Deserialize)]
    struct Package {
        name: String,
    }
    let metadata: Metadata = serde_json::from_slice(&output.stdout)?;
    let model = derive::derive(&root, &atlas::starter_atlas(&root)?)?;
    let labels: BTreeSet<&str> = model
        .nodes
        .iter()
        .filter(|node| node.kind == atlas::NodeKind::Crate)
        .map(|node| node.label.as_str())
        .collect();
    let expected: BTreeSet<&str> = metadata.packages.iter().map(|p| p.name.as_str()).collect();
    assert!(
        expected.is_subset(&labels),
        "{:?}",
        expected.difference(&labels)
    );
    Ok(())
}

fn golden(name: &str, actual: &str) -> TestResult {
    let path = fixtures().join("out").join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("parent")?)?;
        std::fs::write(&path, actual)?;
    }
    let expected = std::fs::read_to_string(&path)
        .map_err(|error| format!("{}: {error}; run with UPDATE_GOLDEN=1", path.display()))?;
    assert_eq!(
        actual, expected,
        "{name} differs from its golden; run with UPDATE_GOLDEN=1 if intended"
    );
    Ok(())
}

#[test]
fn the_fixture_bundle_matches_its_golden() -> TestResult {
    let dir = fixture_copy()?;
    let bundle = atlas::bundle_repo(dir.path())?;
    golden("bundle.json", &json_text::to_pretty_string(&bundle)?)?;
    for view in &bundle.views {
        for (container, source) in &view.sources {
            let name = if container.is_empty() {
                format!("{}.mmd", view.id)
            } else {
                format!("{}/{}.mmd", view.id, container.replace([':', '/'], "_"))
            };
            golden(&name, source)?;
        }
    }
    Ok(())
}

#[test]
fn every_built_view_renders_as_mermaid_and_markdown() -> TestResult {
    let dir = fixture_copy()?;
    let host = Host {
        cwd: dir.path().to_path_buf(),
        path_env: None,
    };
    let bundle = atlas::bundle_repo(dir.path())?;
    for view in &bundle.views {
        let root = if view.sources.contains_key(&view.default_root) {
            Some(view.default_root.clone())
        } else {
            view.sources.keys().next().cloned()
        };
        let Some(root) = root else { continue };
        for format in [RenderFormat::Mermaid, RenderFormat::Md] {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = run_atlas(
                &AtlasCommand::Render {
                    view: view.id.clone(),
                    root: (!root.is_empty()).then_some(root.clone()),
                    out: None,
                    format,
                },
                &host,
                &mut out,
                &mut err,
            )?;
            assert_eq!(outcome, Outcome::Passed);
            let text = String::from_utf8(out)?;
            assert!(text.contains("flowchart "), "{} {format:?}", view.id);
            if format == RenderFormat::Md {
                assert!(text.contains("```mermaid\n"), "{}", view.id);
                assert!(text.contains("Legend:"), "{}", view.id);
                assert!(!text.contains("\nclick "), "{}", view.id);
            } else {
                assert!(!text.contains("```mermaid"), "{}", view.id);
            }
        }
    }
    Ok(())
}

#[test]
fn the_fixture_bundle_covers_every_view_kind_and_a_nested_tree() -> TestResult {
    let dir = fixture_copy()?;
    let bundle = atlas::bundle_repo(dir.path())?;
    let kinds: BTreeSet<&str> = bundle.views.iter().map(|v| v.kind.as_str()).collect();
    assert_eq!(
        kinds,
        BTreeSet::from([
            "connections",
            "custom",
            "data",
            "flow",
            "infra",
            "overview",
            "records",
        ])
    );
    let ids: Vec<&str> = bundle.views.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "overview",
            "infra",
            "records",
            "data",
            "send",
            "write-path",
            "connections"
        ]
    );
    // The tree a panel's Contains list walks: each step's children name it
    // as their parent, from the repo root down to a file.
    for (child, parent) in [
        ("crates", "repo"),
        ("crates-core", "crates"),
        ("d:crates/core/src", "crates-core"),
        ("d:crates/core/src/store", "d:crates/core/src"),
    ] {
        let node = bundle.nodes.iter().find(|n| n.id == child).ok_or(child)?;
        assert_eq!(node.parent.as_deref(), Some(parent), "{child}");
        let above = bundle.nodes.iter().find(|n| n.id == parent).ok_or(parent)?;
        assert!(above.has_children, "{parent}");
    }
    let rows = bundle
        .nodes
        .iter()
        .find(|n| n.id == "f:crates/core/src/store/rows.rs")
        .ok_or("rows.rs")?;
    assert_eq!(rows.lines, Some(5));
    assert_eq!(rows.parent.as_deref(), Some("d:crates/core/src/store"));
    let logo = bundle
        .nodes
        .iter()
        .find(|n| n.id == "f:packages/ui/assets/logo.png")
        .ok_or("logo")?;
    assert_eq!(logo.lines, None);
    let rule_ids: Vec<&str> = bundle.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        rule_ids,
        vec![
            "principle.door-answers-edge-is-live",
            "principle.safe-by-default",
            "rule.door-no-stream"
        ]
    );
    Ok(())
}

#[test]
fn every_source_is_a_coloured_free_flowchart_with_a_link_per_node() -> TestResult {
    let dir = fixture_copy()?;
    let bundle = atlas::bundle_repo(dir.path())?;
    let hex = regex::Regex::new(r"#[0-9a-fA-F]{3,8}\b")?;
    // A part is a box or a cylinder; a "+N more" node stands for parts and
    // carries no link.
    let part = regex::Regex::new(r#"^\s*n\d+\[\(?""#)?;
    let more = regex::Regex::new(r#"^\s*n\d+\["\+\d+ more"\]$"#)?;
    for view in &bundle.views {
        for source in view.sources.values() {
            assert!(!hex.is_match(source), "{source}");
            assert!(source.starts_with("flowchart LR\n") || source.starts_with("flowchart TB\n"));
            let nodes = source
                .lines()
                .filter(|l| part.is_match(l) && !more.is_match(l))
                .count();
            let clicks = source
                .lines()
                .filter(|l| l.trim_start().starts_with("click n") && l.contains("\"atlas:"))
                .count();
            assert_eq!(nodes, clicks, "{source}");
            assert!(!source.contains("classDef"), "{source}");
        }
    }
    Ok(())
}

/// Eight steps over six parts: each part drawn once inside its tier, and
/// seven numbered arrows in step order, a step that stays on its part a loop.
#[test]
fn a_flow_draws_each_part_once_with_a_numbered_arrow_per_step() -> TestResult {
    let dir = fixture_copy()?;
    let bundle = atlas::bundle_repo(dir.path())?;
    let flow = &bundle
        .views
        .iter()
        .find(|v| v.id == "send")
        .ok_or("send")?
        .sources[""];
    let clicks: Vec<&str> = flow
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("click "))
        .collect();
    assert_eq!(
        clicks,
        vec![
            "n1 \"atlas:web\"",
            "n6 \"atlas:packages-ui\"",
            "n2 \"atlas:door\"",
            "n3 \"atlas:store\"",
            "n4 \"atlas:cells\"",
            "n5 \"atlas:edge\"",
        ]
    );
    let arrows: Vec<&str> = flow.lines().filter(|l| l.contains("-->")).collect();
    assert_eq!(
        arrows,
        vec![
            "  n1 -->|\"1. the person sends\"| n2",
            "  n2 -->|\"2. answers #quot;accepted#quot;\"| n2",
            "  n2 -->|\"3. checks the policy\"| n3",
            "  n3 -->|\"4. commits the row\"| n4",
            "  n4 -->|\"5. keeps it\"| n5",
            "  n5 -->|\"6. carries it live\"| n6",
            "  n6 -->|\"7. draws it\"| n1",
        ]
    );
    let groups: Vec<&str> = flow
        .lines()
        .filter(|l| l.trim_start().starts_with("subgraph "))
        .collect();
    assert_eq!(
        groups,
        vec![
            "  subgraph g1[\"Device\"]",
            "  subgraph g2[\"Cloud\"]",
            "  subgraph g3[\"Edge\"]",
        ]
    );
    Ok(())
}

/// The door's picture: what uses it on the left, what it uses on the right,
/// each arrow saying its label or else its kind, the door marked as focus.
#[test]
fn a_connections_source_draws_a_part_between_its_users_and_what_it_uses() -> TestResult {
    let dir = fixture_copy()?;
    let bundle = atlas::bundle_repo(dir.path())?;
    let connections = bundle
        .views
        .iter()
        .find(|v| v.id == atlas::CONNECTIONS_VIEW)
        .ok_or("connections")?;
    assert_eq!(connections.kind, atlas::ViewKind::Connections);
    assert_eq!(connections.default_root, "");
    assert!(!connections.sources.contains_key(""));
    let door = &connections.sources["door"];
    assert!(door.contains("  n1[\"Door\"]\n  click n1 \"atlas:door\"\n"));
    assert!(door.contains("  class n1 is-focus\n"));
    let arrows: Vec<&str> = door.lines().filter(|l| l.contains("-->")).collect();
    assert_eq!(
        arrows,
        vec![
            "  n2 -->|\"calls · request/answer\"| n1",
            "  n3 -->|\"deploys · runs\"| n1",
            "  n1 -->|\"writes\"| n4"
        ]
    );
    assert!(door.contains("click n2 \"atlas:web\""));
    assert!(door.contains("click n3 \"atlas:infra-src-compute-door-cloudrunv2-service\""));
    assert!(door.contains("click n4 \"atlas:store\""));

    let connected: BTreeSet<&str> = bundle
        .edges
        .iter()
        .flat_map(|e| [e.from.as_str(), e.to.as_str()])
        .collect();
    let rooted: BTreeSet<&str> = connections.sources.keys().map(String::as_str).collect();
    assert_eq!(rooted, connected);
    assert!(!connections.sources.contains_key("crates-outside"));
    assert!(
        atlas::render_view(&bundle, atlas::CONNECTIONS_VIEW, Some("door"))?.contains("is-focus")
    );
    assert!(atlas::render_view(&bundle, atlas::CONNECTIONS_VIEW, None).is_err());
    Ok(())
}

/// Data draws every edge that moves state and the parts at its ends, in the
/// model's order; stores are cylinders, and the web app's call to the door
/// is not one of those edges.
#[test]
fn a_data_source_draws_stores_as_cylinders_and_only_data_edges() -> TestResult {
    let dir = fixture_copy()?;
    let bundle = atlas::bundle_repo(dir.path())?;
    let data = &bundle
        .views
        .iter()
        .find(|v| v.id == "data")
        .ok_or("data")?
        .sources[""];
    assert!(data.contains("    n3[(\"Store\")]\n"), "{data}");
    assert!(data.contains("    n5[(\"Cells\")]\n"), "{data}");
    assert!(data.contains("    n2[\"Door\"]\n"), "{data}");
    let arrows: Vec<&str> = data.lines().filter(|l| l.contains("-->")).collect();
    assert_eq!(
        arrows,
        vec![
            "  n2 -->|\"writes\"| n3",
            "  n3 -->|\"writes\"| n5",
            "  n3 -->|\"publishes · outbox\"| n4",
            "  n4 -->|\"delivers · live\"| n1",
        ]
    );
    Ok(())
}

/// Three collections and two refs from the fixture's schema bundle: the
/// areas picture, and an area drawing its collections with their scopes and
/// the area its ref leaves for. A ref to any record draws nothing.
#[test]
fn the_records_view_reads_collections_and_refs_from_the_schema_bundle() -> TestResult {
    let dir = fixture_copy()?;
    let bundle = atlas::bundle_repo(dir.path())?;
    let collections: Vec<&str> = bundle
        .nodes
        .iter()
        .filter(|n| n.kind == atlas::NodeKind::Collection)
        .map(|n| n.id.as_str())
        .collect();
    assert_eq!(
        collections,
        vec!["records:channel", "records:message", "records:principal"]
    );
    let message = bundle
        .nodes
        .iter()
        .find(|n| n.id == "records:message")
        .ok_or("message")?;
    assert_eq!(message.parent.as_deref(), Some("records-area:conversation"));
    assert_eq!(
        message.summary.as_deref(),
        Some("One utterance in a channel.")
    );
    let area = bundle
        .nodes
        .iter()
        .find(|n| n.id == "records-area:conversation")
        .ok_or("area")?;
    assert!(area.has_children);
    let refs: Vec<(&str, &str, Option<&str>)> = bundle
        .edges
        .iter()
        .filter(|e| e.kind == atlas::model::EdgeKind::Refs)
        .map(|e| (e.from.as_str(), e.to.as_str(), e.label.as_deref()))
        .collect();
    assert_eq!(
        refs,
        vec![
            ("records:message", "records:channel", Some("channel")),
            (
                "records:message",
                "records:principal",
                Some("decision.decided_by")
            ),
        ]
    );
    let records = bundle
        .views
        .iter()
        .find(|v| v.id == atlas::RECORDS_VIEW)
        .ok_or("records")?;
    assert_eq!(records.kind, atlas::ViewKind::Records);
    let keys: Vec<&str> = records.sources.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec!["", "records-area:conversation", "records-area:people"]
    );
    assert!(records.sources[""].contains("  n1 -->|\"1 ref\"| n2\n"));
    let conversation = &records.sources["records-area:conversation"];
    assert!(conversation.contains("  n2 -->|\"channel\"| n1\n"));
    assert!(conversation.contains("  n2 -->|\"decision.decided_by\"| n3\n"));
    assert!(conversation.contains("  class n1,n2 scope-container\n"));
    // The collection is reached from the connections view as any part is.
    let connections = bundle
        .views
        .iter()
        .find(|v| v.id == atlas::CONNECTIONS_VIEW)
        .ok_or("connections")?;
    assert!(connections.sources.contains_key("records:principal"));
    Ok(())
}

#[test]
fn check_names_a_collection_no_area_holds() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    edit(
        &root.join(".rosetta/atlas.json"),
        "\"message\", \"chan*\"",
        "\"message\"",
    )?;
    let findings = atlas::check_atlas(root)?;
    assert_eq!(
        findings,
        vec!["records.areas: the collection \"channel\" is in no area; add it to one".to_owned()]
    );
    let bundle = atlas::bundle_repo(root)?;
    let channel = bundle
        .nodes
        .iter()
        .find(|n| n.id == "records:channel")
        .ok_or("channel")?;
    assert_eq!(channel.parent.as_deref(), Some("records-area:other"));
    Ok(())
}

#[test]
fn the_same_input_gives_the_same_bytes() -> TestResult {
    let dir = fixture_copy()?;
    let first = json_text::to_pretty_string(&atlas::bundle_repo(dir.path())?)?;
    let second = json_text::to_pretty_string(&atlas::bundle_repo(dir.path())?)?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn the_committed_schema_is_the_model_s() -> TestResult {
    let schema = schemars::schema_for!(Atlas);
    let text = json_text::to_pretty_string(&schema)?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/atlas.schema.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &text)?;
    }
    assert_eq!(text, std::fs::read_to_string(path)?);
    Ok(())
}

/// Three commits touching different paths: each path, and each directory
/// above it, takes the newest commit that touched it; a path no commit
/// touched has no date, and neither does anything before the first commit.
#[test]
fn history_dates_each_path_by_the_newest_commit_that_touched_it() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let git = |args: &[&str], date: &str| -> std::io::Result<()> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.com",
            ])
            .args(args)
            .env("GIT_COMMITTER_DATE", date)
            .env("GIT_AUTHOR_DATE", date)
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ))
        }
    };
    let paths = BTreeSet::from(["a", "a/x.rs", "b", "b/y.rs", "c.md", ".", "gone.rs"]);
    git(&["init", "-q"], "2026-10-01T00:00:00Z")?;
    assert!(atlas::history::last_changed(root, &paths)?.is_empty());

    std::fs::create_dir_all(root.join("a"))?;
    std::fs::create_dir_all(root.join("b"))?;
    std::fs::write(root.join("a/x.rs"), "one\n")?;
    std::fs::write(root.join("b/y.rs"), "one\n")?;
    git(&["add", "-A"], "2026-10-01T00:00:00Z")?;
    git(&["commit", "-q", "-m", "first"], "2026-10-01T00:00:00Z")?;
    std::fs::write(root.join("a/x.rs"), "two\n")?;
    git(&["commit", "-q", "-am", "second"], "2026-10-02T00:00:00Z")?;
    std::fs::write(root.join("c.md"), "three\n")?;
    git(&["add", "c.md"], "2026-10-03T00:00:00Z")?;
    git(&["commit", "-q", "-m", "third"], "2026-10-03T00:00:00Z")?;

    let found = atlas::history::last_changed(root, &paths)?;
    let subjects: Vec<(&str, &str)> = found
        .iter()
        .map(|(path, updated)| (path.as_str(), updated.subject.as_str()))
        .collect();
    assert_eq!(
        subjects,
        vec![
            (".", "third"),
            ("a", "second"),
            ("a/x.rs", "second"),
            ("b", "first"),
            ("b/y.rs", "first"),
            ("c.md", "third"),
        ]
    );
    assert_eq!(found["b"].at, "2026-10-01T00:00:00Z");
    Ok(())
}

/// The fixture copy has no commit, so nothing in its bundle is dated and
/// its golden stays byte-stable.
#[test]
fn a_bundle_before_the_first_commit_dates_nothing() -> TestResult {
    let dir = fixture_copy()?;
    let bundle = atlas::bundle_repo(dir.path())?;
    assert!(bundle.nodes.iter().all(|node| node.updated.is_none()));
    Ok(())
}
