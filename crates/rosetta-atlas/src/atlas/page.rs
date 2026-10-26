//! The atlas as a Virgil page: what a channel's members read about the
//! repository without opening the viewer. Built from the bundle, in the
//! page vocabulary only (headings, paragraphs, a list, a table and a Mermaid
//! block), so the page's frame never refuses it.

use std::fmt::Write;

use super::bundle::Bundle;
use super::model::ViewKind;
use crate::glossary::{Glossary, TermStatus};

/// The page's name in its channel, which is also how a later publish finds
/// it there.
pub fn page_name(project: &str) -> String {
    format!("{project} atlas")
}

/// What the page says beyond the bundle.
#[derive(Debug, Clone, Copy)]
pub struct PageContext<'a> {
    /// One sentence on what the repository is, from its README.
    pub purpose: Option<&'a str>,
    /// The whole glossary, which the bundle carries only in part.
    pub glossary: Option<&'a Glossary>,
    /// The channel is bound to this repository, so its Atlas tab draws the
    /// live map.
    pub live_tab: bool,
}

/// The page's HTML.
pub fn page_html(bundle: &Bundle, context: PageContext<'_>) -> String {
    let mut html = String::new();
    let _ = writeln!(
        html,
        "<h1>{}</h1>",
        escape(&page_name(&bundle.project.name))
    );
    if let Some(purpose) = context.purpose.filter(|p| !p.trim().is_empty()) {
        let _ = writeln!(html, "<p>{}</p>", escape(purpose.trim()));
    }

    if let Some(overview) = bundle
        .views
        .iter()
        .find(|view| view.kind == ViewKind::Overview)
        && let Some(source) = overview.sources.get(&overview.default_root)
    {
        let _ = writeln!(html, "<h2>{}</h2>", escape(&overview.title));
        let _ = writeln!(
            html,
            "<pre data-virgil-language=\"mermaid\">{}</pre>",
            escape(&without_clicks(source))
        );
    }

    let others: Vec<_> = bundle
        .views
        .iter()
        .filter(|view| view.kind != ViewKind::Overview)
        .collect();
    if !others.is_empty() {
        html.push_str("<h2>Views</h2>\n<ul>\n");
        for view in others {
            let _ = write!(html, "<li><strong>{}</strong>", escape(&view.title));
            if !view.description.trim().is_empty() {
                let _ = write!(html, ": {}", escape(view.description.trim()));
            }
            html.push_str("</li>\n");
        }
        html.push_str("</ul>\n");
    }

    html.push_str("<h2>Glossary</h2>\n");
    match context
        .glossary
        .filter(|glossary| !glossary.terms.is_empty())
    {
        Some(glossary) => {
            html.push_str(
                "<table>\n<thead><tr><th>Term</th><th>Section</th><th>Definition</th><th>Status</th></tr></thead>\n<tbody>\n",
            );
            for term in &glossary.terms {
                let section = glossary
                    .sections
                    .iter()
                    .find(|section| section.id == term.section)
                    .map_or(term.section.as_str(), |section| section.title.as_str());
                let status = match term.status {
                    TermStatus::Settled => "Settled",
                    TermStatus::Proposed => "Proposed",
                };
                let _ = writeln!(
                    html,
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{status}</td></tr>",
                    escape(&term.name),
                    escape(section),
                    escape(&term.definition),
                );
            }
            html.push_str("</tbody>\n</table>\n");
        }
        None => html
            .push_str("<p>No glossary yet. <code>virgil glossary draft</code> proposes one.</p>\n"),
    }

    if context.live_tab {
        html.push_str("<p>The live map is this channel's Atlas tab.</p>\n");
    }
    html.push_str("<p>Refresh this page with <code>virgil atlas publish</code>.</p>\n");
    html
}

