//! The plugin file's states, its bytes and its writes (PRD opencode-plugin
//! B1, B14, B15). Every test builds its own `HOME`; the operator's real
//! OpenCode folder is never a target.

use std::fs;
use std::path::PathBuf;

use super::*;

struct Fixture {
    home: tempfile::TempDir,
    helper: PathBuf,
}

impl Fixture {
    /// A home where OpenCode has made its configuration folder, and a helper.
    fn new() -> Self {
        let fixture = Self::without_opencode();
        fs::create_dir_all(config_directory(fixture.home())).unwrap();
        fixture
    }

    fn without_opencode() -> Self {
        let home = tempfile::tempdir().unwrap();
        let helper = home
            .path()
            .join("hide.app/Contents/Resources/hide-agent-hooks");
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::write(&helper, b"binary").unwrap();
        Self { home, helper }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn observe(&self) -> PluginObserved {
        observe(self.home(), &self.helper)
    }

    fn file(&self) -> String {
        fs::read_to_string(plugin_path(self.home())).unwrap()
    }
}

#[test]
fn the_plugin_is_the_template_with_the_helper_and_a_marker_that_hashes_the_rest() {
    let text = plugin_text(Path::new("/kit path/it's \"here\"/hide-agent-hooks"));
    let (first, body) = text.split_once('\n').unwrap();
    assert_eq!(
        first,
        format!(
            "// hide-opencode-plugin@{PLUGIN_VERSION} sha256={}",
            digest(body)
        )
    );
    // A JSON string literal: quotes and the apostrophe survive as JavaScript.
    assert!(body.contains(r#"const HELPER = "/kit path/it's \"here\"/hide-agent-hooks";"#));
    assert!(!body.contains(HELPER_PLACEHOLDER));
    assert_eq!(
        helper_of(body).as_deref(),
        Some("/kit path/it's \"here\"/hide-agent-hooks")
    );
}

#[test]
fn the_template_names_its_helper_exactly_once() {
    assert_eq!(TEMPLATE.matches(HELPER_PLACEHOLDER).count(), 1);
    assert_eq!(
        TEMPLATE
            .lines()
            .filter(|line| line.starts_with(HELPER_LINE_START))
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn install_writes_beside_herdrs_plugin_and_leaves_it_byte_for_byte() {
    let fixture = Fixture::new();
    let plugins = config_directory(fixture.home()).join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    let herdr = b"export const HerdrAgentState = async () => ({})\n";
    fs::write(plugins.join("herdr-agent-state.js"), herdr).unwrap();
    assert_eq!(fixture.observe(), PluginObserved::Missing);

    assert_eq!(install(fixture.home(), &fixture.helper), Ok(true));

    assert_eq!(fixture.observe(), PluginObserved::Current);
    assert_eq!(fixture.file(), plugin_text(&fixture.helper));
    assert_eq!(
        fs::read(plugins.join("herdr-agent-state.js")).unwrap(),
        herdr
    );
    // Installing twice converges and writes nothing.
    assert_eq!(install(fixture.home(), &fixture.helper), Ok(false));
    assert_eq!(
        installed_helper_path(fixture.home()).as_deref(),
        fixture.helper.to_str()
    );
}

#[cfg(unix)]
#[test]
fn install_makes_the_plugins_folder_but_never_opencodes_own() {
    let missing = Fixture::without_opencode();
    assert_eq!(missing.observe(), PluginObserved::ConfigAbsent);
    let refused = install(missing.home(), &missing.helper).unwrap_err();
    assert!(refused.contains("does not exist"), "{refused}");
    assert!(!config_directory(missing.home()).exists());

    let fixture = Fixture::new();
    assert!(!config_directory(fixture.home()).join("plugins").exists());
    assert_eq!(install(fixture.home(), &fixture.helper), Ok(true));
    assert_eq!(fixture.observe(), PluginObserved::Current);
}

#[cfg(unix)]
#[test]
fn an_edit_reads_edited_and_only_install_puts_hides_back() {
    let fixture = Fixture::new();
    install(fixture.home(), &fixture.helper).unwrap();
    let edited = fixture
        .file()
        .replace("RUNNING_LIMIT = 8", "RUNNING_LIMIT = 2");
    fs::write(plugin_path(fixture.home()), &edited).unwrap();

    assert_eq!(fixture.observe(), PluginObserved::Edited);
    // The kit's switch-off takes only an unedited file.
    assert_eq!(remove(fixture.home()), Ok(false));
    assert_eq!(fixture.file(), edited);
    // Reinstall is the kit calling install on an edited file.
    assert_eq!(install(fixture.home(), &fixture.helper), Ok(true));
    assert_eq!(fixture.observe(), PluginObserved::Current);
}

#[cfg(unix)]
#[test]
fn another_builds_plugin_or_a_gone_helper_reads_stale_with_the_reason() {
    let fixture = Fixture::new();
    let other = fixture.home().join("other/hide-agent-hooks");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, b"binary").unwrap();
    install(fixture.home(), &other).unwrap();
    match fixture.observe() {
        PluginObserved::Stale(reason) => {
            assert!(reason.contains("not this build's"), "{reason}")
        }
        observed => panic!("{observed:?}"),
    }

    fs::remove_file(&other).unwrap();
    match fixture.observe() {
        PluginObserved::Stale(reason) => assert!(reason.contains("which is gone"), "{reason}"),
        observed => panic!("{observed:?}"),
    }

    // An older version Hide wrote, unedited, is replaced without Reinstall.
    let body = body(&fixture.helper);
    fs::write(
        plugin_path(fixture.home()),
        format!("// {PLUGIN_SOURCE_NAME}@0 sha256={}\n{body}", digest(&body)),
    )
    .unwrap();
    match fixture.observe() {
        PluginObserved::Stale(reason) => assert!(reason.contains("version 0"), "{reason}"),
        observed => panic!("{observed:?}"),
    }
    assert_eq!(install(fixture.home(), &fixture.helper), Ok(true));
    assert_eq!(fixture.observe(), PluginObserved::Current);
}

#[cfg(unix)]
#[test]
fn a_file_hide_did_not_write_is_never_replaced_or_removed() {
    let fixture = Fixture::new();
    let plugins = config_directory(fixture.home()).join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    let theirs = "export const Mine = async () => ({})\n";
    fs::write(plugins.join(PLUGIN_FILE_NAME), theirs).unwrap();

    assert_eq!(fixture.observe(), PluginObserved::Foreign);
    assert!(install(fixture.home(), &fixture.helper).is_err());
    assert_eq!(remove(fixture.home()), Ok(false));
    assert_eq!(fixture.file(), theirs);
}

#[cfg(unix)]
#[test]
fn remove_takes_hides_unedited_plugin_and_nothing_else() {
    let fixture = Fixture::new();
    install(fixture.home(), &fixture.helper).unwrap();
    let plugins = config_directory(fixture.home()).join("plugins");
    fs::write(plugins.join("herdr-agent-state.js"), b"herdr").unwrap();

    assert_eq!(remove(fixture.home()), Ok(true));

    assert_eq!(fixture.observe(), PluginObserved::Missing);
    assert_eq!(
        fs::read(plugins.join("herdr-agent-state.js")).unwrap(),
        b"herdr"
    );
    assert_eq!(remove(fixture.home()), Ok(false));
}

#[cfg(windows)]
#[test]
fn the_plugin_is_not_written_on_windows() {
    let fixture = Fixture::new();
    assert!(install(fixture.home(), &fixture.helper).is_err());
    assert!(!plugin_path(fixture.home()).exists());
}
