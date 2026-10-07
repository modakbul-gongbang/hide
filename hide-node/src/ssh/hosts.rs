//! The Host entries Add device offers: the concrete aliases of the account's
//! `~/.ssh/config` (and the files it includes) and where each one leads.
//!
//! Hide stores a device as an alias and a name and resolves everything else
//! through the SSH config at connection time (`runtime/devices.rs`), so this
//! module only reads: it never writes a config file, and it never opens a
//! connection. `ssh -G` prints a host's resolved configuration without
//! connecting, which is how the dialog can say `user@host:port` for an alias
//! and recognise two aliases that reach one machine (B50, B51).
//!
//! Every resource is capped (engineering rule 15): configuration files by
//! count, size and include depth, aliases by [`MAX_HOSTS`], each `ssh -G` by
//! [`RESOLVE_DEADLINE`], and the children by [`CONCURRENCY`] at a time, each an
//! owned child that ends with its tree (`hide_platform::process::run_to_end`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use hide_node_link::device::{
    SshAddress as Address, SshHostEntry as Entry, SshHostListing as Listing,
    SshHostProblem as Problem,
};
use hide_platform::process::{RunFailure, restrict_to_login_environment, run_to_end};

/// Concrete aliases listed; more are reported as truncated.
pub(crate) const MAX_HOSTS: usize = 32;
/// Registered aliases the config does not list that are still resolved, so a
/// device added under a name that is gone from the config can be recognised.
pub(crate) const MAX_EXTRA: usize = 16;
/// How long one `ssh -G` may take.
pub(crate) const RESOLVE_DEADLINE: Duration = Duration::from_secs(3);
/// `ssh -G` children running at once.
const CONCURRENCY: usize = 4;
/// How deep `Include` may nest.
const MAX_INCLUDE_DEPTH: usize = 4;
/// Configuration files read in one listing.
const MAX_CONFIG_FILES: usize = 16;
/// The largest configuration file read; a bigger one is skipped.
const MAX_CONFIG_BYTES: u64 = 256 * 1024;

/// The concrete aliases a config names, in file order.
#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct ConfigAliases {
    pub(crate) aliases: Vec<String>,
    /// More than [`MAX_HOSTS`] concrete aliases were named; the rest are not
    /// listed.
    pub(crate) truncated: bool,
}

/// Reads `<home>/.ssh/config` and what it includes. A missing or unreadable
/// config names no host (the dialog then says where to add one, B52).
pub(crate) fn read_aliases(home: &Path) -> ConfigAliases {
    let mut reader = Reader {
        home,
        files: 0,
        found: ConfigAliases::default(),
    };
    reader.file(&home.join(".ssh").join("config"), 0);
    reader.found
}

struct Reader<'a> {
    home: &'a Path,
    files: usize,
    found: ConfigAliases,
}

impl Reader<'_> {
    fn file(&mut self, path: &Path, depth: usize) {
        if self.files >= MAX_CONFIG_FILES {
            return;
        }
        let Ok(metadata) = std::fs::metadata(path) else {
            return;
        };
        if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
            return;
        }
        let Ok(contents) = std::fs::read_to_string(path) else {
            return;
        };
        self.files += 1;
        for line in contents.lines() {
            let Some((keyword, arguments)) = split_line(line) else {
                continue;
            };
            if keyword.eq_ignore_ascii_case("host") {
                for argument in arguments {
                    self.alias(argument);
                }
            } else if keyword.eq_ignore_ascii_case("include") && depth < MAX_INCLUDE_DEPTH {
                for argument in arguments {
                    for included in self.expand_include(&argument) {
                        self.file(&included, depth + 1);
                    }
                }
            }
        }
    }

    /// Adds `pattern` when it names exactly one host: a pattern with a
    /// wildcard or a negation names none, and a leading `-` could be read as
    /// an option by `ssh`.
    fn alias(&mut self, pattern: String) {
        let concrete = !pattern.is_empty()
            && !pattern.starts_with(['!', '-'])
            && !pattern.contains(['*', '?'])
            && !pattern.chars().any(char::is_control);
        if !concrete || self.found.aliases.contains(&pattern) {
            return;
        }
        if self.found.aliases.len() >= MAX_HOSTS {
            self.found.truncated = true;
            return;
        }
        self.found.aliases.push(pattern);
    }

    /// The files an `Include` argument names: `~` is the account's home, a
    /// relative path is under `~/.ssh`, and `*` or `?` in the last component
    /// match the folder's files, sorted by name as `ssh` reads them.
    fn expand_include(&self, argument: &str) -> Vec<PathBuf> {
        let path = if let Some(rest) = argument.strip_prefix("~/") {
            self.home.join(rest)
        } else if Path::new(argument).is_absolute() {
            PathBuf::from(argument)
        } else {
            self.home.join(".ssh").join(argument)
        };
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return Vec::new();
        };
        if !name.contains(['*', '?']) {
            return vec![path];
        }
        let Some(folder) = path.parent() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut matched: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|candidate| wildcard_match(name, candidate))
            })
            .map(|entry| entry.path())
            .collect();
        matched.sort();
        matched.truncate(MAX_CONFIG_FILES);
        matched
    }
}

