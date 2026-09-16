use agent_context_labels::{
    EventKind, LocalSessionReader, PLUGIN_ID, SessionEvent, SocketHerdr, StatePaths, Watcher,
    analysis_context, analysis_context_from_session, append_log, apply_hook_payload, context_label,
    exclusive_watcher_lock, migrate_legacy_state, provider, request_refresh,
    set_automatic_summaries,
};
use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use hide_ai::{AiResult, AiRouter, CancelToken, ProviderId};
use hide_session::Agent as SessionAgent;
use std::io::Read;
use std::path::{Path, PathBuf};

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

#[derive(Clone, Copy, ValueEnum)]
enum SessionFormat {
    Claude,
    Codex,
}

impl From<SessionFormat> for SessionAgent {
    fn from(format: SessionFormat) -> Self {
        match format {
            SessionFormat::Claude => Self::Claude,
            SessionFormat::Codex => Self::Codex,
        }
    }
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
    AnalyzeStdin {
        /// Format of a JSONL session transcript. Plain `user:`/`assistant:`
        /// input remains accepted for backwards-compatible evaluation.
        #[arg(long, value_enum, default_value = "claude")]
        agent: SessionFormat,
    },
}

fn home_directory() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is unavailable")
}

fn watch(home: &Path) -> Result<()> {
    // The watcher is the one command that moves state left under the
    // previous plugin id, and it does so before touching its own directory:
    // the lock below would otherwise create it and turn the move into a
    // kept-both.
    let migrated = migrate_legacy_state(home)?;
    let paths = &StatePaths::from_home(home);
    let _lock = exclusive_watcher_lock(paths)?;
    if !migrated.is_empty() {
        append_log(paths, "state_migrated", None, Some(&migrated.join(";")))?;
    }
    // Quiet on purpose: the watcher follows this same file below, and it is
    // what writes a reason, once per change of reason rather than per scan.
    let (ai_settings, _) = provider::settings(home);
    let router = provider::router(&ai_settings, paths);
    append_log(paths, "watcher_started", None, Some(PLUGIN_ID))?;
    append_log(
        paths,
        "ai_settings",
        None,
        Some(&provider::settings_detail(&ai_settings)),
    )?;
    append_log(
        paths,
        "ai_provider_availability",
        None,
        Some(&provider::availability_detail(&router.availability())),
    )?;
    let mut watcher = Watcher::new(
        SocketHerdr::from_environment(home),
        router,
        LocalSessionReader::new(home),
        paths.clone(),
    );
    // From here the choice is re-read on every watcher wake, so a change in
    // Settings reaches the next label without restarting the watcher.
    watcher.follow_ai_settings(home);
    // Ordering is a nicety; a rejected view must not stop status reporting.
    match watcher.apply_priority_view() {
        Ok(()) => append_log(paths, "agent_view_applied", None, None)?,
        Err(error) => append_log(
            paths,
            "agent_view_failed",
            None,
            Some(&format!("{error:#}")),
        )?,
    }
    watcher.run_event_loop()
}

/// One label request over `context`, outside any pane. The request id names
/// the command so its log lines are told apart from the watcher's, and the
/// answer names the provider that produced it.
fn analyze_once(router: &AiRouter, subject: &str, context: &str) -> Result<String> {
    let request = context_label::request(subject, format!("{subject}:manual"), context);
    let AiResult { provider, value } = router
        .execute(&request, &CancelToken::new())
        .map_err(|error| anyhow!("{error}"))?;
    let analysis = context_label::parse(value)?;
    Ok(format!(
        "provider={provider} attention={} task={}",
        if analysis.attention.is_some() {
            "question"
        } else {
            "none"
        },
        analysis.task
    ))
}

fn main() -> Result<()> {
    // Parse first: an invalid invocation or `--help` must not touch state.
    let command = Cli::parse().command;
    let home = home_directory()?;
    let paths = StatePaths::from_home(&home);
    match command {
        Action::Watch => watch(&home),
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
        Action::AnalyzeStdin { agent } => {
            let router = provider::router(&provider::settings_once(&home, &paths), &paths);
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .context("cannot read transcript")?;
            let context = analysis_context_from_session(agent.into(), &input);
            println!("{}", analyze_once(&router, "stdin", &context)?);
            Ok(())
        }
        Action::VerifyProvider { provider } => {
            let provider = ProviderId::from(provider);
            let router =
                provider::router_for(provider, &provider::settings_once(&home, &paths), &paths);
            let states = router.availability();
            println!("availability {}", provider::availability_detail(&states));
            let ready = states.iter().any(|(_, state)| state.is_ready());
            if !ready {
                return Err(anyhow!("provider {provider} cannot answer"));
            }
            let events = [
                SessionEvent {
                    role: "user",
                    kind: EventKind::Human,
                    at_unix_ms: 0,
                    text: "Add compact task labels".to_owned(),
                },
                SessionEvent {
                    role: "assistant",
                    kind: EventKind::Assistant,
                    at_unix_ms: 0,
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
