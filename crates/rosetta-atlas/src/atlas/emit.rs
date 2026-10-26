//! One Mermaid source per view, or per root for a view with roots (records
//! areas, connections parts): the only contract between this crate and the
//! viewer.
//!
//! Mermaid ids are `n1, n2, ...` in emission order, never the atlas id, which
//! may hold `:`, `/` or `.`. Every node carries a `fam-` and a `kind-` class
//! and a `click` to `atlas:<id>`, which is how the viewer finds it. No class
//! is defined here: the viewer colours by family from the group's brand.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::bundle::BundleNode;
use super::model::{Edge, EdgeKind, Family, NodeKind, Tier, View, ViewKind};
use super::records::RecordsModel;

/// Escapes text for a quoted Mermaid label.
fn label_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("#quot;"),
            '<' => out.push_str("#lt;"),
            '>' => out.push_str("#gt;"),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Percent-encodes an atlas id for the `atlas:` href.
pub fn encode_id(id: &str) -> String {
    let mut out = String::new();
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// A `subgraph` a chart draws its members inside. Groups are written in this
/// order, so a picture reads the same way every time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    Tier(Tier),
    Family(Side, Family),
    /// The file that declares a set of cloud resources, titled by its path.
    File(String),
}

impl Group {
    fn title(&self) -> &str {
        match self {
            Group::Tier(tier) => tier.title(),
            Group::Family(_, family) => family.plural(),
            Group::File(title) => title,
        }
    }
}

/// Which side of a connections picture a neighbour sits on: what reaches the
/// part on the left, what it reaches on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Into,
    OutOf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Box,
    Cylinder,
}

struct Chart {
    direction: &'static str,
    /// Node lines outside every group.
    loose: String,
    groups: BTreeMap<Group, String>,
    links: String,
    /// Each class with the Mermaid ids that carry it, written once at the end
    /// so a large directory stays inside the renderer's source limit.
    classes: BTreeMap<String, Vec<String>>,
    count: usize,
}

impl Chart {
    fn new(direction: &'static str) -> Self {
        Self {
            direction,
            loose: String::new(),
            groups: BTreeMap::new(),
            links: String::new(),
            classes: BTreeMap::new(),
            count: 0,
        }
    }

    /// Adds a node as a box and returns its Mermaid id.
    fn node(&mut self, node: &BundleNode, label: &str, group: Option<Group>) -> String {
        self.shaped(node, label, group, Shape::Box)
    }

    fn shaped(
        &mut self,
        node: &BundleNode,
        label: &str,
        group: Option<Group>,
        shape: Shape,
    ) -> String {
        self.count += 1;
        let id = format!("n{}", self.count);
        let (body, indent) = match group {
            Some(group) => (self.groups.entry(group).or_default(), "    "),
            None => (&mut self.loose, "  "),
        };
        let (open, close) = match shape {
            Shape::Box => ("[", "]"),
            Shape::Cylinder => ("[(", ")]"),
        };
        let _ = writeln!(body, "{indent}{id}{open}\"{}\"{close}", label_text(label));
        let _ = writeln!(body, "{indent}click {id} \"atlas:{}\"", encode_id(&node.id));
        self.class(&id, format!("fam-{}", node.family.as_str()));
        self.class(&id, format!("kind-{}", node.kind.as_str()));
        id
    }

    /// A node that stands for parts not drawn: no click, nothing to select.
    fn more(&mut self, hidden: usize, group: Option<Group>) -> String {
        self.count += 1;
        let id = format!("n{}", self.count);
        let (body, indent) = match group {
            Some(group) => (self.groups.entry(group).or_default(), "    "),
            None => (&mut self.loose, "  "),
        };
        let _ = writeln!(body, "{indent}{id}[\"+{hidden} more\"]");
        self.class(&id, "is-more".to_owned());
        id
    }

    fn class(&mut self, id: &str, class: String) {
        self.classes.entry(class).or_default().push(id.to_owned());
    }

    fn edge(&mut self, from: &str, to: &str, label: Option<&str>) {
        match label {
            Some(label) => {
                let _ = writeln!(self.links, "  {from} -->|\"{}\"| {to}", label_text(label));
            }
            None => {
                let _ = writeln!(self.links, "  {from} --> {to}");
            }
        }
    }

