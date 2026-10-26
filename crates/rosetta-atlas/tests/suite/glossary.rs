//! The glossary port: every case of the TypeScript pilot's tests, the
//! conformance of the real glossary, the generated query, code names, the
//! closed-epic warning and `add`.

use std::collections::BTreeSet;
use std::path::Path;

use crate::common::{TestResult, edit, fixture_copy, fixture_repo};
use rosetta_atlas::glossary::epic::{EpicStatus, VirgilTasksOnPath, closed_epic_warnings};
use rosetta_atlas::glossary::model::{AvoidWord, GlossarySection};
use rosetta_atlas::glossary::{
    self, CodeNames, Glossary, GlossarySource, GlossaryTerm, NewTerm, TermStatus,
    ValidationContext, count_sentences, query::glossary_query, render_glossary, validate_glossary,
    wrap,
};
use rosetta_atlas::json_text;
use rosetta_atlas::rulebook::Rulebook;
use rosetta_atlas::{config::Config, glossary_context};
use serde_json::{Value, json};

fn context() -> ValidationContext<'static> {
    ValidationContext {
        rule_ids: BTreeSet::from(["rule.known".to_owned()]),
        principle_ids: BTreeSet::from(["principle.known".to_owned()]),
        rule_source: Some("whetstone/rulebook.json".to_owned()),
        principle_source: Some("whetstone/rulebook.json".to_owned()),
        doc_exists: Box::new(|path| path == "docs/known.md"),
        code_names: None,
        known_contexts: BTreeSet::new(),
    }
}

fn source(glossary: Value) -> serde_json::Result<GlossarySource> {
    serde_json::from_value(glossary)
}

fn valid_glossary() -> Value {
    json!({
        "schema": "virgil.glossary/v1",
        "context": "virgil",
        "purpose": "A purpose.",
        "sections": [
            { "id": "product", "title": "Product" },
            { "id": "agents", "title": "Agents" }
        ],
        "terms": [
            {
                "id": "term.sphere",
                "name": "Sphere",
                "section": "product",
                "definition": "A bounded world of context. The core primitive.",
                "avoid": [{ "word": "space", "except": ["Other space"] }],
                "refs": {
                    "rules": ["rule.known"],
                    "principles": ["principle.known"],
                    "docs": ["docs/known.md"]
                },
                "status": "settled",
                "decided": { "on": "2026-10-13", "page": "virgil://page/abc" }
            },
            {
                "id": "term.cato",
                "name": "Cato",
                "section": "agents",
                "definition": "The merge agent.",
                "status": "proposed"
            }
        ]
    })
}

#[test]
fn accepts_a_valid_glossary() -> TestResult {
    assert_eq!(
        validate_glossary(&source(valid_glossary())?, &context()),
        vec![]
    );
    Ok(())
}

