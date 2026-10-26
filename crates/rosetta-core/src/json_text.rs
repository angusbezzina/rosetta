//! JSON as text: a reader that keeps key order and byte spans, so one value in
//! a hand-formatted file can be replaced without touching the rest, and the
//! writer the committed `.rosetta` files are formatted with.
//!
//! The writer prints a value on one line when it fits in [`LINE_WIDTH`]
//! columns and expands it one entry per line otherwise. That is the layout
//! Biome's JSON formatter settles on for such a file, so a file this writes is
//! one Biome leaves alone.

use serde::Serialize;

pub const LINE_WIDTH: usize = 80;
const INDENT: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A string, number, boolean or null, kept as its source text.
    Scalar(String),
    Array(Vec<Node>),
    Object(Vec<Member>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub value: Value,
    /// Byte range of the value in the text it was read from.
    pub span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The key decoded.
    pub key: String,
    /// The key as written, quotes and escapes included.
    pub raw_key: String,
    pub node: Node,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid JSON at byte {offset}: {why}")]
pub struct ParseError {
    pub offset: usize,
    pub why: &'static str,
}

impl Node {
    pub fn get(&self, key: &str) -> Option<&Node> {
        match &self.value {
            Value::Object(members) => members.iter().find(|m| m.key == key).map(|m| &m.node),
            _ => None,
        }
    }

    pub fn items(&self) -> &[Node] {
        match &self.value {
            Value::Array(items) => items,
            _ => &[],
        }
    }

    /// The decoded string when this node is a JSON string.
    pub fn as_str(&self) -> Option<String> {
        match &self.value {
            Value::Scalar(raw) if raw.starts_with('"') => serde_json::from_str(raw).ok(),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Result<Node, ParseError> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        text,
        at: 0,
    };
    reader.skip_space();
    let node = reader.value()?;
    reader.skip_space();
    if reader.at != reader.bytes.len() {
        return Err(reader.error("trailing text after the value"));
    }
    Ok(node)
}

struct Reader<'a> {
    bytes: &'a [u8],
    text: &'a str,
    at: usize,
}

impl Reader<'_> {
    fn error(&self, why: &'static str) -> ParseError {
        ParseError {
            offset: self.at,
            why,
        }
    }

    fn skip_space(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8, why: &'static str) -> Result<(), ParseError> {
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.error(why))
        }
    }

    fn value(&mut self) -> Result<Node, ParseError> {
        let start = self.at;
        let value = match self.bytes.get(self.at) {
            Some(b'{') => self.object()?,
            Some(b'[') => self.array()?,
            Some(b'"') => Value::Scalar(self.string()?.to_owned()),
            Some(_) => Value::Scalar(self.bare()?.to_owned()),
            None => return Err(self.error("expected a value")),
        };
        Ok(Node {
            value,
            span: start..self.at,
        })
    }

    fn string(&mut self) -> Result<&str, ParseError> {
        let start = self.at;
        self.at += 1;
        loop {
            match self.bytes.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(&self.text[start..self.at]);
                }
                Some(b'\\') => self.at += 2,
                Some(_) => self.at += 1,
                None => return Err(self.error("unterminated string")),
            }
        }
    }

    fn bare(&mut self) -> Result<&str, ParseError> {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'.'))
        {
            self.at += 1;
        }
        let raw = &self.text[start..self.at];
        let valid = matches!(raw, "true" | "false" | "null") || raw.parse::<f64>().is_ok();
        if valid {
            Ok(raw)
        } else {
            self.at = start;
            Err(self.error("expected a value"))
        }
    }

    fn array(&mut self) -> Result<Value, ParseError> {
        self.at += 1;
        let mut items = Vec::new();
        self.skip_space();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_space();
            items.push(self.value()?);
            self.skip_space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.error("expected , or ]")),
            }
        }
    }

    fn object(&mut self) -> Result<Value, ParseError> {
        self.at += 1;
        let mut members = Vec::new();
        self.skip_space();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_space();
            if self.bytes.get(self.at) != Some(&b'"') {
                return Err(self.error("expected a key"));
            }
            let raw_key = self.string()?.to_owned();
            let key = serde_json::from_str(&raw_key).map_err(|_| self.error("invalid key"))?;
            self.skip_space();
            self.eat(b':', "expected :")?;
            self.skip_space();
            let node = self.value()?;
            members.push(Member { key, raw_key, node });
            self.skip_space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(members));
                }
                _ => return Err(self.error("expected , or }")),
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("could not serialize: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error(transparent)]
    Parse(#[from] ParseError),
}

/// Serializes a typed value in the committed layout, ending with a newline.
pub fn to_pretty_string<T: Serialize>(value: &T) -> Result<String, FormatError> {
    let compact = serde_json::to_string(value)?;
    Ok(pretty(&parse(&compact)?))
}

/// Prints a parsed value in the committed layout, ending with a newline.
pub fn pretty(node: &Node) -> String {
    let mut out = String::new();
    write_node(&mut out, &node.value, 0, 0, 0);
    out.push('\n');
    out
}

fn width(text: &str) -> usize {
    text.chars().count()
}

fn inline(value: &Value) -> String {
    match value {
        Value::Scalar(raw) => raw.clone(),
        Value::Array(items) if items.is_empty() => "[]".to_owned(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(|item| inline(&item.value)).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(members) if members.is_empty() => "{}".to_owned(),
        Value::Object(members) => {
            let inner: Vec<String> = members
                .iter()
                .map(|member| format!("{}: {}", member.raw_key, inline(&member.node.value)))
                .collect();
            format!("{{ {} }}", inner.join(", "))
        }
    }
}

/// `lead` is what precedes the value on its line after the indent (a key),
/// `trail` what follows it (a comma), both counted against the width.
fn write_node(out: &mut String, value: &Value, indent: usize, lead: usize, trail: usize) {
    let one_line = inline(value);
    if matches!(value, Value::Scalar(_)) || indent + lead + width(&one_line) + trail <= LINE_WIDTH {
        out.push_str(&one_line);
        return;
    }
    let pad = " ".repeat(indent + INDENT);
    match value {
        Value::Scalar(_) => {}
        Value::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                let last = index + 1 == items.len();
                out.push_str(&pad);
                write_node(out, &item.value, indent + INDENT, 0, usize::from(!last));
                out.push_str(if last { "\n" } else { ",\n" });
            }
            out.push_str(&" ".repeat(indent));
            out.push(']');
        }
        Value::Object(members) => {
            out.push_str("{\n");
            for (index, member) in members.iter().enumerate() {
                let last = index + 1 == members.len();
                out.push_str(&pad);
                out.push_str(&member.raw_key);
                out.push_str(": ");
                write_node(
                    out,
                    &member.node.value,
                    indent + INDENT,
                    width(&member.raw_key) + 2,
                    usize::from(!last),
                );
                out.push_str(if last { "\n" } else { ",\n" });
            }
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_short_values_on_one_line_and_expands_long_ones() {
        let text = r#"{"short":{"a":[1,2]},"long":["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"]}"#;
        let expected = "{\n  \"short\": { \"a\": [1, 2] },\n  \"long\": [\n    \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\n    \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n  ]\n}\n";
        let node = parse(text).map_err(|e| e.to_string());
        assert_eq!(node.map(|n| pretty(&n)), Ok(expected.to_owned()));
    }

    #[test]
    fn records_the_span_of_a_nested_value() {
        let text = "{ \"a\": { \"b\": \"x\\\"y\" } }";
        let Ok(root) = parse(text) else {
            panic!("fixture parses");
        };
        let b = root.get("a").and_then(|a| a.get("b"));
        assert_eq!(b.map(|b| &text[b.span.clone()]), Some("\"x\\\"y\""));
        assert_eq!(b.and_then(Node::as_str), Some("x\"y".to_owned()));
    }

    #[test]
    fn rejects_trailing_text() {
        assert!(parse("{} x").is_err());
    }
}