    fn dotted(&mut self, from: &str, to: &str) {
        let _ = writeln!(self.links, "  {from} -.-> {to}");
    }

    fn finish(self) -> String {
        let mut out = format!("flowchart {}\n", self.direction);
        out.push_str(&self.loose);
        for (index, (group, body)) in self.groups.iter().enumerate() {
            let _ = writeln!(
                out,
                "  subgraph g{}[\"{}\"]",
                index + 1,
                label_text(group.title())
            );
            out.push_str(body);
            out.push_str("  end\n");
        }
        out.push_str(&self.links);
        for (class, ids) in &self.classes {
            let _ = writeln!(out, "  class {} {class}", ids.join(","));
        }
        out
    }
}

fn tier_of(node: &BundleNode) -> Option<Group> {
    node.tier.map(Group::Tier)
}

fn shape_of(node: &BundleNode) -> Shape {
    match node.family {
        Family::Store => Shape::Cylinder,
        _ => Shape::Box,
    }
}

/// Whether an edge is drawn in a parts picture: `contains` is the tree the
/// panel walks and `refs` belongs to the records view.
fn drawn_between_parts(edge: &Edge) -> bool {
    !matches!(edge.kind, EdgeKind::Contains | EdgeKind::Refs)
}

/// A picture of the given parts, each in its group, stores as cylinders,
/// with the given edges between them, each saying its verb and label when
/// it has one.
fn picture(
    parts: &[(&BundleNode, Option<Group>)],
    edges: &[&Edge],
    words: fn(&Edge) -> Option<String>,
    implementation_links: &[(&str, &str)],
) -> String {
    let mut chart = Chart::new("LR");
    let mut mermaid: BTreeMap<&str, String> = BTreeMap::new();
    for (node, group) in parts {
        if mermaid.contains_key(node.id.as_str()) {
            continue;
        }
        let key = chart.shaped(node, &node.label, group.clone(), shape_of(node));
        mermaid.insert(node.id.as_str(), key);
    }
    for edge in edges {
        if let (Some(from), Some(to)) = (
            mermaid.get(edge.from.as_str()),
            mermaid.get(edge.to.as_str()),
        ) {
            let (from, to) = data_way(edge, from, to);
            let (from, to) = (from.clone(), to.clone());
            chart.edge(&from, &to, words(edge).as_deref());
        }
    }
    for (part, file) in implementation_links {
        if let (Some(from), Some(to)) = (mermaid.get(part), mermaid.get(file)) {
            chart.dotted(from, to);
        }
    }
    chart.finish()
}

/// The given ids as parts grouped by tier, in the order given.
fn by_tier<'a>(
    ids: &[&str],
    nodes: &BTreeMap<&str, &'a BundleNode>,
) -> Vec<(&'a BundleNode, Option<Group>)> {
    ids.iter()
        .filter_map(|id| nodes.get(id).map(|node| (*node, tier_of(node))))
        .collect()
}

/// Curated parts when they exist; otherwise the workspace members the
/// repository itself declares. A fresh checkout still gets an overview
/// without pretending that packages are already named domain concepts.
pub fn overview(nodes: &[BundleNode], edges: &[Edge]) -> Option<String> {
    let mut parts: Vec<(&BundleNode, Option<Group>)> = nodes
        .iter()
        .filter(|node| node.kind.is_part())
        .map(|node| (node, tier_of(node)))
        .collect();
    if parts.is_empty() {
        parts = nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.kind,
                    NodeKind::Crate | NodeKind::Package | NodeKind::App
                )
            })
            .map(|node| (node, None))
            .collect();
    }
    if parts.is_empty() {
        parts = nodes
            .iter()
            .filter(|node| node.id == super::derive::ROOT_ID)
            .map(|node| (node, None))
            .collect();
    }
    if parts.is_empty() {
        return None;
    }
    let shown: BTreeSet<&str> = parts.iter().map(|(node, _)| node.id.as_str()).collect();
    let drawn: Vec<&Edge> = edges
        .iter()
        .filter(|edge| {
            drawn_between_parts(edge)
                && shown.contains(edge.from.as_str())
                && shown.contains(edge.to.as_str())
        })
        .collect();
    Some(picture(&parts, &drawn, labelled, &[]))
}