#[test]
fn scoped_homonyms_select_only_their_path_and_reopen_the_atlas_view() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    std::fs::write(
        root.join(".rosetta/rosetta.toml"),
        "[refs]\nrulebook = 'whetstone/rulebook.json'\n[[name_sources]]\nglob = 'packages/virgil-schema/src/generated/schemas.json'\npointer = '/x-virgil-collections'\n[[path_scopes]]\ncontext = 'web'\npaths = ['apps/web/**']\n[[path_scopes]]\ncontext = 'service'\npaths = ['crates/door/**']\n",
    )?;
    let path = root.join(".rosetta/glossary.json");
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let entries = value["terms"].as_array_mut().ok_or("terms array")?;
    entries.push(json!({"id":"term.web-connector","name":"Connector","section":"architecture","definition":"The browser connector.","contexts":["web"],"status":"settled"}));
    entries.push(json!({"id":"term.service-connector","name":"Connector","section":"architecture","definition":"The service connector.","contexts":["service"],"status":"settled"}));
    std::fs::write(&path, serde_json::to_string_pretty(&value)?)?;
    let context = glossary::repo_context(root)?;
    let loaded = glossary::load_glossary(root, &context);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    let glossary = loaded.glossary.ok_or("scoped glossary")?;
    assert!(glossary::find_term(&glossary, "Connector").is_none());
    assert!(glossary::find_term(&glossary, "term.web-connector").is_some());
    let config = Config::load(root, None)?;
    let selected = glossary_context::select(
        root,
        &config,
        &glossary,
        &["apps/web/src/index.ts".into()],
        None,
    )?;
    assert!(
        selected
            .terms
            .iter()
            .any(|term| term.id == "term.web-connector")
    );
    assert!(
        !selected
            .terms
            .iter()
            .any(|term| term.id == "term.service-connector")
    );
    assert!(selected.parts.iter().any(|part| part == "web"));
    let reference = selected.view_reference.as_deref().ok_or("view reference")?;
    let bundle = rosetta_atlas::atlas::bundle_repo(root)?;
    let atlas_view = rosetta_atlas::atlas::focus::reopen(&bundle, reference)?;
    let atlas_path_view = rosetta_atlas::atlas::focus::select(
        &bundle,
        rosetta_atlas::atlas::focus::Query {
            paths: vec!["apps/web/src/index.ts".into()],
            ..Default::default()
        },
    )?;
    assert_eq!(
        atlas_path_view.id,
        selected.view_id.as_deref().ok_or("view id")?
    );
    assert_eq!(
        selected
            .terms
            .iter()
            .map(|term| &term.id)
            .collect::<Vec<_>>(),
        atlas_view
            .terms
            .iter()
            .map(|term| &term.id)
            .collect::<Vec<_>>()
    );
    let reopened = glossary_context::select(root, &config, &glossary, &[], Some(reference))?;
    assert_eq!(selected.view_id, reopened.view_id);
    assert_eq!(
        selected
            .terms
            .iter()
            .map(|term| &term.id)
            .collect::<Vec<_>>(),
        reopened
            .terms
            .iter()
            .map(|term| &term.id)
            .collect::<Vec<_>>()
    );
    let text = glossary_context::render_text(&selected, 2000)?;
    assert!(text.chars().count() <= 2000);
    assert!(text.contains("term.web-connector"));
    std::fs::write(root.join("apps/web/src/index.ts"), "changed")?;
    assert!(glossary_context::select(root, &config, &glossary, &[], Some(reference)).is_err());
    Ok(())
}

#[test]
fn scoped_names_reject_overlap_and_undeclared_contexts() -> TestResult {
    let mut glossary = valid_glossary();
    glossary["terms"][0]["contexts"] = json!(["web"]);
    glossary["terms"][1]["name"] = json!("Sphere");
    glossary["terms"][1]["contexts"] = json!(["web"]);
    let mut context = context();
    context.known_contexts.insert("web".into());
    let problems = validate_glossary(&source(glossary.clone())?, &context);
    assert!(
        problems
            .iter()
            .any(|problem| problem.path == "terms[1].name" && problem.why.contains("duplicates"))
    );
    glossary["terms"][1]["contexts"] = json!(["service"]);
    let problems = validate_glossary(&source(glossary.clone())?, &context);
    assert!(
        problems
            .iter()
            .any(|problem| problem.path == "terms[1].contexts[0]"
                && problem.why.contains("unknown context"))
    );
    context.known_contexts.insert("service".into());
    assert!(validate_glossary(&source(glossary)?, &context).is_empty());
    Ok(())
}

#[test]
fn virgil_vocabulary_context_is_bounded_with_explicit_omissions() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    std::fs::create_dir(root.join(".rosetta"))?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rosetta-glossary/tests/fixtures/virgil/glossary.json");
    let text = std::fs::read_to_string(fixture)?;
    std::fs::write(root.join(".rosetta/glossary.json"), &text)?;
    let glossary: Glossary = serde_json::from_str(&text)?;
    let report = glossary_context::select(root, &Config::load(root, None)?, &glossary, &[], None)?;
    assert_eq!(report.terms.len(), 83);
    let output = glossary_context::render_text(&report, 2000)?;
    assert!(output.chars().count() <= 2000);
    assert!(output.contains("Omitted for text budget:"));
    Ok(())
}

