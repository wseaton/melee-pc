mod error;
mod jira;
mod plan;
mod run;
mod scope;
mod stream;

use std::net::SocketAddr;
use std::process::ExitCode;

use clap::Parser;
use melee_events::Command;
use tokio::net::TcpListener;

use crate::error::Error;
use crate::jira::{Jira, Mode, notice};
use crate::plan::{Outcome, Rules, plan};
use crate::scope::{Label, ProjectKey, Scope};
use crate::stream::Session;

/// Closes a Jira ticket when Sandbag goes far enough in Home Run Contest.
///
/// Listens for the game's NDJSON event stream (start the game with
/// MELEE_EVENTS_ADDR pointing here), picks one open ticket carrying the label,
/// and acts on the first home run result. Nothing is written without --live.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Address to listen on for the game's event stream
    #[arg(long, default_value = "127.0.0.1:7788")]
    addr: SocketAddr,

    /// The only Jira project tickets may come from
    #[arg(long)]
    project: ProjectKey,

    /// Only tickets carrying this label are ever touched
    #[arg(long, default_value = "melee-demo")]
    label: Label,

    /// Distance in feet at which the ticket is closed; anything shorter is a bunt and only gets a comment
    #[arg(long, default_value_t = 100.0)]
    min_feet: f64,

    /// Name of the status (or transition) that closes the ticket
    #[arg(long, default_value = "Done")]
    done_status: String,

    /// Really write to Jira. Without this every write is printed instead
    #[arg(long)]
    live: bool,
}

async fn run(args: Args) -> Result<(), Error> {
    let mode = if args.live { Mode::Live } else { Mode::DryRun };
    let scope = Scope {
        project: args.project,
        label: args.label,
    };
    let rules = Rules::new(args.min_feet, args.done_status);

    let jira = Jira::connect(mode)?;
    let ticket = jira.pick(&scope).await?;
    jira.require_transition(&ticket, &rules.done_status).await?;
    println!("mode: {mode:?}");
    println!(
        "ticket: {} {} ({})",
        ticket.key,
        ticket.summary,
        jira.browse_url(&ticket)
    );
    println!(
        "closes at {} ft, otherwise it is a bunt",
        rules.close_at.feet_as_displayed()
    );

    let listener = TcpListener::bind(args.addr).await?;
    println!("waiting for the game on {}", args.addr);
    let greeting = Command::Nameplate {
        key: ticket.key.clone(),
        summary: ticket.summary.clone(),
    };
    let mut session = Session::accept(&listener, &greeting).await?;
    let result = session.result().await?;
    println!(
        "{} sent the bag {} ft: {:?}",
        result.batter.name(),
        result.distance.feet_as_displayed(),
        Outcome::of(&result, &rules)
    );

    for action in plan(&ticket, &result, &rules) {
        jira.apply(&action).await?;
        let text = notice(&action, jira.mode());
        session.send(&Command::Notice { text }).await?;
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("melee-sidecar: {error}");
            ExitCode::FAILURE
        }
    }
}
