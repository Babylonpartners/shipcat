use std::fmt;
use std::marker::PhantomData;
use serde::de::{Visitor, Deserialize, Deserializer, Error, SeqAccess};
use serde::de::value::{SeqAccessDeserializer};

#[derive(Deserialize, Clone, Default)]
pub struct CommaSeparatedString(
    #[serde(deserialize_with="comma_separated_string")]
    Vec<String>
);

impl Into<Vec<String>> for CommaSeparatedString {
    fn into(self) -> Vec<String> {
        self.0
    }
}

pub fn comma_separated_string<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>
{

    struct CommaSeparatedString(PhantomData<fn() -> Vec<String>>);

    impl<'de> Visitor<'de> for CommaSeparatedString {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("comma-separated string or list")
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<String>, E>
        where
            E: Error,
        {
            Ok(value.split(',').map(String::from).collect())
        }

        fn visit_seq<A>(self, seq: A) -> Result<Vec<String>, A::Error>
        where
            A: SeqAccess<'de>,
        {
            Deserialize::deserialize(SeqAccessDeserializer::new(seq))
        }
    }
    deserializer.deserialize_any(CommaSeparatedString(PhantomData))
}

#[cfg(test)]
mod comma_separated_string_tests {
    use super::{CommaSeparatedString};

    #[test]
    fn deserialize_single_string() {
        let CommaSeparatedString(x) = serde_yaml::from_str("'foo'").unwrap();
        assert_eq!(x, vec!["foo".to_string()]);
    }

    #[test]
    fn deserialize_comma_separated_string() {
        let CommaSeparatedString(x) = serde_yaml::from_str("'foo,bar,blort'").unwrap();
        assert_eq!(x, vec!["foo".to_string(), "bar".to_string(), "blort".to_string()]);
    }

    #[test]
    fn deserialize_empty_list() {
        let CommaSeparatedString(x) = serde_yaml::from_str("[]").unwrap();
        assert_eq!(x, Vec::<String>::new());
    }

    #[test]
    fn deserialize_list() {
        let CommaSeparatedString(x) = serde_yaml::from_str("[foo,bar,blort]").unwrap();
        assert_eq!(x, vec!["foo".to_string(), "bar".to_string(), "blort".to_string()]);
    }
}