type Mutation = fn(&mut Value);

#[test]
fn rejects_each_broken_rule() -> TestResult {
    let cases: Vec<(&str, Mutation, &str, &str)> = vec![
        (
            "a wrong schema value",
            |g| g["schema"] = json!("virgil.glossary/v0"),
            "schema",
            "must be",
        ),
        (
            "a duplicate section id",
            |g| {
                if let Some(sections) = g["sections"].as_array_mut() {
                    sections.push(json!({ "id": "product", "title": "Again" }));
                }
            },
            "sections[2].id",
            "duplicates section \"product\"",
        ),
        (
            "a section no term uses",
            |g| {
                if let Some(sections) = g["sections"].as_array_mut() {
                    sections.push(json!({ "id": "spare", "title": "Spare" }));
                }
            },
            "sections",
            "\"spare\" has no terms",
        ),
        (
            "a term in an undeclared section",
            |g| g["terms"][1]["section"] = json!("missing"),
            "terms[1].section",
            "declared section",
        ),
        (
            "a term id without the prefix",
            |g| g["terms"][0]["id"] = json!("sphere"),
            "terms[0].id",
            "term.",
        ),
        (
            "a term id that is not kebab-case",
            |g| g["terms"][0]["id"] = json!("term.Big_Sphere"),
            "terms[0].id",
            "kebab",
        ),
        (
            "a duplicate term id",
            |g| g["terms"][1]["id"] = json!("term.sphere"),
            "terms[1].id",
            "duplicates term",
        ),
        (
            "a duplicate name",
            |g| g["terms"][1]["name"] = json!("sphere"),
            "terms[1].name",
            "duplicates the name",
        ),
        (
            "a definition over 280 characters",
            |g| g["terms"][0]["definition"] = json!(format!("{}.", "word ".repeat(60).trim())),
            "terms[0].definition",
            "characters; at most 280",
        ),
        (
            "a definition over two sentences",
            |g| g["terms"][0]["definition"] = json!("One. Two. Three."),
            "terms[0].definition",
            "3 sentences",
        ),
        (
            "an unknown status",
            |g| g["terms"][0]["status"] = json!("draft"),
            "terms[0].status",
            "settled, proposed",
        ),
        (
            "an avoid word equal to a term name",
            |g| g["terms"][0]["avoid"] = json!([{ "word": "cato", "except": [] }]),
            "terms[0].avoid[0].word",
            "is a term's name",
        ),
        (
            "an unknown rule id",
            |g| g["terms"][0]["refs"]["rules"] = json!(["rule.missing"]),
            "terms[0].refs.rules[0]",
            "not in whetstone/rulebook.json",
        ),
        (
            "an unknown principle id",
            |g| g["terms"][0]["refs"]["principles"] = json!(["principle.missing"]),
            "terms[0].refs.principles[0]",
            "not in whetstone/rulebook.json",
        ),
        (
            "a docs path that does not exist",
            |g| g["terms"][0]["refs"]["docs"] = json!(["docs/missing.md"]),
            "terms[0].refs.docs[0]",
            "does not exist",
        ),
        (
            "a decided date that is not ISO",
            |g| {
                g["terms"][0]["decided"] =
                    json!({ "on": "13 October", "page": "virgil://page/abc" })
            },
            "terms[0].decided.on",
            "YYYY-MM-DD",
        ),
        (
            "an empty epic",
            |g| g["terms"][1]["epic"] = json!(""),
            "terms[1].epic",
            "epic:<ULID>",
        ),
        (
            "an epic that is not an epic ref",
            |g| g["terms"][1]["epic"] = json!("task:01ARZ3NDEKTSV4RRFFQ69G5FAA"),
            "terms[1].epic",
            "imported tracker key",
        ),
    ];
    for (name, mutate, path, why) in cases {
        let mut glossary = valid_glossary();
        mutate(&mut glossary);
        let problems = validate_glossary(&source(glossary)?, &context());
        assert!(
            problems
                .iter()
                .any(|p| p.path == path && p.why.contains(why)),
            "{name}: expected {path}: …{why}…, got {problems:?}"
        );
    }
    Ok(())
}

