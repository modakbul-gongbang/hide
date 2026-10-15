//! The guardrails of a shell call: the `git` and `gh` commands each
//! protection names, and the files protection over every word.
//!
//! The call is read as the spawn guard reads it (`spawn_guard::shell_line`):
//! one simple command at a time, with quotes, heredoc bodies and comments set
//! aside. A `cd` or `pushd` earlier in the line moves where the commands after
//! it run, and `git -C <dir>` moves one command. A command hidden a layer
//! deeper (`bash -c`, `$(...)`, a script, an alias) is not looked for (D-18).

use std::path::{Path, PathBuf};

use super::facts::{Checkout, Located, Place};
use super::{Dirty, Facts, Refusal, Rule, absolute, protected};
use crate::spawn_guard::{command_words, shell_line};

/// The most words of one call checked against the protected paths.
const WORD_LIMIT: usize = 512;

pub(super) fn shell(
    facts: &Facts<'_>,
    cwd: &Path,
    command: &str,
    dirty: Dirty<'_>,
) -> Option<Refusal> {
    let line = shell_line(command);
    let mut dir = cwd.to_path_buf();
    let mut checked = 0usize;
    for (index, words) in line.commands.iter().enumerate() {
        let (words, _) = command_words(words, &|_| None, true);
        let Some((first, args)) = words.split_first() else {
            continue;
        };
        let program = program(first);
        if matches!(program.as_str(), "cd" | "pushd") {
            if let Some(target) = args.iter().find(|arg| !arg.starts_with('-')) {
                dir = absolute(&facts.home, &dir, Path::new(target));
            }
            continue;
        }
        let refusal = match program.as_str() {
            "git" => git(facts, &dir, args, dirty),
            "gh" => gh(facts, &dir, args),
            _ => None,
        };
        if refusal.is_some() {
            return refusal;
        }
        let targets = line
            .targets
            .iter()
            .filter(|(command, _)| *command == index)
            .map(|(_, target)| target);
        for word in words.iter().chain(targets) {
            for candidate in path_words(word) {
                if checked >= WORD_LIMIT {
                    return None;
                }
                checked += 1;
                if let Some(refusal) = protected(
                    facts,
                    &dir,
                    &absolute(&facts.home, &dir, Path::new(candidate)),
                ) {
                    return Some(refusal);
                }
            }
        }
    }
    None
}

/// A command word's program name: its file name without `.exe`.
fn program(word: &str) -> String {
    let name = Path::new(word)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.len().checked_sub(4) {
        Some(stem) if name[stem..].eq_ignore_ascii_case(".exe") => name[..stem].to_owned(),
        _ => name,
    }
}

/// The parts of a word that may name a path: the word, and the value of a
/// `--name=value` or `NAME=value` word. A word still holding an expansion
/// (`$HOME`, `$(...)`, a backtick) is not read, since its path is unknown.
fn path_words(word: &str) -> Vec<&str> {
    let readable = |candidate: &&str| {
        !candidate.is_empty()
            && !candidate.contains('$')
            && !candidate.contains('`')
            && !candidate.starts_with('-')
    };
    std::iter::once(word)
        .chain(word.split_once('=').map(|(_, value)| value))
        .filter(readable)
        .collect()
}

/// Letters of a short-option cluster (`-fd` is `fd`); `None` for anything else.
fn short(word: &str) -> Option<&str> {
    let letters = word.strip_prefix('-')?;
    (!letters.is_empty()
        && !letters.starts_with('-')
        && letters.chars().all(|c| c.is_ascii_alphabetic()))
    .then_some(letters)
}

fn has(args: &[String], names: &[&str]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| names.contains(&arg.as_str()))
}

fn has_short(args: &[String], letter: char) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .filter_map(|arg| short(arg))
        .any(|letters| letters.contains(letter))
}

/// The checkout and policy project a git command run in `dir` works on.
struct Target<'f> {
    checkout: Checkout,
    project: &'f super::ProjectPolicy,
}