/// A view's Mermaid without its `click` lines: they open a part in the
/// viewer, and a page has no viewer to open it in.
pub fn without_clicks(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("click "))
        .fold(String::with_capacity(source.len()), |mut out, line| {
            out.push_str(line);
            out.push('\n');
            out
        })
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::atlas::bundle::{BundleProject, BundleView};
    use crate::glossary::model::{GlossarySection, GlossaryTerm};

    fn bundle() -> Bundle {
        let overview = "flowchart LR\n  n1[\"Door\"]\n  click n1 \"atlas:door\"\n  n2[\"App\"]\n    click n2 \"atlas:app\"\n  n2 --> n1\n";
        Bundle {
            schema: "virgil.atlas-bundle/v1".into(),
            project: BundleProject {
                name: "Acme".into(),
                repo: "acme/acme".into(),
            },
            views: vec![
                BundleView {
                    id: "overview".into(),
                    kind: ViewKind::Overview,
                    title: "Overview".into(),
                    description: "Every part.".into(),
                    default_root: String::new(),
                    sources: BTreeMap::from([(String::new(), overview.to_owned())]),
                },
                BundleView {
                    id: "data".into(),
                    kind: ViewKind::Data,
                    title: "Data".into(),
                    description: "Where state moves.".into(),
                    default_root: String::new(),
                    sources: BTreeMap::new(),
                },
            ],
            nodes: Vec::new(),
            edges: Vec::new(),
            terms: Vec::new(),
            rules: Vec::new(),
            capabilities: Vec::new(),
            path_scopes: Vec::new(),
            target_exists: BTreeMap::new(),
            snapshot: None,
        }
    }

    fn glossary() -> Glossary {
        let term = |name: &str, definition: &str, status| GlossaryTerm {
            id: format!("term.{}", name.to_lowercase()),
            name: name.into(),
            section: "architecture".into(),
            definition: definition.into(),
            contexts: Vec::new(),
            avoid: None,
            code: None,
            refs: None,
            status,
            decided: None,
            epic: None,
        };
        Glossary {
            schema: "virgil.glossary/v1".into(),
            context: "acme".into(),
            purpose: "Acme's words.".into(),
            sections: vec![GlossarySection {
                id: "architecture".into(),
                title: "Architecture".into(),
            }],
            terms: vec![
                term("Door", "The server <that> answers.", TermStatus::Settled),
                term("Queue", "Where jobs wait & retry.", TermStatus::Proposed),
            ],
        }
    }

    #[test]
    fn the_overview_is_mermaid_without_its_click_lines() {
        let html = page_html(
            &bundle(),
            PageContext {
                purpose: Some("Acme sells widgets."),
                glossary: None,
                live_tab: false,
            },
        );
        assert!(html.starts_with("<h1>Acme atlas</h1>\n<p>Acme sells widgets.</p>\n"));
        assert!(html.contains(
            "<pre data-virgil-language=\"mermaid\">flowchart LR\n  n1[&quot;Door&quot;]\n  n2[&quot;App&quot;]\n  n2 --&gt; n1\n</pre>"
        ));
        assert!(!html.contains("click"));
        assert!(html.contains("<li><strong>Data</strong>: Where state moves.</li>"));
        assert!(!html.contains("<li><strong>Overview"));
        assert!(html.contains("<code>virgil glossary draft</code>"));
        assert!(!html.contains("Atlas tab"));
        assert!(html.contains("<code>virgil atlas publish</code>"));
    }

    #[test]
    fn each_term_is_a_table_row_with_its_section_title_and_status() {
        let glossary = glossary();
        let html = page_html(
            &bundle(),
            PageContext {
                purpose: None,
                glossary: Some(&glossary),
                live_tab: true,
            },
        );
        assert!(html.contains(
            "<tr><td>Door</td><td>Architecture</td><td>The server &lt;that&gt; answers.</td><td>Settled</td></tr>"
        ));
        assert!(html.contains(
            "<tr><td>Queue</td><td>Architecture</td><td>Where jobs wait &amp; retry.</td><td>Proposed</td></tr>"
        ));
        assert!(html.contains("<p>The live map is this channel's Atlas tab.</p>"));
    }

    #[test]
    fn the_page_escapes_untrusted_purpose() {
        let glossary = glossary();
        let html = page_html(
            &bundle(),
            PageContext {
                purpose: Some("<img src=x onerror=alert(1)>"),
                glossary: Some(&glossary),
                live_tab: true,
            },
        );
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
        assert!(!html.contains("<img src=x"));
    }
}