#[test]
fn names_a_value_of_the_wrong_type_at_its_path() -> TestResult {
    let mut glossary = valid_glossary();
    glossary["purpose"] = json!(7);
    glossary["sections"][1] = json!(["agents", "Agents"]);
    glossary["terms"][0]["section"] = json!({ "id": [1, true, null] });
    glossary["terms"][0]["avoid"] = json!(null);
    glossary["terms"][0]["refs"]["docs"] = json!("docs/known.md");
    glossary["terms"][0]["refs"]["pages"] = json!([]);
    glossary["terms"][0]["decided"] = json!(["2026-10-13"]);
    glossary["terms"][1] = json!([
        "term.cato",
        "Cato",
        "agents",
        "The merge agent.",
        "proposed"
    ]);
    let problems: Vec<String> = validate_glossary(&source(glossary)?, &context())
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        problems,
        vec![
            "purpose: must be a non-empty string",
            "sections[1]: must be an object",
            "terms[0].section: must name a declared section, got {\"id\":[1,true,null]}",
            "terms[0].avoid: must be an array",
            "terms[0].refs.docs: must be an array of repo paths",
            "terms[0].refs.pages: is not a known ref kind",
            "terms[0].decided.on: must be a YYYY-MM-DD date",
            "terms[1]: must be an object",
            "sections: section \"product\" has no terms",
        ]
    );
    Ok(())
}

#[test]
fn rejects_a_glossary_that_is_not_an_object() -> TestResult {
    let problems = validate_glossary(&source(json!([]))?, &context());
    assert_eq!(
        problems,
        vec![glossary::Problem::new("$", "must be an object")]
    );
    Ok(())
}

#[test]
fn counts_a_terminal_mark_before_whitespace_or_the_end() {
    assert_eq!(count_sentences("One, two; three."), 1);
    assert_eq!(count_sentences("One. Two!"), 2);
}

#[test]
fn ignores_a_dot_inside_a_path_or_identifier() {
    assert_eq!(
        count_sentences("`whetstone/rulebook.json`: the mission."),
        1
    );
}

fn two_terms() -> Glossary {
    Glossary {
        schema: "virgil.glossary/v1".to_owned(),
        context: "virgil".to_owned(),
        purpose: "Virgil keeps context.".to_owned(),
        sections: vec![
            GlossarySection { id: "product".to_owned(), title: "Product".to_owned() },
            GlossarySection { id: "agents".to_owned(), title: "Agents".to_owned() },
        ],
        terms: vec![
            GlossaryTerm {
                id: "term.group".to_owned(),
                name: "Group".to_owned(),
                section: "product".to_owned(),
                definition: "The people who share one Virgil: its members, channels, tasks and the spheres shared with it.".to_owned(),
                contexts: Vec::new(),
                avoid: Some(vec![
                    AvoidWord {
                        word: "workspace".to_owned(),
                        except: Some(vec!["Slack workspace".to_owned(), "Google Workspace".to_owned()]),
                    },
                    AvoidWord { word: "team".to_owned(), except: None },
                ]),
                code: None,
                refs: None,
                status: TermStatus::Settled,
                decided: None,
                epic: None,
            },
            GlossaryTerm {
                id: "term.cato".to_owned(),
                name: "Cato".to_owned(),
                section: "agents".to_owned(),
                definition: "The merge agent.".to_owned(),
                contexts: Vec::new(),
                avoid: None,
                code: None,
                refs: None,
                status: TermStatus::Proposed,
                decided: None,
                epic: None,
            },
        ],
    }
}

const EXPECTED_TWO_TERMS: &str = "# Virgil

This file is generated from `.rosetta/glossary.json` by `task glossary:render`
and is not edited by hand.

Virgil keeps context.

An `_Avoid_` line lists words used for the term that are not its name.
`rule.glossary-terms` flags them in shown copy; a phrase in quotes after
`except` is another product's own name and is allowed.

## Product

