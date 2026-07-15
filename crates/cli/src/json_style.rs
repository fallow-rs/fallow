use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JsonStyle {
    #[default]
    Compact,
    Pretty,
}

impl JsonStyle {
    pub fn serialize<T: Serialize + ?Sized>(self, value: &T) -> Result<String, serde_json::Error> {
        match self {
            Self::Compact => serde_json::to_string(value),
            Self::Pretty => serde_json::to_string_pretty(value),
        }
    }
}
