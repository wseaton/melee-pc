use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectKey(String);

impl FromStr for ProjectKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_uppercase());
        let rest_is_plain = chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if starts_with_letter && rest_is_plain && s.len() >= 2 {
            Ok(Self(s.to_owned()))
        } else {
            Err(format!("{s:?} is not a Jira project key like DEMO"))
        }
    }
}

impl fmt::Display for ProjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label(String);

impl Label {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Label {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let plain = s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if plain && !s.is_empty() {
            Ok(Self(s.to_owned()))
        } else {
            Err(format!(
                "{s:?} is not a usable label: letters, digits, '-', '_' and '.' only"
            ))
        }
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub project: ProjectKey,
    pub label: Label,
}

impl Scope {
    pub fn jql(&self) -> String {
        format!(
            "project = \"{}\" AND labels = \"{}\" AND statusCategory != Done ORDER BY created ASC",
            self.project, self.label
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::scope::{Label, ProjectKey, Scope};

    #[test]
    fn project_keys_are_uppercase_identifiers() {
        assert!("DEMO".parse::<ProjectKey>().is_ok());
        assert!("A1_B".parse::<ProjectKey>().is_ok());
        for bad in [
            "",
            "D",
            "demo",
            "1DEMO",
            "DE MO",
            "DEMO\"",
            "DEMO OR project = INFERENG",
        ] {
            assert!(bad.parse::<ProjectKey>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn labels_cannot_carry_jql() {
        assert!("melee-demo".parse::<Label>().is_ok());
        assert!("melee_demo.v2".parse::<Label>().is_ok());
        for bad in ["", "melee demo", "x\" OR labels = \"llm-d", "a,b", "a)"] {
            assert!(bad.parse::<Label>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_query_is_pinned_to_one_project_one_label_and_open_tickets() {
        let scope = Scope {
            project: "DEMO".parse().unwrap(),
            label: "melee-demo".parse().unwrap(),
        };
        assert_eq!(
            scope.jql(),
            r#"project = "DEMO" AND labels = "melee-demo" AND statusCategory != Done ORDER BY created ASC"#
        );
    }
}