**Group**:
The people who share one Virgil: its members, channels, tasks and the spheres
shared with it.
_Avoid_: workspace (except another product's own: \"Slack workspace\", \"Google Workspace\"), team

## Agents

**Cato**:
The merge agent.
_Proposed_
";

#[test]
fn renders_a_two_term_glossary() {
    assert_eq!(render_glossary(&two_terms()), EXPECTED_TWO_TERMS);
}

#[test]
fn renders_deterministically() {
    assert_eq!(
        render_glossary(&two_terms().clone()),
        render_glossary(&two_terms())
    );
}

#[test]
fn wrap_keeps_every_line_within_80_columns() {
    for line in wrap(&"word ".repeat(100), 80).split('\n') {
        assert!(line.len() <= 80);
    }
}

#[test]
fn wrap_gives_an_over_long_word_a_line_to_itself() {
    let long = "x".repeat(90);
    assert_eq!(wrap(&format!("a {long} b"), 80), format!("a\n{long}\nb"));
}

/// The real glossary, validated without code names: resolving them reads
/// every workspace manifest, which `glossary check` does on the repo.
fn real_context(root: &Path) -> Result<ValidationContext<'_>, Box<dyn std::error::Error>> {
    let rulebook = Rulebook::load(root)?;
    Ok(ValidationContext {
        rule_ids: rulebook.rule_ids(),
        principle_ids: rulebook.principle_ids(),
        rule_source: Some("whetstone/rulebook.json".to_owned()),
        principle_source: Some("whetstone/rulebook.json".to_owned()),
        doc_exists: Box::new(move |path| root.join(path).exists()),
        code_names: None,
        known_contexts: BTreeSet::new(),
    })
}

#[test]
fn the_committed_glossary_md_is_the_render_of_the_real_glossary() -> TestResult {
    let root = fixture_repo();
    let context = real_context(&root)?;
    let loaded = glossary::load_glossary(&root, &context);
    assert_eq!(loaded.problems, vec![]);
    let glossary = loaded.glossary.ok_or("the real glossary is valid")?;
    assert_eq!(
        render_glossary(&glossary),
        std::fs::read_to_string(root.join("GLOSSARY.md"))?
    );
    Ok(())
}

#[test]
fn the_committed_query_is_generated_from_the_real_glossary() -> TestResult {
    let root = fixture_repo();
    let context = real_context(&root)?;
    let glossary = glossary::load_glossary(&root, &context)
        .glossary
        .ok_or("the real glossary is valid")?;
    let rulebook = Rulebook::load(&root)?;
    assert_eq!(
        glossary_query(&glossary).as_deref(),
        rulebook.glossary_query()
    );
    Ok(())
}

#[test]
fn the_real_glossary_round_trips_through_the_writer() -> TestResult {
    let text = std::fs::read_to_string(fixture_repo().join(".rosetta/glossary.json"))?;
    let glossary: Glossary = serde_json::from_str(&text)?;
    assert_eq!(json_text::to_pretty_string(&glossary)?, text);
    Ok(())
}

fn query_regexes(
    query: &str,
) -> Result<(regex::Regex, Vec<regex::Regex>), Box<dyn std::error::Error>> {
    // Each predicate's pattern is a tree-sitter string; undo its escaping.
    let patterns: Vec<String> = query
        .split("@match \"")
        .skip(1)
        .filter_map(|rest| rest.split("\")").next())
        .map(|pattern| pattern.replace("\\\"", "\"").replace("\\\\", "\\"))
        .collect();
    let matched = regex::Regex::new(patterns.first().ok_or("a #match? predicate")?)?;
    let exempt = patterns[1..]
        .iter()
        .map(|p| regex::Regex::new(p))
        .collect::<Result<_, _>>()?;
    Ok((matched, exempt))
}

