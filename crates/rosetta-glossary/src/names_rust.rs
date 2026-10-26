use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use tree_sitter::{Node, Parser};

use super::{Named, NamesError, split_words};

#[derive(Clone)]
struct Attr {
    text: String,
    offset: usize,
}

#[derive(Clone)]
struct Item<'a> {
    kind: &'static str,
    attrs: Vec<Attr>,
    body: Node<'a>,
}

struct Member<'a> {
    name: String,
    offset: usize,
    attrs: Vec<Attr>,
    body: Option<Node<'a>>,
    ty: Option<String>,
}

fn text<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
    source.get(node.byte_range())
}

fn attributed<'a>(body: Node<'a>, source: &str, kinds: &[&str]) -> Vec<(Node<'a>, Vec<Attr>)> {
    let mut found = Vec::new();
    let mut attrs = Vec::new();
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "attribute_item" {
            if let Some(content) = text(child, source) {
                attrs.push(Attr {
                    text: content.to_owned(),
                    offset: child.start_byte(),
                });
            }
        } else if child.kind().contains("comment") {
            // Doc comments may sit between an attribute and its item.
        } else if kinds.contains(&child.kind()) {
            found.push((child, std::mem::take(&mut attrs)));
        } else {
            attrs.clear();
        }
    }
    found
}

fn derives(attrs: &[Attr], trait_name: &str) -> bool {
    attrs.iter().any(|attr| {
        attr.text.starts_with("#[derive")
            && attr
                .text
                .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                .any(|word| word == trait_name)
    })
}

fn has(attrs: &[Attr], category: &str, word: &str) -> bool {
    attrs.iter().any(|attr| {
        attr.text.starts_with(&format!("#[{category}")) && {
            let mut quoted = false;
            let unquoted = attr
                .text
                .chars()
                .map(|ch| {
                    if ch == '"' {
                        quoted = !quoted;
                        ' '
                    } else if quoted {
                        ' '
                    } else {
                        ch
                    }
                })
                .collect::<String>();
            unquoted
                .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                .any(|part| part == word)
        }
    })
}

fn value(attrs: &[Attr], category: &str, field: &str) -> Option<(String, usize)> {
    let expression = Regex::new(&format!(r#"\b{}\s*=\s*"([^"]+)""#, regex::escape(field))).ok()?;
    for attr in attrs {
        if !attr.text.starts_with(&format!("#[{category}")) {
            continue;
        }
        if let Some(found) = expression.captures(&attr.text)
            && let Some(capture) = found.get(1)
        {
            return Some((capture.as_str().to_owned(), attr.offset + capture.start()));
        }
    }
    None
}

fn members<'a>(body: Node<'a>, source: &str, kind: &str) -> Vec<Member<'a>> {
    let types = if kind == "enum" {
        &["enum_variant"][..]
    } else {
        &["field_declaration"][..]
    };
    attributed(body, source, types)
        .into_iter()
        .filter_map(|(node, attrs)| {
            let name_node = node.child_by_field_name("name")?;
            let name = text(name_node, source)?.to_owned();
            let body = node.child_by_field_name("body");
            let ty = node
                .child_by_field_name("type")
                .and_then(|node| text(node, source).map(str::to_owned))
                .or_else(|| {
                    body.filter(|node| node.kind() == "ordered_field_declaration_list")
                        .and_then(|body| {
                            let mut cursor = body.walk();
                            body.named_children(&mut cursor)
                                .find(|node| node.kind() != "attribute_item")
                                .and_then(|node| text(node, source).map(str::to_owned))
                        })
                });
            Some(Member {
                name,
                offset: name_node.start_byte(),
                attrs,
                body,
                ty,
            })
        })
        .collect()
}

