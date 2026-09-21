use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Jira(String),
    BadRow(String),
    NoTicket {
        jql: String,
    },
    LabelMissing {
        key: String,
        label: String,
    },
    NoTransition {
        key: String,
        wanted: String,
        available: Vec<String>,
    },
    StreamClosed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io: {error}"),
            Self::Jira(message) => write!(f, "jira: {message}"),
            Self::BadRow(reason) => {
                write!(f, "jira returned a search row this cannot read: {reason}")
            }
            Self::NoTicket { jql } => write!(f, "no open ticket matches: {jql}"),
            Self::LabelMissing { key, label } => {
                write!(
                    f,
                    "{key} does not carry the label {label}, refusing to touch it"
                )
            }
            Self::NoTransition {
                key,
                wanted,
                available,
            } => write!(
                f,
                "{key} has no transition named {wanted}; available: {}",
                available.join(", ")
            ),
            Self::StreamClosed => write!(f, "the game disconnected before a home run result"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