enum Where<'f> {
    Outside,
    Unreadable(&'f super::ProjectPolicy),
    Git(Target<'f>),
}

fn place<'f>(facts: &Facts<'f>, dir: &Path) -> Where<'f> {
    let found: Place = facts.locate(dir);
    match (found.project(facts), found.place) {
        (Some(project), Located::In(checkout)) if checkout.git() => Where::Git(Target {
            checkout,
            project: project.policy,
        }),
        (Some(project), Located::Unreadable) => Where::Unreadable(project.policy),
        _ => Where::Outside,
    }
}

fn git(facts: &Facts<'_>, dir: &Path, args: &[String], dirty: Dirty<'_>) -> Option<Refusal> {
    let mut dir: PathBuf = dir.to_path_buf();
    let mut at = 0;
    // Git's own options before the subcommand.
    while let Some(arg) = args.get(at) {
        match arg.as_str() {
            "-C" => {
                dir = absolute(&facts.home, &dir, Path::new(args.get(at + 1)?));
                at += 2;
            }
            "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path" | "--config-env"
            | "--super-prefix" => at += 2,
            other if other.starts_with('-') => at += 1,
            _ => break,
        }
    }
    let subcommand = args.get(at)?.as_str();
    let rest = &args[at + 1..];
    let target = match place(facts, &dir) {
        Where::Outside => return None,
        Where::Unreadable(project) => {
            let rule = first_rule(project)?;
            return Some(Refusal::unread(rule, "the checkout this command runs in"));
        }
        Where::Git(target) => target,
    };
    let rules = &target.project.rules;
    let checkout = &target.checkout;
    if rules.worktree_folder && checkout.original() {
        let switches = match subcommand {
            "switch" => !has(rest, &["-h", "--help"]),
            "checkout" => checkout_switches(rest, &dir),
            _ => false,
        };
        if switches {
            return Some(Refusal::new(
                Rule::WorktreeFolder,
                "This call changes the branch of the project's original checkout",
            ));
        }
    }
    if rules.base_branch {
        let commits = match subcommand {
            "commit" => true,
            "merge" => !has(rest, &["--abort", "--quit", "--ff-only"]),
            "cherry-pick" | "revert" => !has(rest, &["--abort", "--quit", "--skip"]),
            "am" => !has(
                rest,
                &["--abort", "--quit", "--skip", "--show-current-patch"],
            ),
            "pull" => !has(rest, &["--ff-only"]),
            _ => false,
        };
        let pushes = subcommand == "push";
        if commits || pushes {
            let Some(base) = facts.base(target.project, checkout) else {
                return Some(Refusal::unread(Rule::BaseBranch, "the base branch"));
            };
            if commits && checkout.branch.as_deref() == Some(base.as_str()) {
                return Some(Refusal::new(
                    Rule::BaseBranch,
                    format!("This call makes a commit on the base branch {base}"),
                ));
            }
            if pushes && Push::read(rest).reaches(&base, checkout.branch.as_deref()) {
                return Some(Refusal::new(
                    Rule::BaseBranch,
                    format!("This call pushes to the base branch {base}"),
                ));
            }
        }
    }
    if rules.git_commands {
        if subcommand == "push" {
            let push = Push::read(rest);
            if push.force {
                return Some(Refusal::new(
                    Rule::GitCommands,
                    "This call force-pushes without a lease; use --force-with-lease",
                ));
            }
            if push.no_verify {
                return Some(Refusal::new(
                    Rule::GitCommands,
                    "This call skips Git's hooks with --no-verify; run it without and fix what they report",
                ));
            }
        }
        if subcommand == "commit" && commit_skips_hooks(rest) {
            return Some(Refusal::new(
                Rule::GitCommands,
                "This call skips Git's hooks with --no-verify; run it without and fix what they report",
            ));
        }
        let discards = match subcommand {
            "reset" => has(rest, &["--hard"]),
            "checkout" => checkout_discards(rest, &dir),
            "restore" => restore_discards(rest),
            "clean" => {
                (has(rest, &["--force"]) || has_short(rest, 'f'))
                    && !(has(rest, &["--dry-run"]) || has_short(rest, 'n'))
            }
            "switch" => has(rest, &["-f", "--force", "--discard-changes"]),
            _ => false,
        };
        if discards {
            match dirty(&checkout.root) {
                Some(true) => {
                    return Some(Refusal::new(
                        Rule::GitCommands,
                        "This call discards uncommitted changes; commit or stash them first",
                    ));
                }
                Some(false) => {}
                None => {
                    return Some(Refusal::unread(
                        Rule::GitCommands,
                        "whether the checkout has uncommitted changes",
                    ));
                }
            }
        }
    }
    None
}