/// `*` (any run) and `?` (any one character) against a file name.
fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    fn go(pattern: &[char], candidate: &[char]) -> bool {
        match pattern.split_first() {
            None => candidate.is_empty(),
            Some(('*', rest)) => (0..=candidate.len()).any(|skip| go(rest, &candidate[skip..])),
            Some(('?', rest)) => !candidate.is_empty() && go(rest, &candidate[1..]),
            Some((head, rest)) => candidate.first() == Some(head) && go(rest, &candidate[1..]),
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let candidate: Vec<char> = candidate.chars().collect();
    go(&pattern, &candidate)
}

/// One config line as `ssh` reads it: a keyword, then its arguments, which
/// are separated by blanks or a single `=`, may be double-quoted, and end at
/// an unquoted `#`. A blank or comment line has none.
fn split_line(line: &str) -> Option<(String, Vec<String>)> {
    let line = line.trim_start();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let keyword_end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let keyword = line[..keyword_end].to_owned();
    let mut rest = line[keyword_end..].trim_start();
    rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    let mut arguments = Vec::new();
    let mut chars = rest.chars().peekable();
    while let Some(&next) = chars.peek() {
        if next.is_whitespace() {
            chars.next();
            continue;
        }
        if next == '#' {
            break;
        }
        let mut argument = String::new();
        let mut quoted = false;
        while let Some(&c) = chars.peek() {
            if c == '"' {
                quoted = !quoted;
                chars.next();
            } else if c.is_whitespace() && !quoted {
                break;
            } else {
                argument.push(c);
                chars.next();
            }
        }
        arguments.push(argument);
    }
    Some((keyword, arguments))
}

/// The `user`, `hostname` and `port` lines of `ssh -G`'s output.
pub(crate) fn parse_resolved(output: &str) -> Option<Address> {
    let (mut user, mut host, mut port) = (None, None, None);
    for line in output.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let value = value.trim();
        match key {
            "user" if user.is_none() => user = Some(value.to_owned()),
            "hostname" if host.is_none() => host = Some(value.to_owned()),
            "port" if port.is_none() => port = value.parse::<u16>().ok(),
            _ => {}
        }
    }
    let (user, host, port) = (user?, host?, port?);
    let clean = |value: &str| !value.is_empty() && !value.chars().any(char::is_control);
    (clean(&user) && clean(&host)).then_some(Address { user, host, port })
}

