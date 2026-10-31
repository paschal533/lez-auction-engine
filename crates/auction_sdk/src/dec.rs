//! u128 as a decimal string (`#[serde(with = "crate::dec")]`): every boundary the app crosses (FFI
//! JSON, module, `.rep`, QML) loses precision on numbers above 2^53, so amounts never travel as JSON
//! numbers. `dec::opt` does the same for `Option<u128>` (`null` for `None`).

use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(v)
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
    String::deserialize(d)?
        .parse::<u128>()
        .map_err(serde::de::Error::custom)
}

pub mod opt {
    use serde::{Deserialize, Deserializer, Serializer};

    #[allow(clippy::ref_option)] // serde's `with` passes a reference to the field
    pub fn serialize<S: Serializer>(v: &Option<u128>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(v) => s.collect_str(v),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u128>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|s| s.parse::<u128>().map_err(serde::de::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Amounts {
        #[serde(with = "crate::dec")]
        a: u128,
        #[serde(with = "crate::dec::opt")]
        b: Option<u128>,
        #[serde(with = "crate::dec::opt")]
        c: Option<u128>,
    }

    #[test]
    fn u128_round_trips_as_a_decimal_string() {
        let v = Amounts {
            a: u128::MAX,
            b: Some(7),
            c: None,
        };
        let json = serde_json::to_string(&v).expect("json");
        assert_eq!(json, format!(r#"{{"a":"{}","b":"7","c":null}}"#, u128::MAX));
        assert_eq!(serde_json::from_str::<Amounts>(&json).expect("back"), v);
        assert!(serde_json::from_str::<Amounts>(r#"{"a":"x","b":null,"c":null}"#).is_err());
    }
}
