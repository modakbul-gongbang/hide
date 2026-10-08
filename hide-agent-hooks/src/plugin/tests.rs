//! Each script file's states, its bytes and its writes (PRD opencode-plugin
//! B1, B14, B15; PRD pi-omp-extension B1, B12, B13). Every test builds its
//! own `HOME`; the operator's real agent folders are never a target.

use std::fs;
use std::path::PathBuf;

use super::*;

const FILES: [&PluginFile; 3] = [
    &crate::opencode::PLUGIN,
    &crate::pi_extension::PI,
    &crate::pi_extension::OMP,
];

struct Fixture {
    file: &'static PluginFile,
    home: tempfile::TempDir,
    helper: PathBuf,
}

impl Fixture {
    /// A home where the agent has made its own folder, and a helper.
    fn new(file: &'static PluginFile) -> Self {
        let fixture = Self::without_agent(file);
        fs::create_dir_all(file.config_directory(fixture.home())).unwrap();
        fixture
    }

    fn without_agent(file: &'static PluginFile) -> Self {
        let home = tempfile::tempdir().unwrap();
        let helper = home
            .path()
            .join("hide.app/Contents/Resources/hide-agent-hooks");
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::write(&helper, b"binary").unwrap();
        Self { file, home, helper }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn observe(&self) -> PluginObserved {
        self.file.observe(self.home(), &self.helper)
    }

    fn install(&self) -> Result<bool, String> {
        self.file.install(self.home(), &self.helper)
    }

    fn remove(&self) -> Result<bool, String> {
        self.file.remove(self.home())
    }

    fn folder(&self) -> PathBuf {
        self.file
            .config_directory(self.home())
            .join(self.file.folder)
    }

    fn read(&self) -> String {
        fs::read_to_string(self.file.path(self.home())).unwrap()
    }
}

#[test]
fn each_file_is_its_template_with_the_helper_and_a_marker_that_hashes_the_rest() {
    for file in FILES {
        let text = file.text(Path::new("/kit path/it's \"here\"/hide-agent-hooks"));
        let (first, body) = text.split_once('\n').unwrap();
        assert_eq!(
            first,
            format!(
                "// {}@{} sha256={}",
                file.source_name,
                file.version,
                digest(body)
            )
        );
        // A JSON string literal: quotes and the apostrophe survive as JavaScript.
        assert!(body.contains(r#"const HELPER = "/kit path/it's \"here\"/hide-agent-hooks";"#));
        assert!(!body.contains(HELPER_PLACEHOLDER), "{}", file.agent);
        assert_eq!(
            helper_of(body).as_deref(),
            Some("/kit path/it's \"here\"/hide-agent-hooks")
        );
        for (placeholder, _) in file.constants {
            assert!(!body.contains(placeholder), "{}: {placeholder}", file.agent);
        }
    }
}

#[test]
fn each_template_names_its_helper_and_its_constants_exactly_once() {
    for file in FILES {
        assert_eq!(file.template.matches(HELPER_PLACEHOLDER).count(), 1);
        assert_eq!(
            file.template
                .lines()
                .filter(|line| line.starts_with(HELPER_LINE_START))
                .count(),
            1
        );
        for (placeholder, _) in file.constants {
            assert_eq!(
                file.template.matches(placeholder).count(),
                1,
                "{placeholder}"
            );
        }
    }
}

#[test]
fn pi_and_omp_get_one_source_that_differs_only_in_the_agents_name() {
    let pi = &crate::pi_extension::PI;
    let omp = &crate::pi_extension::OMP;
    assert_eq!(pi.template, omp.template);
    let helper = Path::new("/kit/hide-agent-hooks");
    let (pi_body, omp_body) = (pi.body(helper), omp.body(helper));
    let differing: Vec<_> = pi_body
        .lines()
        .zip(omp_body.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        differing,
        [(r#"const AGENT = "pi";"#, r#"const AGENT = "omp";"#)]
    );
}

#[cfg(unix)]
#[test]
fn install_writes_beside_herdrs_file_and_leaves_it_byte_for_byte() {
    for file in FILES {
        let fixture = Fixture::new(file);
        fs::create_dir_all(fixture.folder()).unwrap();
        let herdr = b"export default function () {}\n";
        fs::write(fixture.folder().join("herdr-agent-state.ts"), herdr).unwrap();
        assert_eq!(fixture.observe(), PluginObserved::Missing);

        assert_eq!(fixture.install(), Ok(true));

        assert_eq!(fixture.observe(), PluginObserved::Current);
        assert_eq!(fixture.read(), file.text(&fixture.helper));
        assert_eq!(
            fs::read(fixture.folder().join("herdr-agent-state.ts")).unwrap(),
            herdr
        );
        // Installing twice converges and writes nothing.
        assert_eq!(fixture.install(), Ok(false));
    }
}

#[cfg(unix)]
#[test]
fn install_makes_the_extension_folder_but_never_the_agents_own() {
    for file in FILES {
        let missing = Fixture::without_agent(file);
        assert_eq!(missing.observe(), PluginObserved::ConfigAbsent);
        let refused = missing.install().unwrap_err();
        assert!(refused.contains("does not exist"), "{refused}");
        assert!(!file.config_directory(missing.home()).exists());

        let fixture = Fixture::new(file);
        assert!(!fixture.folder().exists());
        assert_eq!(fixture.install(), Ok(true));
        assert_eq!(fixture.observe(), PluginObserved::Current);
    }
}

#[cfg(unix)]
#[test]
fn an_edit_reads_edited_and_only_install_puts_hides_back() {
    for file in FILES {
        let fixture = Fixture::new(file);
        fixture.install().unwrap();
        let edited = format!("{}\n// the operator's line\n", fixture.read());
        fs::write(file.path(fixture.home()), &edited).unwrap();

        assert_eq!(fixture.observe(), PluginObserved::Edited);
        // The kit's switch-off takes only an unedited file.
        assert_eq!(fixture.remove(), Ok(false));
        assert_eq!(fixture.read(), edited);
        // Reinstall is the kit calling install on an edited file.
        assert_eq!(fixture.install(), Ok(true));
        assert_eq!(fixture.observe(), PluginObserved::Current);
    }
}

#[cfg(unix)]
#[test]
fn another_builds_file_or_a_gone_helper_reads_stale_with_the_reason() {
    for file in FILES {
        let fixture = Fixture::new(file);
        let other = fixture.home().join("other/hide-agent-hooks");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, b"binary").unwrap();
        file.install(fixture.home(), &other).unwrap();
        match fixture.observe() {
            PluginObserved::Stale(reason) => {
                assert!(reason.contains("not this build's"), "{reason}");
                assert!(reason.contains(file.noun), "{reason}");
            }
            observed => panic!("{observed:?}"),
        }

        fs::remove_file(&other).unwrap();
        match fixture.observe() {
            PluginObserved::Stale(reason) => assert!(reason.contains("which is gone"), "{reason}"),
            observed => panic!("{observed:?}"),
        }

        // An older version Hide wrote, unedited, is replaced without Reinstall.
        let body = file.body(&fixture.helper);
        fs::write(
            file.path(fixture.home()),
            format!("// {}@0 sha256={}\n{body}", file.source_name, digest(&body)),
        )
        .unwrap();
        match fixture.observe() {
            PluginObserved::Stale(reason) => assert!(reason.contains("version 0"), "{reason}"),
            observed => panic!("{observed:?}"),
        }
        assert_eq!(fixture.install(), Ok(true));
        assert_eq!(fixture.observe(), PluginObserved::Current);
    }
}

#[cfg(unix)]
#[test]
fn a_file_hide_did_not_write_is_never_replaced_or_removed() {
    for file in FILES {
        let fixture = Fixture::new(file);
        fs::create_dir_all(fixture.folder()).unwrap();
        let theirs = "export default function () {}\n";
        fs::write(fixture.folder().join(file.file_name), theirs).unwrap();

        assert_eq!(fixture.observe(), PluginObserved::Foreign);
        assert!(fixture.install().is_err());
        assert_eq!(fixture.remove(), Ok(false));
        assert_eq!(fixture.read(), theirs);
    }
}

#[test]
fn the_other_agents_file_in_an_agents_folder_reads_stale_and_is_replaced() {
    let fixture = Fixture::new(&crate::pi_extension::PI);
    fs::create_dir_all(fixture.folder()).unwrap();
    // omp's file copied into Pi's folder by hand is Hide's, but not Pi's.
    fs::write(
        crate::pi_extension::PI.path(fixture.home()),
        crate::pi_extension::OMP.text(&fixture.helper),
    )
    .unwrap();
    assert!(matches!(fixture.observe(), PluginObserved::Stale(_)));
    #[cfg(unix)]
    {
        assert_eq!(fixture.install(), Ok(true));
        assert!(fixture.read().contains(r#"const AGENT = "pi";"#));
    }
}

#[cfg(unix)]
#[test]
fn remove_takes_hides_unedited_file_and_nothing_else() {
    for file in FILES {
        let fixture = Fixture::new(file);
        fixture.install().unwrap();
        fs::write(fixture.folder().join("herdr-agent-state.ts"), b"herdr").unwrap();

        assert_eq!(fixture.remove(), Ok(true));

        assert_eq!(fixture.observe(), PluginObserved::Missing);
        assert_eq!(
            fs::read(fixture.folder().join("herdr-agent-state.ts")).unwrap(),
            b"herdr"
        );
        assert_eq!(fixture.remove(), Ok(false));
    }
}

#[cfg(windows)]
#[test]
fn no_file_is_written_on_windows() {
    for file in FILES {
        let fixture = Fixture::new(file);
        assert!(fixture.install().is_err());
        assert!(!file.path(fixture.home()).exists());
    }
}
