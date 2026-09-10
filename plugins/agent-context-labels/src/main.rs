use agent_context_labels::{
    CliHerdr, LocalSessionReader, PLUGIN_ID, POLL_INTERVAL, SessionEvent, StatePaths, Watcher,
    analysis_context, append_log, apply_hook_payload, apply_priority_agent_view, context_label,
    exclusive_watcher_lock, migrate_legacy_state, provider, request_refresh,
    set_automatic_summaries,
};
use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use hide_ai::{AiRouter, CancelToken, ProviderId};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::SystemTime;

#[derive(Parser)]
#[command(name = "hide-agent-context-labels")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Clone, Copy, ValueEnum)]
enum Provider {
    Codex,
    Claude,
}

impl From<Provider> for ProviderId {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Codex => Self::Codex,
            Provider::Claude => Self::Claude,
        }
    }
}

#[derive(Subcommand)]
enum Action {
    Watch,
    /// Ask the running watcher to re-analyze the focused pane. The watcher owns
    /// every state file, so the request is a marker rather than a second writer.
    RequestRefresh,
    /// Set automatic summaries to an explicit state. Applying the same value
    /// twice leaves the same result.
    SetAutomaticSummaries {
        #[arg(long, action = ArgAction::Set)]
        enabled: bool,
    },
    /// Consume one Claude Code or Codex hook payload from stdin.
    Hook,
    /// Make exactly one synthetic, sanitized request through the named
    /// provider without touching a pane. Prints the provider's availability
    /// and exits non-zero when it cannot answer.
    VerifyProvider {
        #[arg(long, value_enum)]
        provider: Provider,
    },
    /// Classify a transcript from stdin through the configured providers and
    /// print the verdict. Evaluation aid; touches no pane state.
    AnalyzeStdin,
}

fn home_directory() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is unavailable")
}

fn watch(paths: &StatePaths, home: &Path) -> Result<()> {
    let _lock = exclusive_watcher_lock(paths)?;
    let router = provider::router(paths);
    append_log(paths, "watcher_started", None, Some(PLUGIN_ID))?;
    append_log(
        paths,
        "ai_provider_availability",
        None,
        Some(&provider::availability_detail(&router.availability())),
    )?;
    let mut watcher = Watcher::new(
        CliHerdr,
        router,
        LocalSessionReader::new(home),
        paths.clone(),
    );
    // Ordering is a nicety; a rejected view must not stop status reporting.
    match apply_priority_agent_view(home) {
        Ok(()) => append_log(paths, "agent_view_applied", None, None)?,
        Err(error) => append_log(
            paths,
            "agent_view_failed",
            None,
            Some(&format!("{error:#}")),
        )?,
    }
    let mut failing_since: Option<(u32, SystemTime)> = None;
    loop {
        match watcher.scan() {
            Ok(_) => {
                if let Some((count, since)) = failing_since.take() {
                    let seconds = since.elapsed().unwrap_or_default().as_secs();
                    append_log(
                        paths,
                        "watcher_scan_recovered",
                        None,
                        Some(&format!("failures={count};seconds={seconds}")),
                    )?;
                }
            }
            Err(error) => match &mut failing_since {
                Some((count, _)) => *count += 1,
                slot @ None => {
                    // Only the first failure of a streak is logged, so it has to
                    // carry the reason; the recovery record carries the extent.
                    append_log(
                        paths,
                        "watcher_scan_failed",
                        None,
                        Some(&format!("{error:#}")),
                    )?;
                    *slot = Some((1, SystemTime::now()));
                }
            },
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// One label request over `context`, outside any pane. The request id names
/// the command so its log lines are told apart from the watcher's.
fn analyze_once(router: &AiRouter, subject: &str, context: &str) -> Result<String> {
    let request = context_label::request(subject, format!("{subject}:manual"), context);
    let value = router
        .execute(&request, &CancelToken::new())
        .map_err(|error| anyhow!("{error}"))?;
    let analysis = context_label::parse(value)?;
    Ok(format!(
        "attention={} summary={}",
        if analysis.attention.is_some() {
            "question"
        } else {
            "none"
        },
        analysis.summary
    ))
}

fn main() -> Result<()> {
    let home = home_directory()?;
    let migrated = migrate_legacy_state(&home)?;
    let paths = StatePaths::from_home(&home);
    if !migrated.is_empty() {
        append_log(&paths, "state_migrated", None, Some(&migrated.join(";")))?;
    }
    match Cli::parse().command {
        Action::Watch => watch(&paths, &home),
        Action::RequestRefresh => {
            request_refresh(&paths)?;
            println!("refresh requested");
            Ok(())
        }
        Action::SetAutomaticSummaries { enabled } => {
            set_automatic_summaries(&paths, enabled)?;
            let state = if enabled { "enabled" } else { "disabled" };
            append_log(&paths, "automatic_summaries_set", None, Some(state))?;
            println!("automatic summaries {state}");
            Ok(())
        }
        Action::Hook => {
            let pane_id = std::env::var("HERDR_PANE_ID")
                .context("HERDR_PANE_ID is unavailable for hook event")?;
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .context("cannot read hook payload")?;
            let payload: serde_json::Value =
                serde_json::from_str(&input).context("hook payload is invalid")?;
            apply_hook_payload(&paths, &pane_id, &payload)?;
            Ok(())
        }
        Action::AnalyzeStdin => {
            let router = provider::router(&paths);
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .context("cannot read transcript")?;
            println!("{}", analyze_once(&router, "stdin", input.trim())?);
            Ok(())
        }
        Action::VerifyProvider { provider } => {
            let provider = ProviderId::from(provider);
            let router = provider::router_for(provider, &paths);
            let states = router.availability();
            println!("availability {}", provider::availability_detail(&states));
            let ready = states.iter().any(|(_, state)| state.is_ready());
            if !ready {
                return Err(anyhow!("provider {provider} cannot answer"));
            }
            let events = [
                SessionEvent {
                    role: "user",
                    text: "Add compact task labels".to_owned(),
                },
                SessionEvent {
                    role: "assistant",
                    text: "Implementing the labels and waiting for review.".to_owned(),
                },
            ];
            let context = analysis_context(&events);
            let verdict = analyze_once(&router, "verify", &context)?;
            append_log(
                &paths,
                "live_provider_verified",
                None,
                Some(&format!("provider={provider}")),
            )?;
            println!("provider {provider} accepted: {verdict}");
            Ok(())
        }
    }
}