fn gh(facts: &Facts<'_>, dir: &Path, args: &[String]) -> Option<Refusal> {
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| !arg.starts_with('-'))
        .take(2)
        .collect();
    let (merge, checkout) = match words.as_slice() {
        ["pr", "merge"] => (true, false),
        ["pr", "checkout"] | ["pr", "co"] => (false, true),
        _ => return None,
    };
    let target = match place(facts, dir) {
        Where::Outside => return None,
        Where::Unreadable(project) => {
            let rules = &project.rules;
            if (merge && rules.base_branch && rules.agent_merge)
                || (checkout && rules.worktree_folder)
            {
                let rule = if merge {
                    Rule::AgentMerge
                } else {
                    Rule::WorktreeFolder
                };
                return Some(Refusal::unread(rule, "the checkout this command runs in"));
            }
            return None;
        }
        Where::Git(target) => target,
    };
    let rules = &target.project.rules;
    if merge && rules.base_branch && rules.agent_merge {
        return Some(Refusal::new(
            Rule::AgentMerge,
            "This call merges a pull request",
        ));
    }
    if checkout && rules.worktree_folder && target.checkout.original() {
        return Some(Refusal::new(
            Rule::WorktreeFolder,
            "This call changes the branch of the project's original checkout",
        ));
    }
    None
}

/// The first protection that is on, for a refusal whose fact was unreadable.
fn first_rule(project: &super::ProjectPolicy) -> Option<Rule> {
    let rules = &project.rules;
    if rules.worktree_folder {
        Some(Rule::WorktreeFolder)
    } else if rules.base_branch {
        Some(Rule::BaseBranch)
    } else if rules.git_commands {
        Some(Rule::GitCommands)
    } else {
        None
    }
}

/// The first word `git checkout` reads as a branch or a path, with whether
/// a `--` came first. `-b`, `-B`, `--orphan`, `--detach` and `-` choose a
/// branch outright (`Some(None)`).
fn checkout_operand(args: &[String]) -> Option<Option<&str>> {
    for arg in args {
        match arg.as_str() {
            "--" => return None,
            "-b" | "-B" | "--orphan" | "--detach" | "-" => return Some(None),
            other if other.starts_with("--orphan=") => return Some(None),
            other if other.starts_with('-') => {}
            other => return Some(Some(other)),
        }
    }
    None
}

fn exists(dir: &Path, word: &str) -> bool {
    word == "." || std::fs::symlink_metadata(dir.join(word)).is_ok()
}

/// Whether `git checkout <args>` changes the branch: it names a branch or a
/// commit rather than a path (D-14).
fn checkout_switches(args: &[String], dir: &Path) -> bool {
    match checkout_operand(args) {
        Some(None) => true,
        Some(Some(word)) => !exists(dir, word),
        None => false,
    }
}