fn local_type(value: Option<&str>, items: &BTreeMap<String, Item<'_>>) -> Option<String> {
    let mut bare = value?.trim();
    while let Some((outer, inside)) = bare.split_once('<') {
        if !matches!(outer, "Option" | "Box" | "Vec") || !inside.ends_with('>') {
            break;
        }
        bare = &inside[..inside.len() - 1];
    }
    (bare
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && items.contains_key(bare))
    .then(|| bare.to_owned())
}

fn rendered_value(name: &str, rename_all: Option<&str>) -> String {
    let words = split_words(name);
    match rename_all {
        Some("verbatim") => name.to_owned(),
        Some("lower" | "lowercase") => words.join(""),
        Some("UPPER" | "UPPERCASE") => words.join("").to_uppercase(),
        Some("snake_case") => words.join("_"),
        Some("SCREAMING_SNAKE_CASE") => words.join("_").to_uppercase(),
        Some("PascalCase") => words
            .iter()
            .map(|word| {
                let mut chars = word.chars();
                format!(
                    "{}{}",
                    chars.next().unwrap_or_default().to_ascii_uppercase(),
                    chars.as_str()
                )
            })
            .collect(),
        Some("camelCase") => words
            .iter()
            .enumerate()
            .map(|(index, word)| {
                if index == 0 {
                    word.clone()
                } else {
                    let mut chars = word.chars();
                    format!(
                        "{}{}",
                        chars.next().unwrap_or_default().to_ascii_uppercase(),
                        chars.as_str()
                    )
                }
            })
            .collect(),
        _ => words.join("-"),
    }
}

struct Walker<'a> {
    items: BTreeMap<String, Item<'a>>,
    source: &'a str,
    seen: BTreeSet<String>,
    found: Vec<Named>,
}

impl Walker<'_> {
    fn args(&mut self, name: &str) {
        let Some(item) = self.items.get(name).cloned() else {
            return;
        };
        if item.kind != "struct" || !self.seen.insert(format!("args:{name}")) {
            return;
        }
        for field in members(item.body, self.source, "struct") {
            self.field(&field);
        }
    }

    fn field(&mut self, field: &Member<'_>) {
        let Some(ty) = local_type(field.ty.as_deref(), &self.items) else {
            return;
        };
        if has(&field.attrs, "command", "subcommand") {
            self.subcommands(&ty);
        } else if has(&field.attrs, "command", "flatten") {
            self.args(&ty);
        } else if !["long", "short", "skip"]
            .iter()
            .any(|word| has(&field.attrs, "arg", word) || has(&field.attrs, "clap", word))
        {
            self.values(&ty);
        }
    }

    fn subcommands(&mut self, name: &str) {
        let Some(item) = self.items.get(name).cloned() else {
            return;
        };
        if item.kind != "enum"
            || !derives(&item.attrs, "Subcommand")
            || !self.seen.insert(format!("sub:{name}"))
        {
            return;
        }
        for variant in members(item.body, self.source, "enum") {
            if has(&variant.attrs, "command", "skip")
                || has(&variant.attrs, "command", "external_subcommand")
            {
                continue;
            }
            let nested = local_type(variant.ty.as_deref(), &self.items);
            if has(&variant.attrs, "command", "flatten") {
                if let Some(nested) = nested {
                    self.subcommands(&nested);
                }
                continue;
            }
            let (name, offset) = value(&variant.attrs, "command", "name")
                .unwrap_or_else(|| (split_words(&variant.name).join("-"), variant.offset));
            self.found.push(Named {
                name,
                kind: "CLI command",
                offset,
            });
            if let Some(nested) = nested {
                if has(&variant.attrs, "command", "subcommand") {
                    self.subcommands(&nested);
                } else {
                    self.args(&nested);
                }
            }
            if let Some(body) = variant
                .body
                .filter(|body| body.kind() == "field_declaration_list")
            {
                for field in members(body, self.source, "struct") {
                    self.field(&field);
                }
            }
        }
    }

    fn values(&mut self, name: &str) {
        let Some(item) = self.items.get(name).cloned() else {
            return;
        };
        if item.kind != "enum"
            || !derives(&item.attrs, "ValueEnum")
            || !self.seen.insert(format!("value:{name}"))
        {
            return;
        }
        let rename_all = value(&item.attrs, "value", "rename_all").map(|(name, _)| name);
        for variant in members(item.body, self.source, "enum") {
            if has(&variant.attrs, "value", "skip") {
                continue;
            }
            let (name, offset) = value(&variant.attrs, "value", "name").unwrap_or_else(|| {
                (
                    rendered_value(&variant.name, rename_all.as_deref()),
                    variant.offset,
                )
            });
            self.found.push(Named {
                name,
                kind: "CLI command",
                offset,
            });
        }
    }
}

