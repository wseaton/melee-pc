use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Nameplate { key: String, summary: String },
}

#[cfg(test)]
mod tests {
    use crate::Command;

    #[test]
    fn a_nameplate_round_trips() {
        let sent = Command::Nameplate {
            key: "DEMO-7".to_owned(),
            summary: "Sandbag \"quoted\" summary".to_owned(),
        };
        let line = serde_json::to_string(&sent).unwrap();
        assert_eq!(
            line,
            r#"{"type":"nameplate","key":"DEMO-7","summary":"Sandbag \"quoted\" summary"}"#
        );
        assert_eq!(serde_json::from_str::<Command>(&line).unwrap(), sent);
    }

    #[test]
    fn an_unknown_command_is_an_error() {
        assert!(serde_json::from_str::<Command>(r#"{"type":"self_destruct"}"#).is_err());
    }

    #[test]
    fn a_nameplate_without_a_key_is_an_error() {
        assert!(serde_json::from_str::<Command>(r#"{"type":"nameplate","summary":"x"}"#).is_err());
    }
}