/// Whether `git checkout <args>` overwrites the working tree's changes: a
/// path form (`-- <path>`, `.`, an existing path) or `-f`.
fn checkout_discards(args: &[String], dir: &Path) -> bool {
    if has(args, &["-f", "--force"]) {
        return true;
    }
    if let Some(at) = args.iter().position(|arg| arg == "--") {
        return args.len() > at + 1;
    }
    matches!(checkout_operand(args), Some(Some(word)) if exists(dir, word))
}

/// `git restore` touches the working tree unless it names only the index.
fn restore_discards(args: &[String]) -> bool {
    let staged = has(args, &["--staged"]) || has_short(args, 'S');
    let worktree = has(args, &["--worktree"]) || has_short(args, 'W');
    let operands = args.iter().any(|arg| !arg.starts_with('-'));
    operands && (!staged || worktree)
}

fn commit_skips_hooks(args: &[String]) -> bool {
    let mut words = args.iter();
    while let Some(arg) = words.next() {
        match arg.as_str() {
            "--" => return false,
            "--no-verify" => return true,
            // Options whose value is the next word, which may itself look
            // like `-n`.
            "-m" | "-F" | "-c" | "-C" | "-t" | "--message" | "--file" | "--author" | "--date"
            | "--template" | "--reuse-message" | "--reedit-message" | "--fixup" | "--squash"
            | "--trailer" | "--cleanup" => {
                words.next();
            }
            other => {
                if let Some(letters) = short(other) {
                    for letter in letters.chars() {
                        if letter == 'n' {
                            return true;
                        }
                        // The rest of the cluster is this option's value.
                        if "mFcCt".contains(letter) {
                            break;
                        }
                    }
                }
            }
        }
    }
    false
}

/// What one `git push` asks.
struct Push {
    force: bool,
    no_verify: bool,
    every_branch: bool,
    delete: bool,
    refspecs: Vec<String>,
}

impl Push {
    fn read(args: &[String]) -> Self {
        let mut push = Self {
            force: false,
            no_verify: false,
            every_branch: false,
            delete: false,
            refspecs: Vec::new(),
        };
        let mut positional = Vec::new();
        let mut words = args.iter();
        while let Some(arg) = words.next() {
            match arg.as_str() {
                "--" => {
                    positional.extend(words.by_ref().cloned());
                    break;
                }
                "--force" => push.force = true,
                "--no-verify" => push.no_verify = true,
                "--all" | "--branches" | "--mirror" => push.every_branch = true,
                "--delete" => push.delete = true,
                "--repo" | "-o" | "--push-option" | "--receive-pack" | "--exec" => {
                    words.next();
                }
                other if other.starts_with("--") => {}
                other if short(other).is_some() => {
                    let letters = short(other).unwrap_or_default();
                    for letter in letters.chars() {
                        match letter {
                            'f' => push.force = true,
                            'd' => push.delete = true,
                            // `-o` takes the rest, or the next word.
                            'o' => break,
                            _ => {}
                        }
                    }
                }
                other => positional.push(other.to_owned()),
            }
        }
        // The first operand is the remote; the rest are refspecs.
        push.refspecs = positional.into_iter().skip(1).collect();
        if push.refspecs.iter().any(|refspec| refspec.starts_with('+')) {
            push.force = true;
        }
        push
    }

    /// Whether this push updates `base` on the remote, from a checkout on
    /// `current` (B9).
    fn reaches(&self, base: &str, current: Option<&str>) -> bool {
        if self.every_branch {
            return true;
        }
        if self.refspecs.is_empty() {
            return current == Some(base);
        }
        self.refspecs.iter().any(|refspec| {
            let refspec = refspec.trim_start_matches('+');
            let destination = if self.delete {
                refspec
            } else {
                match refspec.split_once(':') {
                    Some((source, "")) => source,
                    Some((_, destination)) => destination,
                    None if refspec == "HEAD" || refspec == "@" => current.unwrap_or_default(),
                    None => refspec,
                }
            };
            destination
                .strip_prefix("refs/heads/")
                .unwrap_or(destination)
                == base
        })
    }
}