pub(super) fn commands(path: &str, source: &str) -> Result<Vec<Named>, NamesError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::language())
        .map_err(|_| NamesError::Parser(path.to_owned()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| NamesError::Parser(path.to_owned()))?;
    let mut items = BTreeMap::new();
    for (node, attrs) in attributed(tree.root_node(), source, &["struct_item", "enum_item"]) {
        let Some(name) = node
            .child_by_field_name("name")
            .and_then(|name| text(name, source))
        else {
            continue;
        };
        let Some(body) = node.child_by_field_name("body") else {
            continue;
        };
        items.insert(
            name.to_owned(),
            Item {
                kind: if node.kind() == "struct_item" {
                    "struct"
                } else {
                    "enum"
                },
                attrs,
                body,
            },
        );
    }
    let roots = items
        .iter()
        .filter_map(|(name, item)| {
            (item.kind == "struct" && derives(&item.attrs, "Parser")).then_some(name.clone())
        })
        .collect::<Vec<_>>();
    let mut walker = Walker {
        items,
        source,
        seen: BTreeSet::new(),
        found: Vec::new(),
    };
    if roots.is_empty() {
        // Library CLI types can be embedded by a Parser in another crate.
        for name in walker
            .items
            .iter()
            .filter_map(|(name, item)| {
                (item.kind == "enum" && derives(&item.attrs, "Subcommand")).then_some(name.clone())
            })
            .collect::<Vec<_>>()
        {
            walker.subcommands(&name);
        }
    } else {
        for root in roots {
            if let Some((name, offset)) = walker
                .items
                .get(&root)
                .and_then(|item| value(&item.attrs, "command", "name"))
            {
                walker.found.push(Named {
                    name,
                    kind: "CLI command",
                    offset,
                });
            }
            walker.args(&root);
        }
    }
    walker
        .found
        .sort_by(|left, right| (left.offset, &left.name).cmp(&(right.offset, &right.name)));
    walker.found.dedup();
    Ok(walker.found)
}

#[cfg(test)]
mod tests {
    use super::commands;

    #[test]
    fn follows_reachable_clap_commands_and_positional_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "#[derive(Parser)]\n#[command(name = \"tool\")]\nstruct Cli {\n #[command(subcommand)]\n command: Command,\n}\n#[derive(Subcommand)]\nenum Command {\n WorkspaceSwitch,\n Task(TaskArgs),\n #[command(name = \"ws\")]\n Renamed,\n}\n#[derive(Args)]\nstruct TaskArgs {\n #[arg(long)]\n mode: Mode,\n operation: Operation,\n}\n#[derive(ValueEnum)]\nenum Operation {\n WorkspaceReset,\n}\n#[derive(ValueEnum)]\nenum Mode {\n WorkspaceHidden,\n}\n#[derive(Subcommand)]\nenum Unreached {\n WorkspaceOther,\n}\n";
        let found = commands("src/cli.rs", source)?;
        assert_eq!(
            found
                .iter()
                .map(|name| name.name.as_str())
                .collect::<Vec<_>>(),
            ["tool", "workspace-switch", "task", "ws", "workspace-reset"]
        );
        Ok(())
    }
}