/// Resolves one alias with `ssh -F <config> -G -- <alias>`. `-F` names the
/// account's own config, because `ssh` would otherwise read the passwd
/// entry's home and not the one hided was given.
fn resolve_one(
    ssh: &Path,
    home: &Path,
    alias: &str,
    stop: &AtomicBool,
) -> Result<Address, Problem> {
    let mut command = Command::new(ssh);
    restrict_to_login_environment(&mut command);
    command
        .env("HOME", home)
        .arg("-F")
        .arg(home.join(".ssh").join("config"))
        .args(["-G", "--"])
        .arg(alias);
    match run_to_end(&mut command, RESOLVE_DEADLINE, stop) {
        Ok(finished) if finished.succeeded() => {
            parse_resolved(&finished.stdout).ok_or(Problem::SshFailed)
        }
        Ok(_) => Err(Problem::SshFailed),
        Err(RunFailure::Start(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(Problem::SshMissing)
        }
        Err(RunFailure::TimedOut) => Err(Problem::TimedOut),
        Err(_) => Err(Problem::SshFailed),
    }
}

/// Resolves every alias, [`CONCURRENCY`] at a time, in the order given. The
/// threads end with the call, and `stop` ends the children early.
pub(crate) fn resolve_all(
    ssh: &Path,
    home: &Path,
    aliases: &[String],
    stop: &AtomicBool,
) -> Vec<Result<Address, Problem>> {
    let next = AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Result<Address, Problem>>>> = aliases
        .iter()
        .map(|_| std::sync::Mutex::new(None))
        .collect();
    std::thread::scope(|scope| {
        for _ in 0..CONCURRENCY.min(aliases.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some(alias) = aliases.get(index) else {
                        return;
                    };
                    let outcome = if stop.load(Ordering::SeqCst) {
                        Err(Problem::SshFailed)
                    } else {
                        resolve_one(ssh, home, alias, stop)
                    };
                    if let Ok(mut slot) = results[index].lock() {
                        *slot = Some(outcome);
                    }
                }
            });
        }
    });
    results
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .ok()
                .flatten()
                .unwrap_or(Err(Problem::SshFailed))
        })
        .collect()
}

