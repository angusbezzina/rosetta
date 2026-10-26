use tree_sitter::{Node, Parser};

use super::{Named, NamesError};

pub(super) fn names(
    path: &str,
    text: &str,
    commands: bool,
    ids: bool,
) -> Result<Vec<Named>, NamesError> {
    let language = if path.ends_with(".tsx") {
        tree_sitter_typescript::language_tsx()
    } else {
        tree_sitter_typescript::language_typescript()
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|_| NamesError::Parser(path.to_owned()))?;
    let tree = parser
        .parse(text, None)
        .ok_or_else(|| NamesError::Parser(path.to_owned()))?;
    let mut found = Vec::new();
    visit(tree.root_node(), text, commands, ids, &mut found);
    Ok(found)
}

fn source<'a>(node: Node<'_>, text: &'a str) -> Option<&'a str> {
    text.get(node.byte_range())
}

fn literal(node: Node<'_>, text: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let raw = source(node, text)?;
    if raw.len() < 2 {
        return None;
    }
    match raw.as_bytes()[0] {
        b'"' => serde_json::from_str(raw).ok(),
        b'\'' if raw.ends_with('\'') => Some(raw[1..raw.len() - 1].replace("\\'", "'")),
        _ => None,
    }
}

fn key(node: Node<'_>, text: &str) -> Option<String> {
    let raw = source(node, text)?;
    if node.kind() == "string" {
        literal(node, text)
    } else {
        Some(raw.to_owned())
    }
}

fn first_argument(node: Node<'_>) -> Option<Node<'_>> {
    let arguments = node.child_by_field_name("arguments")?;
    arguments.named_child(0)
}

fn command_arg(node: Node<'_>, text: &str) -> Option<(String, usize)> {
    let arg = first_argument(node)?;
    if let Some(name) = literal(arg, text) {
        return Some((name, arg.start_byte() + 1));
    }
    if arg.kind() != "object" {
        return None;
    }
    let mut cursor = arg.walk();
    for pair in arg.named_children(&mut cursor) {
        if pair.kind() != "pair" {
            continue;
        }
        let name = pair
            .child_by_field_name("key")
            .and_then(|node| key(node, text));
        if name.as_deref() == Some("command") {
            let value = pair.child_by_field_name("value")?;
            return literal(value, text).map(|name| (name, value.start_byte() + 1));
        }
    }
    None
}

fn visit(node: Node<'_>, text: &str, commands: bool, ids: bool, found: &mut Vec<Named>) {
    if ids && node.kind() == "pair" {
        let name = node
            .child_by_field_name("key")
            .and_then(|key_node| key(key_node, text));
        if name.as_deref() == Some("id")
            && let Some(value) = node.child_by_field_name("value")
            && let Some(name) = literal(value, text)
        {
            found.push(Named {
                name,
                kind: "declared identifier",
                offset: value.start_byte() + 1,
            });
        }
    }
    if node.kind() == "call_expression" {
        let function = node.child_by_field_name("function");
        let function_name = function.and_then(|function| {
            let property = function.child_by_field_name("property").unwrap_or(function);
            source(property, text)
        });
        if ids
            && function_name.is_some_and(|name| name.ends_with("Collection"))
            && let Some(first) = first_argument(node)
            && let Some(name) = literal(first, text)
        {
            found.push(Named {
                name,
                kind: "collection id",
                offset: first.start_byte() + 1,
            });
        }
        if commands
            && function_name == Some("command")
            && let Some((raw, offset)) = command_arg(node, text)
            && let Some(name) = raw.split_whitespace().next()
            && !name.is_empty()
        {
            found.push(Named {
                name: name.to_owned(),
                kind: "CLI command",
                offset,
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit(child, text, commands, ids, found);
    }
}

#[cfg(test)]
mod tests {
    use super::names;

    #[test]
    fn commander_yargs_and_declared_ids_are_syntax_bound() -> Result<(), Box<dyn std::error::Error>>
    {
        let text = "const fake = '.command(\\\"workspace-fake\\\")';\nprogram.command('workspace-list <name>');\nyargs.command({ command: 'workspace-show [id]', describe: 'test' });\nconst x = { id: 'workspace_draft' };\nrecordCollection('workspace_notes');";
        let found = names("src/cli.ts", text, true, true)?;
        assert_eq!(
            found
                .iter()
                .map(|name| name.name.as_str())
                .collect::<Vec<_>>(),
            [
                "workspace-list",
                "workspace-show",
                "workspace_draft",
                "workspace_notes"
            ]
        );
        Ok(())
    }
}
