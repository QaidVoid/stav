//! Converting values to Rust types through serde.
//!
//! A [`Value`] is a [`serde::Deserializer`] whose data model follows the
//! JSON rendering: a type that deserializes from the JSON form of a value
//! also deserializes from the value directly, without rendering it.

use std::fmt;

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde::forward_to_deserialize_any;

use crate::value::Value;

/// Why a value could not be converted to a Rust type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertError {
    message: String,
}

impl ConvertError {
    /// Returns what went wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ConvertError {}

impl de::Error for ConvertError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self {
            message: msg.to_string(),
        }
    }
}

/// Deserializes a value as its JSON rendering reads.
///
/// `none` is a unit and a missing `Option`, strings, symbols, references
/// and paths are strings, structs and maps are maps, lists and sets are
/// sequences, a variant without payload is a string or a unit variant, and
/// a variant with payload is a map with one entry, as an externally tagged
/// enum expects. Integers convert to every integer type that holds them.
///
/// # Examples
///
/// ```
/// use serde::Deserialize;
/// use stav::Value;
///
/// let value = Value::List(vec![Value::Int(80), Value::Int(443)]);
/// let ports = Vec::<u16>::deserialize(&value).unwrap();
/// assert_eq!(ports, [80, 443]);
/// ```
impl<'de> Deserializer<'de> for &'de Value {
    type Error = ConvertError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ConvertError> {
        match self {
            Value::None => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(*b),
            Value::Int(i) => visitor.visit_i64(*i),
            Value::Float(f) => visitor.visit_f64(*f),
            Value::Str(s) | Value::Ref(s) | Value::Symbol { text: s, .. } => {
                visitor.visit_borrowed_str(s)
            }
            Value::Path(p) => match p.to_str() {
                Some(s) => visitor.visit_borrowed_str(s),
                None => visitor.visit_string(p.to_string_lossy().into_owned()),
            },
            Value::List(items) | Value::Set(items) => visitor.visit_seq(Seq(items.iter())),
            Value::Map(entries)
            | Value::Struct {
                fields: entries, ..
            } => visitor.visit_map(Entries::new(entries)),
            Value::Variant {
                name, fields: None, ..
            } => visitor.visit_borrowed_str(name),
            Value::Variant {
                name,
                fields: Some(fields),
                ..
            } => visitor.visit_map(Tagged {
                name: Some(name),
                payload: fields,
            }),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ConvertError> {
        match self {
            Value::None => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ConvertError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ConvertError> {
        match self {
            Value::Variant {
                name, fields: None, ..
            } => visitor.visit_enum(name.as_str().into_deserializer()),
            Value::Variant {
                name,
                fields: Some(fields),
                ..
            } => visitor.visit_enum(Enum {
                name,
                payload: Payload::Fields(fields),
            }),
            Value::Str(s) | Value::Ref(s) | Value::Symbol { text: s, .. } => {
                visitor.visit_enum(s.as_str().into_deserializer())
            }
            Value::Map(entries) if entries.len() == 1 => {
                let (name, value) = &entries[0];
                visitor.visit_enum(Enum {
                    name,
                    payload: Payload::Value(value),
                })
            }
            _ => Err(de::Error::custom(format!(
                "expected a variant, found {}",
                kind(self)
            ))),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::None => "`none`",
        Value::Bool(_) => "a boolean",
        Value::Int(_) => "an integer",
        Value::Float(_) => "a float",
        Value::Str(_) | Value::Path(_) | Value::Symbol { .. } | Value::Ref(_) => "a string",
        Value::List(_) | Value::Set(_) => "a list",
        Value::Map(_) | Value::Struct { .. } => "a map",
        Value::Variant { .. } => "a variant",
    }
}

struct Seq<'de>(std::slice::Iter<'de, Value>);

impl<'de> SeqAccess<'de> for Seq<'de> {
    type Error = ConvertError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, ConvertError> {
        self.0.next().map(|v| seed.deserialize(v)).transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

/// The entries of a struct, a map or a payload, as a map.
struct Entries<'de> {
    entries: std::slice::Iter<'de, (String, Value)>,
    value: Option<&'de Value>,
}

impl<'de> Entries<'de> {
    fn new(entries: &'de [(String, Value)]) -> Self {
        Self {
            entries: entries.iter(),
            value: None,
        }
    }
}

impl<'de> MapAccess<'de> for Entries<'de> {
    type Error = ConvertError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ConvertError> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(key.as_str().into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, ConvertError> {
        match self.value.take() {
            Some(value) => seed.deserialize(value),
            None => Err(de::Error::custom("a map value was read before its key")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

/// A variant with payload as a map with one entry.
struct Tagged<'de> {
    name: Option<&'de str>,
    payload: &'de [(String, Value)],
}

impl<'de> MapAccess<'de> for Tagged<'de> {
    type Error = ConvertError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ConvertError> {
        match self.name.take() {
            Some(name) => seed.deserialize(name.into_deserializer()).map(Some),
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, ConvertError> {
        seed.deserialize(Fields(self.payload))
    }
}

/// The payload fields of a variant, as a map.
struct Fields<'de>(&'de [(String, Value)]);

impl<'de> Deserializer<'de> for Fields<'de> {
    type Error = ConvertError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ConvertError> {
        visitor.visit_map(Entries::new(self.0))
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ConvertError> {
        let values: Vec<&'de Value> = self.0.iter().map(|(_, v)| v).collect();
        visitor.visit_seq(Refs(values.into_iter()))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, ConvertError> {
        self.deserialize_seq(visitor)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct tuple_struct map
        struct enum identifier ignored_any
    }
}

struct Refs<'de>(std::vec::IntoIter<&'de Value>);

impl<'de> SeqAccess<'de> for Refs<'de> {
    type Error = ConvertError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, ConvertError> {
        self.0.next().map(|v| seed.deserialize(v)).transpose()
    }
}

enum Payload<'de> {
    Fields(&'de [(String, Value)]),
    Value(&'de Value),
}

struct Enum<'de> {
    name: &'de str,
    payload: Payload<'de>,
}

impl<'de> EnumAccess<'de> for Enum<'de> {
    type Error = ConvertError;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self), ConvertError> {
        let name = seed.deserialize(self.name.into_deserializer())?;
        Ok((name, self))
    }
}

impl<'de> VariantAccess<'de> for Enum<'de> {
    type Error = ConvertError;

    fn unit_variant(self) -> Result<(), ConvertError> {
        match self.payload {
            Payload::Fields([]) | Payload::Value(Value::None) => Ok(()),
            _ => Err(de::Error::custom(format!(
                "the variant `{}` has payload fields",
                self.name
            ))),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, ConvertError> {
        match self.payload {
            Payload::Fields(fields) => seed.deserialize(Fields(fields)),
            Payload::Value(value) => seed.deserialize(value),
        }
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, ConvertError> {
        match self.payload {
            Payload::Fields(fields) => Fields(fields).deserialize_tuple(len, visitor),
            Payload::Value(value) => value.deserialize_tuple(len, visitor),
        }
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ConvertError> {
        match self.payload {
            Payload::Fields(fields) => visitor.visit_map(Entries::new(fields)),
            Payload::Value(value) => value.deserialize_any(visitor),
        }
    }
}

#[cfg(test)]
#[path = "de_tests.rs"]
mod tests;