/// Draw exactly the semantic selection made by the focused-view contract.
/// Unlike an overview, files and implementation nodes remain visible here.
pub fn focused(
    nodes: &[BundleNode],
    edges: &[Edge],
    implementation_links: &[(&str, &str)],
) -> String {
    let parts: Vec<(&BundleNode, Option<Group>)> =
        nodes.iter().map(|node| (node, tier_of(node))).collect();
    let drawn: Vec<&Edge> = edges.iter().collect();
    picture(
        &parts,
        &drawn,
        |edge| Some(edge_words(edge)),
        implementation_links,
    )
}

/// Every edge that moves state and the nodes at its ends, grouped by tier,
/// stores as cylinders. Arrows follow the data, so a read points from what
/// is read to its reader.
pub fn data(nodes: &[BundleNode], edges: &[Edge]) -> Option<String> {
    let moving: Vec<&Edge> = edges.iter().filter(|edge| edge.kind.moves_data()).collect();
    let ends: BTreeSet<&str> = moving
        .iter()
        .flat_map(|edge| [edge.from.as_str(), edge.to.as_str()])
        .collect();
    let parts: Vec<(&BundleNode, Option<Group>)> = nodes
        .iter()
        .filter(|node| ends.contains(node.id.as_str()))
        .map(|node| (node, tier_of(node)))
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(picture(&parts, &moving, |edge| Some(edge_words(edge)), &[]))
}

/// The cloud resources that hold state or run (IAM and network wiring stay
/// in Connections), each grouped by the file that declares it, plus every
/// node with an edge to or from one, grouped by tier, with those edges.
pub fn infra(nodes: &[BundleNode], edges: &[Edge]) -> Option<String> {
    let resources: BTreeSet<&str> = nodes
        .iter()
        .filter(|node| {
            node.kind == NodeKind::Resource
                && matches!(node.family, Family::Store | Family::Service)
        })
        .map(|node| node.id.as_str())
        .collect();
    if resources.is_empty() {
        return None;
    }
    let touching: Vec<&Edge> = edges
        .iter()
        .filter(|edge| {
            drawn_between_parts(edge)
                && (resources.contains(edge.from.as_str()) || resources.contains(edge.to.as_str()))
        })
        .collect();
    let users: BTreeSet<&str> = touching
        .iter()
        .flat_map(|edge| [edge.from.as_str(), edge.to.as_str()])
        .collect();
    let common = common_dir(
        nodes
            .iter()
            .filter(|node| resources.contains(node.id.as_str()))
            .filter_map(|node| node.path.as_deref()),
    );
    let parts: Vec<(&BundleNode, Option<Group>)> = nodes
        .iter()
        .filter_map(|node| {
            if resources.contains(node.id.as_str()) {
                Some((node, declaring_file(node, &common)))
            } else if users.contains(node.id.as_str()) {
                Some((node, tier_of(node)))
            } else {
                None
            }
        })
        .collect();
    Some(picture(
        &parts,
        &touching,
        |edge| Some(edge_words(edge)),
        &[],
    ))
}

