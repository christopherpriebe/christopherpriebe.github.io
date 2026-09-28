//! Reading the core's enums of unit variants, such as `Replacement` or
//! `EventKind`, as serde's derive reads them, with one set of visitors for
//! all of them.
//!
//! serde's derive gives each enum visitors of its own, and each is compiled
//! again for every deserializer that reads the enum: JSON text,
//! `serde_json::Value`, and JavaScript values in the facade. Each enum here
//! only lists its variants' names; the visitors below take the names as data,
//! so they are compiled once per deserializer. They make the derive's calls
//! and give its errors word for word: `deserialize_enum` with the enum's name
//! and its variants' names, `expecting` "enum Name", the variant read with
//! `deserialize_identifier` by name, bytes or index, then `unit_variant`.

use std::fmt;

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, Expected, Unexpected, VariantAccess, Visitor,
};

/// Implements `Deserialize` for an enum of unit variants, as serde's derive
/// would for the same names: `deserialize_unit_enum!(Kind { "read" => Read,
/// "write" => Write })`. Every variant must be listed, in the enum's order,
/// with the name its `Serialize` writes.
macro_rules! deserialize_unit_enum {
    ($type:ident { $($name:literal => $variant:ident),+ $(,)? }) => {
        impl<'de> ::serde::Deserialize<'de> for $type {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                // Stops compiling when the enum has a variant not listed.
                const _: fn($type) = |value| match value {
                    $($type::$variant)|+ => {}
                };
                const NAMES: &[&str] = &[$($name),+];
                const VALUES: &[$type] = &[$($type::$variant),+];
                $crate::de::read_unit_enum(deserializer, stringify!($type), NAMES, VALUES)
            }
        }
    };
}

pub(crate) use deserialize_unit_enum;

/// The variant of the enum `name` that `deserializer` holds: the one of
/// `values` at the place of its name in `names`.
///
/// # Errors
///
/// serde's derive's error for a value that is not one of the variants.
pub(crate) fn read_unit_enum<'de, D: Deserializer<'de>, T: Copy>(
    deserializer: D,
    name: &'static str,
    names: &'static [&'static str],
    values: &[T],
) -> Result<T, D::Error> {
    let index = deserializer.deserialize_enum(name, names, EnumVisitor { name, names })?;
    // The index is below `names.len()`, which is `values.len()`.
    values.get(index).copied().ok_or_else(|| {
        let index = u64::try_from(index).unwrap_or(u64::MAX);
        de::Error::invalid_value(Unexpected::Unsigned(index), &IndexBound(names.len()))
    })
}

/// Reads an enum as the derive's `__Visitor` does, giving the variant's
/// index.
struct EnumVisitor {
    /// The enum's name.
    name: &'static str,
    /// Its variants' names, in order.
    names: &'static [&'static str],
}

impl<'de> Visitor<'de> for EnumVisitor {
    type Value = usize;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "enum {}", self.name)
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<usize, A::Error> {
        let (index, variant) = data.variant_seed(VariantIndex(self.names))?;
        variant.unit_variant()?;
        Ok(index)
    }
}

/// Reads a variant's identifier as the derive's `__FieldVisitor` does, by
/// name, by name as bytes or by index, giving the index. The names are its
/// enum's, in order.
struct VariantIndex(&'static [&'static str]);

impl<'de> DeserializeSeed<'de> for VariantIndex {
    type Value = usize;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<usize, D::Error> {
        deserializer.deserialize_identifier(self)
    }
}

impl Visitor<'_> for VariantIndex {
    type Value = usize;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("variant identifier")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<usize, E> {
        usize::try_from(value)
            .ok()
            .filter(|&index| index < self.0.len())
            .ok_or_else(|| E::invalid_value(Unexpected::Unsigned(value), &IndexBound(self.0.len())))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<usize, E> {
        self.0
            .iter()
            .position(|name| *name == value)
            .ok_or_else(|| E::unknown_variant(value, self.0))
    }

    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<usize, E> {
        self.0
            .iter()
            .position(|name| name.as_bytes() == value)
            .ok_or_else(|| E::unknown_variant(&String::from_utf8_lossy(value), self.0))
    }
}

/// What a variant's index is expected to be, below the number of variants:
/// the derive's "variant index 0 <= i < 3".
struct IndexBound(usize);

impl Expected for IndexBound {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "variant index 0 <= i < {}", self.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use serde::de::value::{BytesDeserializer, Error, U64Deserializer};
    use serde::Deserialize;

    /// An enum read by serde's derive.
    mod derived {
        #[derive(Debug, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub(super) enum Policy {
            WriteBack,
            WriteThrough,
            #[serde(rename = "==")]
            Equal,
        }
    }

    /// The same enum, read by the macro.
    mod listed {
        #[derive(Clone, Copy, Debug)]
        pub(super) enum Policy {
            WriteBack,
            WriteThrough,
            Equal,
        }

        deserialize_unit_enum!(Policy {
            "writeBack" => WriteBack,
            "writeThrough" => WriteThrough,
            "==" => Equal,
        });
    }

    /// What each reading gave, as text: the variant, or the error message.
    fn both<A, B>(derived: A, listed: B) -> (String, String)
    where
        A: FnOnce() -> Result<derived::Policy, String>,
        B: FnOnce() -> Result<listed::Policy, String>,
    {
        (format!("{:?}", derived()), format!("{:?}", listed()))
    }

    #[test]
    fn an_enum_reads_and_fails_as_serde_derives_it_from_text_and_from_values() {
        let texts = [
            r#""writeBack""#,
            r#""writeThrough""#,
            r#""==""#,
            r#""WriteBack""#,
            r#""""#,
            r#"{"writeBack": null}"#,
            r#"{"writeBack": 1}"#,
            r#"{"nope": null}"#,
            r#"{"writeBack": null, "writeThrough": null}"#,
            "{}",
            "[]",
            r#"["writeBack"]"#,
            "0",
            "2",
            "3",
            "-1",
            "1.5",
            "null",
            "true",
            r#"{"x": "writeBack"}"#,
        ];
        for text in texts {
            let (a, b) = both(
                || serde_json::from_str(text).map_err(|e| e.to_string()),
                || serde_json::from_str(text).map_err(|e| e.to_string()),
            );
            assert_eq!(a, b, "{text}");
            let value: serde_json::Value = serde_json::from_str(text).unwrap();
            let (a, b) = both(
                || serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
                || serde_json::from_value(value.clone()).map_err(|e| e.to_string()),
            );
            assert_eq!(a, b, "{text} as a value");
        }
        for index in [0_u64, 2, 3, u64::MAX] {
            let read = U64Deserializer::<Error>::new(index);
            let (a, b) = both(
                || derived::Policy::deserialize(read).map_err(|e| e.to_string()),
                || listed::Policy::deserialize(read).map_err(|e| e.to_string()),
            );
            assert_eq!(a, b, "index {index}");
        }
        let bytes = BytesDeserializer::<Error>::new(b"nope\xff");
        let (a, b) = both(
            || derived::Policy::deserialize(bytes).map_err(|e| e.to_string()),
            || listed::Policy::deserialize(bytes).map_err(|e| e.to_string()),
        );
        assert_eq!(a, b);
    }
}