/// Reads the config and resolves its aliases and `registered` (the aliases of
/// the devices already added), as one bounded run.
pub(crate) fn list(ssh: &Path, home: &Path, registered: &[String], stop: &AtomicBool) -> Listing {
    let config = read_aliases(home);
    let mut wanted = config.aliases.clone();
    let extra: Vec<String> = registered
        .iter()
        .filter(|alias| !wanted.contains(alias) && !alias.starts_with('-'))
        .take(MAX_EXTRA)
        .cloned()
        .collect();
    wanted.extend(extra);
    let mut outcomes = resolve_all(ssh, home, &wanted, stop).into_iter();
    let entries = config
        .aliases
        .iter()
        .map(|alias| Entry {
            alias: alias.clone(),
            address: outcomes.next().unwrap_or(Err(Problem::SshFailed)),
        })
        .collect::<Vec<_>>();
    let extra_resolved = wanted[config.aliases.len()..]
        .iter()
        .zip(outcomes)
        .filter_map(|(alias, outcome)| Some((alias.clone(), outcome.ok()?)));
    let registered = entries
        .iter()
        .filter(|entry| registered.contains(&entry.alias))
        .filter_map(|entry| Some((entry.alias.clone(), entry.address.clone().ok()?)))
        .chain(extra_resolved)
        .collect();
    Listing {
        entries,
        registered,
        truncated: config.truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn aliases(config: &str) -> ConfigAliases {
        let home = tempfile::tempdir().unwrap();
        write(&home.path().join(".ssh/config"), config);
        read_aliases(home.path())
    }

    #[test]
    fn a_host_line_yields_each_concrete_alias_and_no_pattern() {
        let found = aliases(
            "Host studio mini\n  HostName 10.0.0.2\n\
             Host *\n  ServerAliveInterval 30\n\
             Host *.corp !bad build-?\n\
             host=quoted \"two words\"\n\
             Host trailing # a comment\n\
             Host -oProxyCommand=x\n\
             Host studio\n",
        );
        assert_eq!(
            found.aliases,
            ["studio", "mini", "quoted", "two words", "trailing"]
        );
        assert!(!found.truncated);
    }

    #[test]
    fn include_reads_relative_tilde_and_wildcard_files_in_order_within_bounds() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        write(
            &root.join(".ssh/config"),
            "Include conf.d/*.conf\nHost first\nInclude ~/extra\nInclude missing\n",
        );
        write(&root.join(".ssh/conf.d/b.conf"), "Host from-b\n");
        write(&root.join(".ssh/conf.d/a.conf"), "Host from-a\n");
        write(&root.join(".ssh/conf.d/skip.txt"), "Host skipped\n");
        write(&root.join("extra"), "Host from-extra\nInclude extra\n");
        let found = read_aliases(root);
        assert_eq!(found.aliases, ["from-a", "from-b", "first", "from-extra"]);
        // A file that includes itself stops at the depth and file bounds.
        assert!(!found.truncated);
    }

    #[test]
    fn more_than_the_cap_is_truncated_not_listed() {
        let config: String = (0..MAX_HOSTS + 5)
            .map(|index| format!("Host h{index}\n"))
            .collect();
        let found = aliases(&config);
        assert_eq!(found.aliases.len(), MAX_HOSTS);
        assert!(found.truncated);
        assert_eq!(found.aliases[MAX_HOSTS - 1], format!("h{}", MAX_HOSTS - 1));
    }

    #[test]
    fn an_oversized_or_missing_config_names_no_host() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(read_aliases(home.path()), ConfigAliases::default());
        let big = format!("Host big\n{}", "# padding\n".repeat(40_000));
        write(&home.path().join(".ssh/config"), &big);
        assert_eq!(read_aliases(home.path()), ConfigAliases::default());
    }

    #[test]
    fn ssh_g_output_gives_the_address_and_nothing_less_does() {
        let resolved = parse_resolved(
            "host studio\nuser grab\nhostname 10.0.0.2\nport 2222\nidentityfile ~/.ssh/id\n",
        )
        .unwrap();
        assert_eq!(resolved.display(), "grab@10.0.0.2:2222");
        let v6 = parse_resolved("user grab\nhostname ::1\nport 22\n").unwrap();
        assert_eq!(v6.display(), "grab@[::1]:22");
        assert_eq!(parse_resolved("user grab\nhostname h\n"), None);
        assert_eq!(parse_resolved("user grab\nhostname h\nport nope\n"), None);
        assert_eq!(parse_resolved(""), None);
    }

    #[test]
    fn a_wildcard_matches_names_the_way_ssh_reads_include() {
        assert!(wildcard_match("*.conf", "a.conf"));
        assert!(wildcard_match("a?.conf", "ab.conf"));
        assert!(!wildcard_match("*.conf", "a.txt"));
        assert!(!wildcard_match("a?.conf", "a.conf"));
    }

    /// At most [`CONCURRENCY`] children run at once, and every alias is asked
    /// exactly once. The stand-in keeps each call alive briefly, counts how
    /// many are alive when it starts and records the count.
    #[cfg(unix)]
    #[test]
    fn resolving_runs_a_bounded_number_of_children_and_asks_each_alias_once() {
        let folder = tempfile::tempdir().unwrap();
        let ssh = folder.path().join("ssh");
        crate::executable_fixture::write_executable(
            &ssh,
            "#!/bin/sh\ndir=$(dirname \"$0\")\nmkdir -p \"$dir/alive\"\n: > \"$dir/alive/$5\"\n\
             ls \"$dir/alive\" | wc -l >> \"$dir/counts\"\n\
             /bin/sleep 0.2\nrm \"$dir/alive/$5\"\n\
             printf 'user u\\nhostname %s\\nport 22\\n' \"$5\"\n",
        );
        let aliases: Vec<String> = (0..12).map(|index| format!("h{index}")).collect();
        let stop = AtomicBool::new(false);
        let resolved = resolve_all(&ssh, folder.path(), &aliases, &stop);
        assert_eq!(resolved.len(), aliases.len());
        for (alias, outcome) in aliases.iter().zip(&resolved) {
            assert_eq!(outcome.as_ref().unwrap().host, *alias, "in the order asked");
        }
        let counts = std::fs::read_to_string(folder.path().join("counts")).unwrap();
        let counts: Vec<usize> = counts
            .lines()
            .map(|line| line.trim().parse().unwrap())
            .collect();
        assert_eq!(counts.len(), aliases.len(), "each alias was asked once");
        assert!(
            counts.iter().all(|alive| *alive <= CONCURRENCY),
            "{counts:?}"
        );
    }
}