/// The deepest directory holding every path given; empty for the repo root.
fn common_dir<'a>(paths: impl Iterator<Item = &'a str>) -> String {
    let mut common: Option<Vec<&str>> = None;
    for path in paths {
        let dirs: Vec<&str> = path.split('/').collect();
        let dirs = &dirs[..dirs.len().saturating_sub(1)];
        common = Some(match common {
            None => dirs.to_vec(),
            Some(held) => held
                .iter()
                .zip(dirs)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    common.unwrap_or_default().join("/")
}

/// The group a resource is drawn in: the file that declares it, without
/// its extension, relative to the deepest directory holding every resource
/// drawn, so `infra/src/database/cloudsql.ts` beside `infra/src/compute/*`
/// and `infra/production.release` reads `src/database/cloudsql`.
fn declaring_file(node: &BundleNode, common: &str) -> Option<Group> {
    let path = node.path.as_deref()?;
    let relative = if common.is_empty() {
        path
    } else {
        path.strip_prefix(common)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(path)
    };
    let title = match relative.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() && !stem.ends_with('/') => stem,
        _ => relative,
    };
    Some(Group::File(title.to_owned()))
}

/// Each part a flow touches, drawn once, with one numbered arrow per step
/// that leaves it: arrow `n` goes from step `n`'s part to step `n + 1`'s and
/// says what happens at step `n`. The last step is where the flow arrives,
/// so its part ends the sequence. A step that stays on its part draws a loop.
fn flow(view: &View, nodes: &BTreeMap<&str, &BundleNode>) -> String {
    let mut chart = Chart::new("LR");
    let mut mermaid: BTreeMap<&str, String> = BTreeMap::new();
    let steps: Vec<(&super::model::Step, String)> = view
        .steps
        .iter()
        .flatten()
        .filter_map(|step| {
            let node = nodes.get(step.node.as_str())?;
            let key = match mermaid.get(step.node.as_str()) {
                Some(key) => key.clone(),
                None => {
                    let key = chart.node(node, &node.label, tier_of(node));
                    mermaid.insert(step.node.as_str(), key.clone());
                    key
                }
            };
            Some((step, key))
        })
        .collect();
    for (number, pair) in steps.windows(2).enumerate() {
        let [(step, from), (_, to)] = pair else {
            continue;
        };
        chart.edge(from, to, Some(&format!("{}. {}", number + 1, step.label)));
    }
    chart.finish()
}

/// Neighbours beyond this on one side are grouped by family.
pub const CONNECTIONS_GROUP_OVER: usize = 6;
/// Neighbours drawn on one side at most; the rest are counted in one node.
pub const CONNECTIONS_MAX_PER_SIDE: usize = 40;

/// A labelled edge's words: its verb, then its label, so two edges between
/// one pair that share a label (a write and a read of the same thing) never
/// read alike. The viewer's panel rows say the same.
fn labelled(edge: &Edge) -> Option<String> {
    edge.label
        .as_deref()
        .map(|label| format!("{} · {label}", edge.kind.as_str()))
}

/// What one edge says where every arrow is labelled: its words, else its verb.
fn edge_words(edge: &Edge) -> String {
    labelled(edge).unwrap_or_else(|| edge.kind.as_str().to_owned())
}

/// An edge's ends in the direction its data moves: a read flows from what
/// is read to its reader; every other edge runs from its source.
fn data_way<'a, T>(edge: &Edge, from: &'a T, to: &'a T) -> (&'a T, &'a T)
where
    T: ?Sized,
{
    match edge.kind {
        EdgeKind::Reads => (to, from),
        _ => (from, to),
    }
}

/// One part with what reaches it on the left and what it reaches on the
/// right, each arrow pointing the way the data moves, as Data draws it (a
/// part the focus reads is on the left). A neighbour on both sides is drawn
/// on each, so each side reads whole.
fn connections(focus: &BundleNode, nodes: &BTreeMap<&str, &BundleNode>, edges: &[Edge]) -> String {
    let mut chart = Chart::new("LR");
    let centre = chart.node(focus, &focus.label, None);
    chart.class(&centre, "is-focus".to_owned());
    for side in [Side::Into, Side::OutOf] {
        let mut by_neighbour: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
        for edge in edges {
            let (source, target) = data_way(edge, &edge.from, &edge.to);
            let neighbour = match side {
                Side::Into if *target == focus.id => source,
                Side::OutOf if *source == focus.id => target,
                _ => continue,
            };
            by_neighbour
                .entry(neighbour.as_str())
                .or_default()
                .push(edge);
        }
        let mut neighbours: Vec<(&BundleNode, Vec<&Edge>)> = by_neighbour
            .into_iter()
            .filter_map(|(id, edges)| Some((*nodes.get(id)?, edges)))
            .collect();
        neighbours.sort_by(|(a, _), (b, _)| {
            (a.family, &a.label, &a.id).cmp(&(b.family, &b.label, &b.id))
        });
        let grouped = neighbours.len() > CONNECTIONS_GROUP_OVER;
        let hidden = neighbours.len().saturating_sub(CONNECTIONS_MAX_PER_SIDE);
        for (neighbour, edges) in neighbours.iter().take(CONNECTIONS_MAX_PER_SIDE) {
            let group = grouped.then_some(Group::Family(side, neighbour.family));
            let key = chart.node(neighbour, &neighbour.label, group);
            for edge in edges {
                match side {
                    Side::Into => chart.edge(&key, &centre, Some(&edge_words(edge))),
                    Side::OutOf => chart.edge(&centre, &key, Some(&edge_words(edge))),
                }
            }
        }
        if hidden > 0 {
            let more = chart.more(hidden, None);
            match side {
                Side::Into => chart.dotted(&more, &centre),
                Side::OutOf => chart.dotted(&centre, &more),
            }
        }
    }
    chart.finish()
}

/// The connections view's sources: one per part with at least one edge.
pub fn connections_sources(nodes: &[BundleNode], edges: &[Edge]) -> BTreeMap<String, String> {
    let by_id: BTreeMap<&str, &BundleNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let connected: BTreeSet<&str> = edges
        .iter()
        .flat_map(|edge| [edge.from.as_str(), edge.to.as_str()])
        .collect();
    connected
        .into_iter()
        .filter_map(|id| {
            let focus = by_id.get(id)?;
            Some((id.to_owned(), connections(focus, &by_id, edges)))
        })
        .collect()
}

/// "1 ref", "3 refs".
fn ref_count(count: usize) -> String {
    match count {
        1 => "1 ref".to_owned(),
        count => format!("{count} refs"),
    }
}

/// Fields beyond this many between one collection and another area are
/// counted instead of named.
const FIELDS_NAMED: usize = 3;

/// The records view: "" draws the areas with one arrow per pair of areas a
/// ref crosses, saying how many; each area draws its collections, the refs
/// between them, and the other areas its refs reach or come from.
pub fn records_sources(records: &RecordsModel) -> BTreeMap<String, String> {
    let area_of: BTreeMap<&str, &str> = records
        .collections
        .iter()
        .filter_map(|c| Some((c.id.as_str(), c.parent.as_deref()?)))
        .collect();
    let areas: BTreeMap<&str, &BundleNode> =
        records.areas.iter().map(|a| (a.id.as_str(), a)).collect();
    let mut sources = BTreeMap::new();

    let mut chart = Chart::new("LR");
    let mut keys: BTreeMap<&str, String> = BTreeMap::new();
    for area in &records.areas {
        keys.insert(area.id.as_str(), chart.node(area, &area.label, None));
    }
    let mut crossings: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for edge in &records.edges {
        if let (Some(from), Some(to)) = (
            area_of.get(edge.from.as_str()),
            area_of.get(edge.to.as_str()),
        ) && from != to
        {
            *crossings.entry((from, to)).or_default() += 1;
        }
    }
    for ((from, to), count) in crossings {
        if let (Some(from), Some(to)) = (keys.get(from), keys.get(to)) {
            let (from, to) = (from.clone(), to.clone());
            chart.edge(&from, &to, Some(&ref_count(count)));
        }
    }
    sources.insert(String::new(), chart.finish());

    for area in &records.areas {
        let mut chart = Chart::new("LR");
        let mut keys: BTreeMap<&str, String> = BTreeMap::new();
        for collection in records
            .collections
            .iter()
            .filter(|c| c.parent.as_deref() == Some(area.id.as_str()))
        {
            let key = chart.node(collection, &collection.label, None);
            if let Some(scope) = records.scopes.get(&collection.id) {
                chart.class(&key, format!("scope-{}", scope.as_str()));
            }
            keys.insert(collection.id.as_str(), key);
        }
        // Refs that leave or enter the area, gathered per collection and
        // other area so a much-referenced collection stays one arrow.
        let mut crossing: BTreeMap<(&str, &str, bool), BTreeSet<&str>> = BTreeMap::new();
        for edge in &records.edges {
            let field = edge.label.as_deref().unwrap_or_default();
            let (inside_from, inside_to) = (
                keys.contains_key(edge.from.as_str()),
                keys.contains_key(edge.to.as_str()),
            );
            match (inside_from, inside_to) {
                (true, true) => {
                    let (from, to) = (
                        keys[edge.from.as_str()].clone(),
                        keys[edge.to.as_str()].clone(),
                    );
                    chart.edge(&from, &to, Some(field));
                }
                (true, false) => {
                    if let Some(other) = area_of.get(edge.to.as_str()) {
                        crossing
                            .entry((edge.from.as_str(), other, true))
                            .or_default()
                            .insert(field);
                    }
                }
                (false, true) => {
                    if let Some(other) = area_of.get(edge.from.as_str()) {
                        crossing
                            .entry((edge.to.as_str(), other, false))
                            .or_default()
                            .insert(field);
                    }
                }
                (false, false) => {}
            }
        }
        let mut others: BTreeMap<&str, String> = BTreeMap::new();
        for ((collection, other, outward), fields) in crossing {
            let Some(other_node) = areas.get(other) else {
                continue;
            };
            let other_key = others
                .entry(other)
                .or_insert_with(|| chart.node(other_node, &other_node.label, None))
                .clone();
            let label = if fields.len() > FIELDS_NAMED {
                ref_count(fields.len())
            } else {
                fields.into_iter().collect::<Vec<_>>().join(", ")
            };
            let inside = keys[collection].clone();
            if outward {
                chart.edge(&inside, &other_key, Some(&label));
            } else {
                chart.edge(&other_key, &inside, Some(&label));
            }
        }
        sources.insert(area.id.clone(), chart.finish());
    }
    sources
}

/// A declared view's one source: a flow's numbered steps, or a custom
/// view's parts grouped by tier with every edge between them.
pub fn declared_source(view: &View, nodes: &[BundleNode], edges: &[Edge]) -> String {
    let by_id: BTreeMap<&str, &BundleNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    match view.kind {
        ViewKind::Flow => flow(view, &by_id),
        _ => {
            let ids: Vec<&str> = view.include.iter().flatten().map(String::as_str).collect();
            let drawn: Vec<&Edge> = edges.iter().filter(|e| drawn_between_parts(e)).collect();
            picture(
                &by_tier(&ids, &by_id),
                &drawn,
                |edge| Some(edge_words(edge)),
                &[],
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::NodeKind;
    use super::*;

    #[test]
    fn encodes_ids_for_the_href() {
        assert_eq!(encode_id("door"), "door");
        assert_eq!(encode_id("f:crates/a b.rs"), "f%3Acrates%2Fa%20b.rs");
    }

    fn part(id: &str, family: super::super::model::Family) -> BundleNode {
        BundleNode {
            id: id.to_owned(),
            label: id.to_owned(),
            family,
            kind: NodeKind::Service,
            parent: None,
            path: None,
            summary: None,
            lines: None,
            has_children: false,
            tier: None,
            updated: None,
            refs: super::super::bundle::BundleRefs::default(),
        }
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: from.to_owned(),
            to: to.to_owned(),
            kind: super::super::model::EdgeKind::Calls,
            label: None,
            derived: None,
            evidence: None,
        }
    }

    #[test]
    fn a_crowded_side_groups_by_family_and_counts_what_it_leaves_out() {
        use super::super::model::Family;
        let mut nodes = vec![part("hub", Family::Service), part("user", Family::Surface)];
        let mut edges = vec![edge("user", "hub")];
        for index in 0..45 {
            let family = if index % 3 == 0 {
                Family::Store
            } else {
                Family::Service
            };
            nodes.push(part(&format!("dep-{index:02}"), family));
            edges.push(edge("hub", &format!("dep-{index:02}")));
        }
        let sources = connections_sources(&nodes, &edges);
        let hub = &sources["hub"];
        assert_eq!(
            hub.matches("click ").count(),
            1 + 1 + CONNECTIONS_MAX_PER_SIDE
        );
        let groups: Vec<&str> = hub.lines().filter(|l| l.contains("subgraph")).collect();
        assert_eq!(
            groups,
            vec!["  subgraph g1[\"Services\"]", "  subgraph g2[\"Stores\"]"]
        );
        // The one user is not grouped; the overflow node is outside every
        // group, carries no click and points from the hub.
        assert!(hub.starts_with(
            "flowchart LR\n  n1[\"hub\"]\n  click n1 \"atlas:hub\"\n  n2[\"user\"]\n"
        ));
        assert!(hub.contains("  n43[\"+5 more\"]\n"));
        assert!(!hub.contains("click n43"));
        assert!(hub.contains("  n1 -.-> n43\n"));
        assert!(hub.contains("  class n43 is-more\n"));
        assert!(hub.contains("  n2 -->|\"calls\"| n1\n"));
        assert_eq!(sources.len(), 47);
    }

    #[test]
    fn a_data_arrow_follows_the_data_and_leaves_calls_out() {
        use super::super::model::{EdgeKind, Family};
        let nodes = [part("worker", Family::Service), part("db", Family::Store)];
        let read = Edge {
            kind: EdgeKind::Reads,
            ..edge("worker", "db")
        };
        let source = data(&nodes, &[read, edge("worker", "db")]).unwrap_or_default();
        assert!(source.contains("  n2[(\"db\")]\n"), "{source}");
        let arrows: Vec<&str> = source.lines().filter(|l| l.contains("-->")).collect();
        assert_eq!(arrows, vec!["  n2 -->|\"reads\"| n1"]);
    }

    #[test]
    fn overview_and_focused_views_agree_on_read_and_write_direction() {
        use super::super::model::{EdgeKind, Family};
        let nodes = [part("worker", Family::Service), part("db", Family::Store)];
        let edges = [
            Edge {
                kind: EdgeKind::Reads,
                ..edge("worker", "db")
            },
            Edge {
                kind: EdgeKind::Writes,
                ..edge("worker", "db")
            },
        ];
        let broad = overview(&nodes, &edges).unwrap_or_default();
        assert!(broad.contains("  n2 --> n1\n"), "{broad}");
        assert!(broad.contains("  n1 --> n2\n"), "{broad}");
        let detail = focused(&nodes, &edges, &[]);
        assert!(detail.contains("  n2 -->|\"reads\"| n1\n"), "{detail}");
        assert!(detail.contains("  n1 -->|\"writes\"| n2\n"), "{detail}");
    }

    #[test]
    fn a_write_and_a_read_of_one_thing_read_apart_in_connections() {
        use super::super::model::{EdgeKind, Family};
        let nodes = [part("door", Family::Service), part("blobs", Family::Store)];
        let labelled = |kind| Edge {
            kind,
            label: Some("bot media".to_owned()),
            ..edge("door", "blobs")
        };
        let edges = [labelled(EdgeKind::Writes), labelled(EdgeKind::Reads)];
        let sources = connections_sources(&nodes, &edges);
        // The focus is n1. The read is drawn from the store to the door, as
        // Data draws it, and each arrow says its verb before the label.
        let arrows = |focus: &str| -> Vec<String> {
            sources[focus]
                .lines()
                .filter(|l| l.contains("-->"))
                .map(|l| {
                    let (from, rest) = l.trim().split_once(" -->|").unwrap_or_default();
                    let (label, to) = rest.split_once("| ").unwrap_or_default();
                    let end = |n: &str| if n == "n1" { focus } else { "other" };
                    format!("{} {label} {}", end(from), end(to))
                })
                .collect()
        };
        assert_eq!(
            arrows("door"),
            vec![
                "other \"reads · bot media\" door",
                "door \"writes · bot media\" other"
            ]
        );
        assert_eq!(
            arrows("blobs"),
            vec![
                "other \"writes · bot media\" blobs",
                "blobs \"reads · bot media\" other"
            ]
        );
    }

    #[test]
    fn a_cycle_and_two_kinds_between_parts_keep_each_label_and_direction() {
        use super::super::model::{EdgeKind, Family};
        let nodes = [part("a", Family::Service), part("b", Family::Service)];
        let edges = [
            Edge {
                label: Some("request".to_owned()),
                ..edge("a", "b")
            },
            Edge {
                label: Some("callback".to_owned()),
                ..edge("b", "a")
            },
            Edge {
                kind: EdgeKind::Depends,
                ..edge("a", "b")
            },
        ];
        let sources = connections_sources(&nodes, &edges);
        let a = &sources["a"];
        assert!(a.contains("n2 -->|\"calls · callback\"| n1"), "{a}");
        assert!(a.contains("n1 -->|\"calls · request\"| n3"), "{a}");
        assert!(a.contains("n1 -->|\"depends on\"| n3"), "{a}");
        assert_eq!(a.matches("click n2 \"atlas:b\"").count(), 1);
        assert_eq!(a.matches("click n3 \"atlas:b\"").count(), 1);
    }
}