#[test]
fn a_new_avoid_word_changes_the_query_and_its_exemptions_pass() -> TestResult {
    let mut glossary = two_terms();
    glossary.terms[1].status = TermStatus::Settled;
    let before = glossary_query(&glossary).ok_or("a query")?;
    glossary.terms[1].avoid = Some(vec![AvoidWord {
        word: "bot".to_owned(),
        except: Some(vec!["Slack bot".to_owned()]),
    }]);
    let after = glossary_query(&glossary).ok_or("a query")?;
    assert_ne!(before, after);
    let (matched, exempt) = query_regexes(&after)?;
    let flagged = |text: &str| matched.is_match(text) && !exempt.iter().any(|e| e.is_match(text));
    assert!(flagged("Ask the bot"));
    assert!(flagged("Two bots"));
    assert!(flagged("Your workspace"));
    assert!(flagged("A team"));
    assert!(!flagged("Add the Slack bot"));
    assert!(!flagged("Your Google Workspace"));
    assert!(!flagged("robotic"));
    assert!(!flagged("../bots/index.ts"));
    assert!(!flagged("https://example.com/bot"));
    assert!(flagged("Open the bot/agent list"));
    Ok(())
}

#[test]
fn a_proposed_avoid_word_is_not_enforced() -> TestResult {
    let mut glossary = two_terms();
    let before = glossary_query(&glossary);
    glossary.terms[1].avoid = Some(vec![AvoidWord {
        word: "bot".to_owned(),
        except: None,
    }]);
    assert_eq!(glossary_query(&glossary), before);
    Ok(())
}

#[test]
fn code_names_resolve_to_collections_and_packages() -> TestResult {
    let dir = fixture_copy()?;
    let names = CodeNames::from_repo(dir.path())?;
    for name in [
        "channel",
        "fixture-core",
        "fixture-outside",
        "@fixture/ui",
        "@fixture/edge",
        "fixture-repo",
    ] {
        assert!(names.resolves(name), "{name} resolves");
    }
    for name in ["channels", "fixture", "glossary", "@fixture/missing"] {
        assert!(!names.resolves(name), "{name} does not resolve");
    }
    Ok(())
}

#[test]
fn an_unresolved_code_name_is_a_problem() -> TestResult {
    let dir = fixture_copy()?;
    edit(
        &dir.path().join(".rosetta/glossary.json"),
        "\"fixture-door\"",
        "\"fixture-doors\"",
    )?;
    let context = glossary::repo_context(dir.path())?;
    let problems = glossary::load_glossary(dir.path(), &context).problems;
    assert_eq!(
        problems.iter().map(ToString::to_string).collect::<Vec<_>>(),
        vec![
            ".rosetta/glossary.json#terms[1].code[0]: \"fixture-doors\" is not a Cargo package, a package.json name or a JSON name from packages/virgil-schema/src/generated/schemas.json#/x-virgil-collections"
        ]
    );
    Ok(())
}

struct Never;
impl EpicStatus for Never {
    fn is_closed(&self, _: &str) -> Option<bool> {
        None
    }
}

#[test]
fn check_passes_on_the_fixture_and_fails_on_hand_edits() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let context = glossary::repo_context(root)?;
    assert!(glossary::check_glossary(root, &context, &Never)?.passed());

    edit(
        &root.join("GLOSSARY.md"),
        "The only live transport.",
        "The live transport.",
    )?;
    let report = glossary::check_glossary(root, &context, &Never)?;
    assert_eq!(report.problems[0].path, "GLOSSARY.md");
    assert!(report.diffs[0].contains("-The live transport.\n+The only live transport."));

    glossary::render_files(
        root,
        &glossary::load_glossary(root, &context)
            .glossary
            .ok_or("valid")?,
    )?;
    edit(
        &root.join("whetstone/rulebook.json"),
        "(slack|google)",
        "(slack)",
    )?;
    let report = glossary::check_glossary(root, &context, &Never)?;
    assert_eq!(
        report.problems[0].path,
        "whetstone/rulebook.json#rule.glossary-terms.enforcer.query"
    );
    Ok(())
}

