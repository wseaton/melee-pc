use std::fmt;

use melee_events::Centimeters;

use crate::run::RunResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub key: String,
    pub summary: String,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rules {
    pub close_at: Centimeters,
    pub done_status: String,
}

impl Rules {
    pub fn new(min_feet: f64, done_status: String) -> Self {
        Self {
            close_at: Centimeters::from_feet(min_feet),
            done_status,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    HomeRun,
    Bunt,
}

impl Outcome {
    pub fn of(result: &RunResult, rules: &Rules) -> Self {
        if result.distance >= rules.close_at {
            Self::HomeRun
        } else {
            Self::Bunt
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Comment { key: String, body: String },
    Transition { key: String, to: String },
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Comment { key, body } => write!(f, "comment on {key}: {body}"),
            Self::Transition { key, to } => write!(f, "transition {key} to {to}"),
        }
    }
}

fn distance(distance: Centimeters) -> String {
    format!(
        "{} ft ({:.1} m)",
        distance.feet_as_displayed(),
        distance.meters()
    )
}

pub fn plan(ticket: &Ticket, result: &RunResult, rules: &Rules) -> Vec<Action> {
    let batter = result.batter.name();
    let key = ticket.key.clone();
    match Outcome::of(result, rules) {
        Outcome::HomeRun => vec![
            Action::Comment {
                key: key.clone(),
                body: format!(
                    "Closed by a {} home run from {batter} in Home Run Contest. Posted by melee-sidecar.",
                    distance(result.distance)
                ),
            },
            Action::Transition {
                key,
                to: rules.done_status.clone(),
            },
        ],
        Outcome::Bunt => vec![Action::Comment {
            key,
            body: format!(
                "{batter} only sent this {}, short of the {} needed to close it. Needs more info. Posted by melee-sidecar.",
                distance(result.distance),
                distance(rules.close_at)
            ),
        }],
    }
}

#[cfg(test)]
mod tests {
    use melee_events::{Centimeters, Character};

    use crate::plan::{Action, Outcome, Rules, Ticket, plan};
    use crate::run::RunResult;

    fn ticket() -> Ticket {
        Ticket {
            key: "DEMO-7".to_owned(),
            summary: "Sandbag".to_owned(),
            labels: vec!["melee-demo".to_owned()],
        }
    }

    fn rules() -> Rules {
        Rules::new(100.0, "Done".to_owned())
    }

    fn rest(distance: i32) -> RunResult {
        RunResult {
            batter: Character::Jigglypuff,
            distance: Centimeters(distance),
        }
    }

    #[test]
    fn the_threshold_is_given_in_feet_and_held_in_centimeters() {
        assert_eq!(rules().close_at, Centimeters(3048));
        assert_eq!(Rules::new(0.0, "Done".to_owned()).close_at, Centimeters(0));
        assert_eq!(
            Rules::new(154.8, "Done".to_owned()).close_at,
            Centimeters(4719)
        );
    }

    #[test]
    fn the_threshold_itself_is_a_home_run() {
        assert_eq!(Outcome::of(&rest(3048), &rules()), Outcome::HomeRun);
        assert_eq!(Outcome::of(&rest(3047), &rules()), Outcome::Bunt);
        assert_eq!(Outcome::of(&rest(0), &rules()), Outcome::Bunt);
    }

    #[test]
    fn a_home_run_comments_then_closes() {
        assert_eq!(
            plan(&ticket(), &rest(4720), &rules()),
            [
                Action::Comment {
                    key: "DEMO-7".to_owned(),
                    body: "Closed by a 154.8 ft (47.2 m) home run from Jigglypuff in Home Run Contest. Posted by melee-sidecar.".to_owned(),
                },
                Action::Transition {
                    key: "DEMO-7".to_owned(),
                    to: "Done".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_bunt_only_comments() {
        assert_eq!(
            plan(&ticket(), &rest(375), &rules()),
            [Action::Comment {
                key: "DEMO-7".to_owned(),
                body: "Jigglypuff only sent this 12.3 ft (3.8 m), short of the 100.0 ft (30.5 m) needed to close it. Needs more info. Posted by melee-sidecar.".to_owned(),
            }]
        );
    }

    #[test]
    fn the_done_status_is_configurable() {
        let rules = Rules::new(100.0, "Closed".to_owned());
        let actions = plan(&ticket(), &rest(4720), &rules);
        assert_eq!(
            actions.last(),
            Some(&Action::Transition {
                key: "DEMO-7".to_owned(),
                to: "Closed".to_owned()
            })
        );
    }

    #[test]
    fn actions_print_as_the_call_they_stand_for() {
        let actions = plan(&ticket(), &rest(4720), &rules());
        assert!(
            actions[0]
                .to_string()
                .starts_with("comment on DEMO-7: Closed by a 154.8 ft")
        );
        assert_eq!(actions[1].to_string(), "transition DEMO-7 to Done");
    }
}
