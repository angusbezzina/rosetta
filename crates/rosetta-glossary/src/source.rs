//! `.rosetta/glossary.json` as a person wrote it, before it is trusted.
//!
//! Every field is lenient: absent, the expected shape, or something else
//! kept as JSON text for the message. So any well-formed JSON reads into this
//! model, and the validator can name every problem at the path a person edits
//! before the file is read as a [`Glossary`](super::Glossary).

use std::fmt;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

/// One value as the file holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Field<T> {
    #[default]
    Missing,
    Valid(T),
    /// Present, but not the expected shape: the value as compact JSON.
    Invalid(Unexpected),
}

impl<T> Field<T> {
    pub fn is_present(&self) -> bool {
        !matches!(self, Self::Missing)
    }

    pub fn valid(&self) -> Option<&T> {
        match self {
            Self::Valid(value) => Some(value),
            Self::Missing | Self::Invalid(_) => None,
        }
    }
}

impl Field<String> {
    /// The text, when it is a string that is not blank.
    pub fn text(&self) -> Option<&str> {
        self.valid()
            .map(String::as_str)
            .filter(|text| !text.trim().is_empty())
    }

    /// The value as JSON, or `undefined` when it is absent.
    pub fn as_json(&self) -> String {
        match self {
            Self::Missing => "undefined".to_owned(),
            Self::Valid(text) => json_string(text),
            Self::Invalid(unexpected) => unexpected.0.clone(),
        }
    }
}

impl Field<Vec<Field<String>>> {
    /// Every entry, when this is an array of non-blank strings.
    pub fn texts(&self) -> Option<Vec<&str>> {
        self.valid()?.iter().map(Field::text).collect()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Shape<T> {
    Valid(T),
    Invalid(Unexpected),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Shape::deserialize(deserializer)? {
            Shape::Valid(value) => Self::Valid(value),
            Shape::Invalid(unexpected) => Self::Invalid(unexpected),
        })
    }
}

/// Any JSON value, kept only as its compact text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unexpected(pub String);

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

impl<'de> Deserialize<'de> for Unexpected {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UnexpectedVisitor)
    }
}

struct UnexpectedVisitor;

impl<'de> Visitor<'de> for UnexpectedVisitor {
    type Value = Unexpected;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Unexpected, E> {
        Ok(Unexpected(value.to_string()))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Unexpected, E> {
        Ok(Unexpected(value.to_string()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Unexpected, E> {
        Ok(Unexpected(value.to_string()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Unexpected, E> {
        Ok(Unexpected(
            serde_json::Number::from_f64(value)
                .map_or_else(|| "null".to_owned(), |n| n.to_string()),
        ))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Unexpected, E> {
        Ok(Unexpected(json_string(value)))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Unexpected, E> {
        Ok(Unexpected("null".to_owned()))
    }

    fn visit_none<E: de::Error>(self) -> Result<Unexpected, E> {
        self.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Unexpected, D::Error> {
        Unexpected::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Unexpected, A::Error> {
        let mut items = Vec::new();
        while let Some(Unexpected(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Unexpected(format!("[{}]", items.join(","))))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unexpected, A::Error> {
        let mut members = Vec::new();
        while let Some((key, Unexpected(value))) = map.next_entry::<String, Unexpected>()? {
            members.push(format!("{}:{value}", json_string(&key)));
        }
        Ok(Unexpected(format!("{{{}}}", members.join(","))))
    }
}

/// A JSON object read as `T`; an array or scalar is not one, even where `T`
/// could be read from a sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object<T>(pub T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ObjectVisitor(PhantomData))
    }
}

struct ObjectVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
    type Value = Object<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object")
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Object<T>, A::Error> {
        T::deserialize(MapAccessDeserializer::new(map)).map(Object)
    }
}

/// The keys of an object that no field claimed, in the order written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OtherKeys(pub Vec<String>);

impl<'de> Deserialize<'de> for OtherKeys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(OtherKeysVisitor)
    }
}

struct OtherKeysVisitor;

impl<'de> Visitor<'de> for OtherKeysVisitor {
    type Value = OtherKeys;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<OtherKeys, A::Error> {
        let mut keys = Vec::new();
        while let Some((key, IgnoredAny)) = map.next_entry::<String, IgnoredAny>()? {
            keys.push(key);
        }
        Ok(OtherKeys(keys))
    }
}

type Text = Field<String>;
type Texts = Field<Vec<Field<String>>>;
type Objects<T> = Field<Vec<Field<Object<T>>>>;

/// The whole file: valid only when it is an object.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct GlossarySource(pub Field<Object<SourceGlossary>>);

impl GlossarySource {
    /// Reads any well-formed JSON; only a syntax error fails.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SourceGlossary {
    pub schema: Text,
    pub context: Text,
    pub purpose: Text,
    pub sections: Objects<SourceSection>,
    pub terms: Objects<SourceTerm>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SourceSection {
    pub id: Text,
    pub title: Text,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SourceTerm {
    pub id: Text,
    pub name: Text,
    pub section: Text,
    pub definition: Text,
    pub contexts: Texts,
    pub status: Text,
    pub avoid: Objects<SourceAvoid>,
    pub code: Texts,
    pub refs: Field<Object<SourceRefs>>,
    pub decided: Field<Object<SourceDecision>>,
    pub epic: Text,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SourceAvoid {
    pub word: Text,
    pub except: Texts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SourceRefs {
    pub rules: Texts,
    pub principles: Texts,
    pub docs: Texts,
    #[serde(flatten)]
    pub other: OtherKeys,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SourceDecision {
    pub on: Text,
    pub page: Text,
}