#[test]
fn render_rewrites_only_the_query_in_the_rulebook() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let path = root.join("whetstone/rulebook.json");
    let committed = std::fs::read_to_string(&path)?;
    let generated = Rulebook::load(root)?
        .glossary_query()
        .ok_or("a query")?
        .to_owned();
    let literal = serde_json::to_string(&generated)?;
    std::fs::write(&path, committed.replace(&literal, "\"stale\""))?;
    let context = glossary::repo_context(root)?;
    glossary::render_files(
        root,
        &glossary::load_glossary(root, &context)
            .glossary
            .ok_or("valid")?,
    )?;
    assert_eq!(std::fs::read_to_string(&path)?, committed);
    Ok(())
}

#[test]
fn render_uses_the_configured_rulebook_path() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    std::fs::create_dir(root.join("policy"))?;
    let original_path = root.join("whetstone/rulebook.json");
    let original = std::fs::read_to_string(&original_path)?;
    let configured_path = root.join("policy/rules.json");
    let query = Rulebook::load(root)?
        .glossary_query()
        .ok_or("a query")?
        .to_owned();
    let literal = serde_json::to_string(&query)?;
    std::fs::write(&configured_path, original.replace(&literal, "\"stale\""))?;
    std::fs::write(
        root.join(".rosetta/rosetta.toml"),
        "[refs]\nrulebook = 'policy/rules.json'\n[[name_sources]]\nglob = 'packages/virgil-schema/src/generated/schemas.json'\npointer = '/x-virgil-collections'\n",
    )?;
    let context = glossary::repo_context(root)?;
    let glossary = glossary::load_glossary(root, &context)
        .glossary
        .ok_or("valid glossary")?;
    glossary::render_files(root, &glossary)?;
    assert_eq!(std::fs::read_to_string(&configured_path)?, original);
    assert_eq!(std::fs::read_to_string(&original_path)?, original);
    Ok(())
}

fn stub_virgil(dir: &Path, body: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("virgil");
    std::fs::write(&script, format!("#!/bin/sh\n{body}\n"))?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
}

/// What `virgil tasks show <epic> --json` prints for an epic in `state`.
fn shows_state(state: &str) -> String {
    format!(
        "echo '{{\"api_version\":\"virgil.tasks/v1\",\"result\":{{\"task\":{{\"scope\":{{}},\"task\":\"epic:01ARZ3NDEKTSV4RRFFQ69G5FAA\",\"title\":\"Edge\",\"state\":\"{state}\"}},\"files\":[],\"comments\":[],\"links\":{{}}}}}}'"
    )
}

#[test]
fn a_proposed_term_with_a_closed_epic_warns() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let context = glossary::repo_context(root)?;
    let glossary = glossary::load_glossary(root, &context)
        .glossary
        .ok_or("valid")?;
    let on = |bin: &Path| VirgilTasksOnPath::new(Some(bin.into()));

    let bin = tempfile::tempdir()?;
    stub_virgil(bin.path(), &shows_state("closed"))?;
    let warnings = closed_epic_warnings(&glossary, &on(bin.path()));
    assert_eq!(
        warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
        vec![
            ".rosetta/glossary.json#terms[3].epic: Edge is proposed but its epic FIX-open1 is closed; the PR that lands the epic sets the term settled"
        ]
    );
    let report = glossary::check_glossary(root, &context, &on(bin.path()))?;
    assert!(report.passed(), "a warning never fails the check");
    assert_eq!(report.warnings.len(), 1);

    stub_virgil(bin.path(), &shows_state("open"))?;
    assert_eq!(closed_epic_warnings(&glossary, &on(bin.path())), vec![]);

    stub_virgil(bin.path(), "echo 'daemon unreachable' >&2; exit 1")?;
    assert_eq!(closed_epic_warnings(&glossary, &on(bin.path())), vec![]);

    let empty = tempfile::tempdir()?;
    assert_eq!(closed_epic_warnings(&glossary, &on(empty.path())), vec![]);
    assert_eq!(
        closed_epic_warnings(&glossary, &VirgilTasksOnPath::new(None)),
        vec![]
    );
    Ok(())
}

