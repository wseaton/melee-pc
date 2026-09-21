use serde::Deserialize;
use serde_json::Value;
use ujira::{Access, Config, JiraClient};

use crate::error::Error;
use crate::plan::{Action, Ticket};
use crate::scope::{Label, Scope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    DryRun,
    Live,
}

#[derive(Deserialize)]
struct Row {
    key: String,
    fields: Fields,
}

#[derive(Deserialize)]
struct Fields {
    summary: String,
    #[serde(default)]
    labels: Vec<String>,
}

pub fn ticket_from_row(row: Value, label: &Label) -> Result<Ticket, Error> {
    let row: Row = serde_json::from_value(row).map_err(|error| Error::BadRow(error.to_string()))?;
    if !row.fields.labels.iter().any(|have| have == label.as_str()) {
        return Err(Error::LabelMissing {
            key: row.key,
            label: label.to_string(),
        });
    }
    Ok(Ticket {
        key: row.key,
        summary: row.fields.summary,
        labels: row.fields.labels,
    })
}

pub fn resolve_transition<'a>(
    key: &str,
    available: &'a [(String, String)],
    wanted: &str,
) -> Result<&'a str, Error> {
    available
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(wanted.trim()))
        .map(|(id, _)| id.as_str())
        .ok_or_else(|| Error::NoTransition {
            key: key.to_owned(),
            wanted: wanted.to_owned(),
            available: available.iter().map(|(_, name)| name.clone()).collect(),
        })
}

pub fn notice(action: &Action, mode: Mode) -> String {
    match (action, mode) {
        (Action::Comment { key, .. }, Mode::Live) => format!("{key}  comment posted"),
        (Action::Comment { key, .. }, Mode::DryRun) => format!("DRY RUN  would comment on {key}"),
        (Action::Transition { key, to }, Mode::Live) => format!("{key}  moved to {to}"),
        (Action::Transition { key, to }, Mode::DryRun) => {
            format!("DRY RUN  would move {key} to {to}")
        }
    }
}

fn jira<E: std::fmt::Display>(error: E) -> Error {
    Error::Jira(format!("{error:#}"))
}

pub struct Jira {
    client: JiraClient,
    mode: Mode,
}

impl Jira {
    pub fn connect(mode: Mode) -> Result<Self, Error> {
        let mut config = Config::load().map_err(jira)?;
        if mode == Mode::DryRun {
            config.access = Access::ReadOnly;
        }
        Ok(Self {
            client: JiraClient::new(config),
            mode,
        })
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn browse_url(&self, ticket: &Ticket) -> String {
        self.client.browse_url(&ticket.key)
    }

    pub async fn pick(&self, scope: &Scope) -> Result<Ticket, Error> {
        let jql = scope.jql();
        let row = self
            .client
            .search(&jql, 1)
            .await
            .map_err(jira)?
            .into_iter()
            .next()
            .ok_or(Error::NoTicket { jql })?;
        ticket_from_row(row, &scope.label)
    }

    pub async fn require_transition(&self, ticket: &Ticket, to: &str) -> Result<(), Error> {
        let available = self.client.transitions(&ticket.key).await.map_err(jira)?;
        resolve_transition(&ticket.key, &available, to).map(|_| ())
    }

    pub async fn apply(&self, action: &Action) -> Result<(), Error> {
        if self.mode == Mode::DryRun {
            println!("dry-run: would {action}");
            return Ok(());
        }
        println!("live: {action}");
        match action {
            Action::Comment { key, body } => {
                self.client.add_comment(key, body).await.map_err(jira)?;
            }
            Action::Transition { key, to } => {
                let available = self.client.transitions(key).await.map_err(jira)?;
                let id = resolve_transition(key, &available, to)?;
                self.client.transition(key, id).await.map_err(jira)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::error::Error;
    use crate::jira::{Mode, notice, resolve_transition, ticket_from_row};
    use crate::plan::Action;
    use crate::scope::Label;

    fn label() -> Label {
        "melee-demo".parse().unwrap()
    }

    #[test]
    fn a_search_row_becomes_a_ticket() {
        let row = json!({
            "id": "10001",
            "key": "DEMO-7",
            "fields": {
                "summary": "Sandbag",
                "labels": ["other", "melee-demo"],
                "status": {"name": "To Do"}
            }
        });
        let ticket = ticket_from_row(row, &label()).unwrap();
        assert_eq!(ticket.key, "DEMO-7");
        assert_eq!(ticket.summary, "Sandbag");
        assert_eq!(ticket.labels, ["other", "melee-demo"]);
    }

    #[test]
    fn a_row_without_the_label_is_refused() {
        for fields in [
            json!({"summary": "Real work", "labels": ["llm-d"]}),
            json!({"summary": "Real work", "labels": []}),
            json!({"summary": "Real work"}),
            json!({"summary": "Real work", "labels": ["melee-demo-2", "MELEE-DEMO"]}),
        ] {
            let row = json!({"key": "INFERENG-1", "fields": fields});
            match ticket_from_row(row, &label()) {
                Err(Error::LabelMissing { key, label }) => {
                    assert_eq!(key, "INFERENG-1");
                    assert_eq!(label, "melee-demo");
                }
                other => panic!("expected LabelMissing, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_malformed_row_is_an_error() {
        for row in [
            json!({}),
            json!({"key": "DEMO-7"}),
            json!({"key": 7, "fields": {}}),
        ] {
            assert!(matches!(
                ticket_from_row(row, &label()),
                Err(Error::BadRow(_))
            ));
        }
    }

    #[test]
    fn a_notice_says_what_happened_or_what_would_have() {
        let comment = Action::Comment {
            key: "DEMO-7".to_owned(),
            body: "long text the HUD has no room for".to_owned(),
        };
        let close = Action::Transition {
            key: "DEMO-7".to_owned(),
            to: "Closed".to_owned(),
        };
        assert_eq!(notice(&comment, Mode::Live), "DEMO-7  comment posted");
        assert_eq!(notice(&close, Mode::Live), "DEMO-7  moved to Closed");
        assert_eq!(
            notice(&comment, Mode::DryRun),
            "DRY RUN  would comment on DEMO-7"
        );
        assert_eq!(
            notice(&close, Mode::DryRun),
            "DRY RUN  would move DEMO-7 to Closed"
        );
    }

    fn transitions() -> Vec<(String, String)> {
        vec![
            ("11".to_owned(), "In Progress".to_owned()),
            ("31".to_owned(), "Done".to_owned()),
        ]
    }

    #[test]
    fn a_transition_is_found_by_name_ignoring_case() {
        let available = transitions();
        assert_eq!(
            resolve_transition("DEMO-7", &available, "Done").unwrap(),
            "31"
        );
        assert_eq!(
            resolve_transition("DEMO-7", &available, " done ").unwrap(),
            "31"
        );
        assert_eq!(
            resolve_transition("DEMO-7", &available, "in progress").unwrap(),
            "11"
        );
    }

    #[test]
    fn a_missing_transition_lists_what_is_available() {
        let available = transitions();
        let error = resolve_transition("DEMO-7", &available, "Closed").unwrap_err();
        assert_eq!(
            error.to_string(),
            "DEMO-7 has no transition named Closed; available: In Progress, Done"
        );
    }
}