#[test]
fn a_slow_virgil_is_abandoned_without_a_warning() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let context = glossary::repo_context(root)?;
    let glossary = glossary::load_glossary(root, &context)
        .glossary
        .ok_or("valid")?;
    let bin = tempfile::tempdir()?;
    stub_virgil(bin.path(), &format!("sleep 5\n{}", shows_state("closed")))?;
    let started = std::time::Instant::now();
    let slow = VirgilTasksOnPath::with_timeout(
        Some(bin.path().into()),
        std::time::Duration::from_millis(200),
    );
    assert_eq!(closed_epic_warnings(&glossary, &slow), vec![]);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    Ok(())
}

#[test]
fn add_appends_a_proposed_term_and_renders() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let context = glossary::repo_context(root)?;
    let added = glossary::add_term(
        root,
        &context,
        NewTerm {
            name: "Huddle Room".to_owned(),
            definition: "A live call inside a channel.".to_owned(),
            section: "product".to_owned(),
            page: Some("virgil://page/plan".to_owned()),
            epic: Some("FIX-abc12".to_owned()),
        },
        "2026-10-21",
    )?;
    assert_eq!(added.id, "term.huddle-room");
    let text = std::fs::read_to_string(root.join(".rosetta/glossary.json"))?;
    assert!(text.ends_with(
        "    {\n      \"id\": \"term.huddle-room\",\n      \"name\": \"Huddle Room\",\n      \"section\": \"product\",\n      \"definition\": \"A live call inside a channel.\",\n      \"status\": \"proposed\",\n      \"decided\": { \"on\": \"2026-10-21\", \"page\": \"virgil://page/plan\" },\n      \"epic\": \"FIX-abc12\"\n    }\n  ]\n}\n"
    ));
    let markdown = std::fs::read_to_string(root.join("GLOSSARY.md"))?;
    assert!(markdown.contains("**Huddle Room**:\nA live call inside a channel.\n_Proposed_\n"));
    assert!(glossary::check_glossary(root, &context, &Never)?.passed());
    Ok(())
}

#[test]
fn adding_to_a_legacy_schema_keeps_the_legacy_schema_id() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let path = root.join(".rosetta/glossary.json");
    let before = std::fs::read_to_string(&path)?;
    std::fs::write(
        &path,
        before.replacen("rosetta.glossary/v1", "virgil.glossary/v1", 1),
    )?;
    let context = glossary::repo_context(root)?;
    glossary::add_term(
        root,
        &context,
        NewTerm {
            name: "Review Room".to_owned(),
            definition: "A space for reviewing changes.".to_owned(),
            section: "product".to_owned(),
            page: None,
            epic: None,
        },
        "2026-10-21",
    )?;
    let after: Glossary = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    assert_eq!(after.schema, "virgil.glossary/v1");
    assert!(after.terms.iter().any(|term| term.name == "Review Room"));
    Ok(())
}

#[test]
fn add_writes_nothing_when_the_term_breaks_a_rule() -> TestResult {
    let dir = fixture_copy()?;
    let root = dir.path();
    let before = std::fs::read_to_string(root.join(".rosetta/glossary.json"))?;
    let context = glossary::repo_context(root)?;
    let result = glossary::add_term(
        root,
        &context,
        NewTerm {
            name: "Door".to_owned(),
            definition: "Again.".to_owned(),
            section: "nowhere".to_owned(),
            page: None,
            epic: None,
        },
        "2026-10-21",
    );
    match result {
        Err(glossary::GlossaryError::Invalid(problems)) => {
            let paths: Vec<&str> = problems.iter().map(|p| p.path.as_str()).collect();
            assert!(paths.contains(&".rosetta/glossary.json#terms[4].id"));
            assert!(paths.contains(&".rosetta/glossary.json#terms[4].section"));
        }
        other => return Err(format!("expected Invalid, got {other:?}").into()),
    }
    assert_eq!(
        std::fs::read_to_string(root.join(".rosetta/glossary.json"))?,
        before
    );
    Ok(())
}

#[test]
fn civil_dates() {
    assert_eq!(glossary::civil_date(0), "1970-01-01");
    assert_eq!(glossary::civil_date(20_747), "2026-10-21");
    assert_eq!(glossary::civil_date(11_016), "2000-02-29");
}
