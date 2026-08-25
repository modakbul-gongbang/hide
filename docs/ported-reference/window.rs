use std::process::Command;

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

/// A local process that renders one herdr session: either the plain local
/// session or a `herdr --remote <host>` view onto another machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionProcess {
    pub pid: String,
    /// Full command line. Terminal emulators title the hosting tab with this
    /// string, which is what makes tab selection possible below.
    pub command: String,
}

/// Parse `ps -axo pid=,command=` output. Lines without a numeric pid or with an
/// empty command are dropped rather than guessed at.
pub fn parse_process_list(output: &str) -> Vec<SessionProcess> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (pid, command) = line.split_once(char::is_whitespace)?;
            if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let command = command.trim();
            if command.is_empty() {
                return None;
            }
            Some(SessionProcess {
                pid: pid.to_string(),
                command: command.to_string(),
            })
        })
        .collect()
}

/// The argument `herdr --remote <arg>` was launched with, if this is a remote view.
fn remote_argument(command: &str) -> Option<&str> {
    let mut fields = command.split_whitespace();
    let executable = fields.next()?;
    // Match the binary, not any command line that merely mentions herdr.
    if executable.rsplit('/').next()? != "herdr" {
        return None;
    }
    let mut fields = fields.peekable();
    while let Some(field) = fields.next() {
        if field == "--remote" {
            return fields.next();
        }
        if let Some(value) = field.strip_prefix("--remote=") {
            return Some(value);
        }
    }
    None
}

fn is_local_session(command: &str) -> bool {
    let mut fields = command.split_whitespace();
    let Some(executable) = fields.next() else {
        return false;
    };
    if executable.rsplit('/').next() != Some("herdr") {
        return false;
    }
    // A bare `herdr` (optionally with --session/flags) attaches the local
    // session. Subcommands like `herdr client` or `herdr api` are machinery,
    // not something the user is looking at.
    if remote_argument(command).is_some() {
        return false;
    }
    match fields.find(|field| !field.starts_with('-')) {
        // `herdr --session foo` still consumes a value that is not a subcommand,
        // so only reject values that name a known non-attaching subcommand.
        Some(word) => !matches!(
            word,
            "client"
                | "api"
                | "server"
                | "agent"
                | "pane"
                | "tab"
                | "workspace"
                | "worktree"
                | "session"
                | "config"
                | "channel"
                | "notification"
                | "integration"
                | "update"
                | "completion"
                | "status"
                | "remote-client-bridge"
        ),
        None => true,
    }
}

/// Whether a running `herdr --remote <arg>` view points at the pet target
/// configured with `host`.
///
/// The two strings routinely disagree: the pet config names an ssh alias
/// (`mini`) while the process was launched with the resolved destination
/// (`grab@grabs-mac-mini`). Compare the host parts with `user@` stripped and
/// accept either being contained in the other. This is a heuristic; it is only
/// ever used to pick which terminal tab to raise, never to route a command.
pub fn remote_argument_matches_host(remote_arg: &str, host: &str) -> bool {
    let strip_user = |value: &str| value.rsplit('@').next().unwrap_or(value).trim().to_string();
    let arg_host = strip_user(remote_arg);
    let target_host = strip_user(host);
    if arg_host.is_empty() || target_host.is_empty() {
        return false;
    }
    arg_host == target_host || arg_host.contains(&target_host) || target_host.contains(&arg_host)
}

/// Pick the local process that displays `ssh_host`'s session, or the local
/// session when `ssh_host` is `None`.
///
/// This is the routing that used to be missing: the old code ran a fixed
/// `pgrep -f "herdr client"` and raised whatever it found first, so clicking a
/// remote agent raised whichever session happened to own that client process.
pub fn select_session_process(
    processes: &[SessionProcess],
    ssh_host: Option<&str>,
) -> Option<SessionProcess> {
    match ssh_host {
        Some(host) => processes
            .iter()
            .find(|process| {
                remote_argument(&process.command)
                    .is_some_and(|arg| remote_argument_matches_host(arg, host))
            })
            .cloned(),
        None => processes
            .iter()
            .find(|process| is_local_session(&process.command))
            .cloned(),
    }
}

/// Escape a string for embedding in an AppleScript double-quoted literal.
pub fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn list_processes() -> anyhow::Result<Vec<SessionProcess>> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()?;
    Ok(parse_process_list(&String::from_utf8_lossy(&output.stdout)))
}

/// Walk a process's parent chain until an app bundle executable (Ghostty, iTerm,
/// Terminal, ...) is found and return its pid, so nothing has to assume window
/// titles or a particular terminal emulator.
#[cfg(target_os = "macos")]
fn hosting_app_pid(start_pid: &str) -> anyhow::Result<String> {
    let mut pid = start_pid.to_string();
    for _ in 0..10 {
        let command = Command::new("ps")
            .args(["-o", "comm=", "-p", &pid])
            .output()?;
        let executable = String::from_utf8_lossy(&command.stdout).trim().to_string();
        if executable.contains(".app/Contents/MacOS") {
            return Ok(pid);
        }
        let parent = Command::new("ps")
            .args(["-o", "ppid=", "-p", &pid])
            .output()?;
        let parent_pid = String::from_utf8_lossy(&parent.stdout).trim().to_string();
        if parent_pid.is_empty() || parent_pid == "1" || parent_pid == "0" {
            break;
        }
        pid = parent_pid;
    }
    anyhow::bail!("no terminal app found above pid {start_pid}")
}

#[cfg(target_os = "macos")]
fn activate_terminal_app(app_pid: &str) -> anyhow::Result<()> {
    let pid = app_pid
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid terminal process id {app_pid}: {error}"))?;
    let application = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .ok_or_else(|| anyhow::anyhow!("terminal process {app_pid} is no longer running"))?;
    #[allow(deprecated)]
    let options = NSApplicationActivationOptions::ActivateAllWindows
        | NSApplicationActivationOptions::ActivateIgnoringOtherApps;
    anyhow::ensure!(
        application.activateWithOptions(options),
        "macOS refused to activate terminal process {app_pid}"
    );
    Ok(())
}

/// AppleScript that activates the app and then selects the tab whose name equals
/// `command`, if the app exposes a tab group.
///
/// Activating the app is not enough on its own: a user viewing the local and a
/// remote herdr session keeps them as two tabs of one terminal window, so
/// `set frontmost` restores whichever tab was last active. Terminal emulators
/// title each tab with the command running in it, which is what the name match
/// below relies on. A miss must be an error: callers cannot claim that they
/// focused a pane when only an unrelated terminal tab came forward.
#[cfg(target_os = "macos")]
fn raise_and_select_tab_script(app_pid: &str, command: &str) -> String {
    let escaped = escape_applescript(command);
    format!(
        "tell application \"System Events\"
tell (first process whose unix id is {app_pid})
if not frontmost then
error \"The terminal app did not become active\"
end if
set selectedHerdrTab to false
repeat with w in windows
repeat with tg in tab groups of w
repeat with r in radio buttons of tg
if name of r is \"{escaped}\" then
click r
set selectedHerdrTab to true
end if
end repeat
end repeat
end repeat
if not selectedHerdrTab then
error \"The Herdr session tab was not found\"
end if
end tell
end tell"
    )
}

/// Bring the terminal tab showing `ssh_host`'s herdr session to the front, or
/// the local session's tab when `ssh_host` is `None`.
#[cfg(target_os = "macos")]
pub fn raise_session_terminal(ssh_host: Option<&str>) -> anyhow::Result<()> {
    let processes = list_processes()?;
    let session = select_session_process(&processes, ssh_host).ok_or_else(|| match ssh_host {
        Some(host) => anyhow::anyhow!("no local `herdr --remote` view onto {host}"),
        None => anyhow::anyhow!("no local herdr session process"),
    })?;
    let app_pid = hosting_app_pid(&session.pid)?;
    activate_terminal_app(&app_pid)?;
    let script = raise_and_select_tab_script(&app_pid, &session.command);
    let output = Command::new("osascript").args(["-e", &script]).output()?;
    anyhow::ensure!(
        output.status.success(),
        "could not activate terminal app: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn raise_session_terminal(_ssh_host: Option<&str>) -> anyhow::Result<()> {
    anyhow::bail!("terminal raising is only supported on macOS")
}

/// Bring the terminal app hosting the local herdr session to the front.
/// Retained as the target-agnostic fallback for callers with no target context.
pub fn raise_herdr_terminal() -> anyhow::Result<()> {
    raise_session_terminal(None)
}

/// Raise a terminal window whose title was set through Herdr's window-title API.
/// Missing Accessibility permission is returned as an error for the UI to explain.
pub fn raise_terminal_window(title: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let escaped = title.replace('"', "\\\"");
        let script = format!(
            "tell application \"System Events\"\nrepeat with p in (every process whose background only is false)\nrepeat with w in windows of p\nif (name of w contains \"{escaped}\") then\nset frontmost of p to true\nperform action \"AXRaise\" of w\nreturn\nend if\nend repeat\nend repeat\nerror \"No matching terminal window\"\nend tell"
        );
        let output = Command::new("osascript").args(["-e", &script]).output()?;
        anyhow::ensure!(
            output.status.success(),
            "macOS Accessibility could not raise the terminal window: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = title;
        anyhow::bail!("terminal window raising is only supported on macOS");
    }
}

/// A connected display in physical pixels, decoupled from Tauri's monitor type
/// so the placement rules below stay unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Distance a saved position may sit past a monitor edge and still be trusted.
const RESTORE_MARGIN: i32 = 48;

/// A saved position is only trusted when it still lands on a connected monitor.
/// Stale coordinates (unplugged display, corrupted state file) otherwise strand
/// the pet off-screen, where `show()` succeeds but the user sees nothing.
pub fn position_on_any_monitor(position: [i32; 2], monitors: &[MonitorRect]) -> bool {
    if monitors.is_empty() {
        return true;
    }
    monitors.iter().any(|monitor| {
        position[0] >= monitor.x - RESTORE_MARGIN
            && position[0] <= monitor.x + monitor.width as i32 - RESTORE_MARGIN
            && position[1] >= monitor.y - RESTORE_MARGIN
            && position[1] <= monitor.y + monitor.height as i32 - RESTORE_MARGIN
    })
}

/// Whether a screen point falls inside `monitor`.
///
/// Used to decide when a cached monitor is still the right one during a drag.
/// Asking the OS which monitor a window is on costs a round trip, and doing it
/// per frame is what made fast drags stutter; this arithmetic replaces it for
/// every frame that stays on the same display.
pub fn contains_point(monitor: &MonitorRect, point: [i32; 2]) -> bool {
    point[0] >= monitor.x
        && point[0] < monitor.x + monitor.width as i32
        && point[1] >= monitor.y
        && point[1] < monitor.y + monitor.height as i32
}

/// Keep a window of `size` fully inside `monitor`. The drag path clamps through
/// `compute_anchored_drag_position` on every frame, so a move event that arrives
/// during a drag needs no second clamp.
pub fn clamp_to_monitor(position: [i32; 2], size: (u32, u32), monitor: &MonitorRect) -> [i32; 2] {
    let max_x = monitor.x + monitor.width as i32 - size.0 as i32;
    let max_y = monitor.y + monitor.height as i32 - size.1 as i32;
    [
        position[0].clamp(monitor.x.min(max_x), max_x.max(monitor.x)),
        position[1].clamp(monitor.y.min(max_y), max_y.max(monitor.y)),
    ]
}

/// The physical cursor and window bounds captured at the beginning of a drag.
/// Keeping this independent from Tauri makes the drag invariant testable without
/// a running desktop session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragSnapshot {
    pub cursor: [i32; 2],
    pub bounds: [i32; 2],
    pub size: (u32, u32),
}

pub fn create_drag_snapshot(cursor: [i32; 2], bounds: [i32; 2], size: (u32, u32)) -> DragSnapshot {
    DragSnapshot {
        cursor,
        bounds,
        size,
    }
}

/// Move a window by the cursor delta from its drag snapshot, then constrain it
/// to the monitor on every step. This is the manual-drag replacement for
/// platform-native window drag.
pub fn compute_anchored_drag_position(
    snapshot: DragSnapshot,
    cursor: [i32; 2],
    monitor: &MonitorRect,
) -> [i32; 2] {
    let proposed = [
        snapshot.bounds[0] + cursor[0] - snapshot.cursor[0],
        snapshot.bounds[1] + cursor[1] - snapshot.cursor[1],
    ];
    clamp_to_monitor(proposed, snapshot.size, monitor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfacePlacement {
    pub position: [i32; 2],
    pub horizontal: &'static str,
    pub vertical: &'static str,
}

/// Place a surface beside the pet, flipping at the work-area edges and then
/// clamping the final rectangle so an unusually small work area is still safe.
pub fn compute_anchored_surface_placement(
    pet_position: [i32; 2],
    pet_size: (u32, u32),
    surface_size: (u32, u32),
    work_area: &MonitorRect,
    gap: i32,
) -> SurfacePlacement {
    let work_right = work_area.x + work_area.width as i32;
    let work_bottom = work_area.y + work_area.height as i32;
    let pet_right = pet_position[0] + pet_size.0 as i32;
    let pet_bottom = pet_position[1] + pet_size.1 as i32;
    let surface_width = surface_size.0 as i32;
    let surface_height = surface_size.1 as i32;
    let horizontal = if pet_right + gap + surface_width <= work_right {
        "right"
    } else {
        "left"
    };
    let vertical = if pet_position[1] + surface_height <= work_bottom {
        "top"
    } else {
        "bottom"
    };
    let raw_x = if horizontal == "right" {
        pet_right + gap
    } else {
        pet_position[0] - gap - surface_width
    };
    let raw_y = if vertical == "top" {
        pet_position[1]
    } else {
        pet_bottom - surface_height
    };
    let max_x = (work_right - surface_width).max(work_area.x);
    let max_y = (work_bottom - surface_height).max(work_area.y);
    SurfacePlacement {
        position: [
            raw_x.clamp(work_area.x, max_x),
            raw_y.clamp(work_area.y, max_y),
        ],
        horizontal,
        vertical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primary() -> MonitorRect {
        MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn rejects_the_recorded_offscreen_position() {
        // Real coordinates found in ~/.config/herdr-pet/window.json after the pet
        // vanished: show() reported success while the window sat far off-screen.
        assert!(!position_on_any_monitor([542_720, 163_840], &[primary()]));
    }

    #[test]
    fn accepts_a_position_on_a_secondary_monitor() {
        let secondary = MonitorRect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(position_on_any_monitor(
            [-1800, 120],
            &[primary(), secondary]
        ));
        // ...and rejects it once that display is unplugged.
        assert!(!position_on_any_monitor([-1800, 120], &[primary()]));
    }

    #[test]
    fn trusts_the_saved_position_when_no_monitor_is_known() {
        assert!(position_on_any_monitor([542_720, 163_840], &[]));
    }

    #[test]
    fn clamps_a_window_dragged_past_the_edges() {
        let size = (124, 124);
        assert_eq!(clamp_to_monitor([1900, 500], size, &primary()), [1796, 500]);
        assert_eq!(clamp_to_monitor([-40, -40], size, &primary()), [0, 0]);
        assert_eq!(
            clamp_to_monitor([542_720, 163_840], size, &primary()),
            [1796, 956]
        );
    }

    #[test]
    fn leaves_an_in_bounds_window_untouched() {
        assert_eq!(
            clamp_to_monitor([120, 120], (124, 124), &primary()),
            [120, 120]
        );
    }

    #[test]
    fn clamps_to_the_origin_when_the_window_exceeds_the_monitor() {
        let tiny = MonitorRect {
            x: 100,
            y: 100,
            width: 80,
            height: 80,
        };
        assert_eq!(
            clamp_to_monitor([9999, 9999], (124, 124), &tiny),
            [100, 100]
        );
    }

    #[test]
    fn anchored_drag_preserves_the_cursor_delta() {
        let snapshot = create_drag_snapshot([100, 100], [400, 300], (124, 124));
        assert_eq!(
            compute_anchored_drag_position(snapshot, [135, 82], &primary()),
            [435, 282]
        );
    }

    #[test]
    fn anchored_drag_clamps_during_the_gesture() {
        let snapshot = create_drag_snapshot([100, 100], [400, 300], (124, 124));
        assert_eq!(
            compute_anchored_drag_position(snapshot, [2_000, 2_000], &primary()),
            [1796, 956]
        );
    }

    #[test]
    fn anchored_drag_supports_negative_monitor_origins() {
        let monitor = MonitorRect {
            x: -1920,
            y: -100,
            width: 1920,
            height: 1080,
        };
        let snapshot = create_drag_snapshot([100, 100], [-100, 0], (124, 124));
        assert_eq!(
            compute_anchored_drag_position(snapshot, [-1_900, -90], &monitor),
            [-1920, -100]
        );
    }

    #[test]
    fn surface_placement_flips_and_stays_in_the_work_area() {
        let work = primary();
        let placement =
            compute_anchored_surface_placement([1800, 900], (124, 124), (300, 520), &work, 6);
        assert_eq!(placement.horizontal, "left");
        assert_eq!(placement.vertical, "bottom");
        assert_eq!(placement.position, [1494, 504]);
    }

    /// Real `ps -axo pid=,command=` output captured while a local and a remote
    /// herdr session were open as two tabs of one Ghostty window. Clicking a
    /// remote agent raised the wrong session because the old code looked for a
    /// fixed `herdr client` process instead of routing by target.
    fn recorded_processes() -> Vec<SessionProcess> {
        parse_process_list(
            "13262 /Applications/Herdr Pet.app/Contents/MacOS/herdr-pet-app\n\
             22954 herdr\n\
             69590 herdr --remote grab@grabs-mac-mini\n\
             69779 /Users/hoyeonlee/.local/bin/herdr client\n\
             97575 ssh -N -T -L /tmp/herdr-pet/mini.sock:/Users/grab/.config/herdr/herdr.sock mini\n",
        )
    }

    #[test]
    fn parses_pid_and_command_pairs() {
        let processes = recorded_processes();
        assert_eq!(processes.len(), 5);
        assert_eq!(processes[1].pid, "22954");
        assert_eq!(processes[2].command, "herdr --remote grab@grabs-mac-mini");
    }

    #[test]
    fn skips_lines_without_a_numeric_pid() {
        assert!(parse_process_list("  PID COMMAND\n\nnotapid herdr\n").is_empty());
    }

    #[test]
    fn routes_a_remote_target_to_its_remote_view() {
        // The pet config names the ssh alias `mini`; the process was launched
        // with the resolved destination `grab@grabs-mac-mini`.
        let picked = select_session_process(&recorded_processes(), Some("mini")).unwrap();
        assert_eq!(picked.pid, "69590");
        assert_eq!(picked.command, "herdr --remote grab@grabs-mac-mini");
    }

    #[test]
    fn routes_a_local_target_to_the_plain_session() {
        let picked = select_session_process(&recorded_processes(), None).unwrap();
        assert_eq!(picked.pid, "22954");
    }

    #[test]
    fn never_picks_the_client_helper_process_for_a_local_target() {
        // pid 69779 is `herdr client`, a child of the *remote* view. The old
        // `pgrep -f "herdr client"` matched exactly this process and so raised
        // the remote session no matter which agent was clicked.
        let picked = select_session_process(&recorded_processes(), None).unwrap();
        assert_ne!(picked.pid, "69779");
    }

    #[test]
    fn reports_no_match_when_the_remote_view_is_not_open() {
        assert!(select_session_process(&recorded_processes(), Some("other-host")).is_none());
    }

    #[test]
    fn matches_a_remote_argument_against_an_ssh_alias() {
        assert!(remote_argument_matches_host("grab@grabs-mac-mini", "mini"));
        assert!(remote_argument_matches_host("mini", "grab@grabs-mac-mini"));
        assert!(remote_argument_matches_host(
            "grab@grabs-mac-mini",
            "grabs-mac-mini"
        ));
        assert!(!remote_argument_matches_host(
            "grab@grabs-mac-mini",
            "laptop"
        ));
        assert!(!remote_argument_matches_host("", "mini"));
    }

    #[test]
    fn accepts_the_equals_form_of_the_remote_flag() {
        let processes = parse_process_list("501 herdr --remote=grab@grabs-mac-mini\n");
        assert!(select_session_process(&processes, Some("mini")).is_some());
    }

    #[test]
    fn treats_a_session_flag_as_a_local_session() {
        let processes = parse_process_list("501 herdr --session work\n");
        assert_eq!(select_session_process(&processes, None).unwrap().pid, "501");
    }

    #[test]
    fn ignores_herdr_subcommands_that_nobody_is_looking_at() {
        let processes = parse_process_list("501 herdr api snapshot\n502 herdr pane list\n");
        assert!(select_session_process(&processes, None).is_none());
    }

    #[test]
    fn detects_whether_a_point_is_on_a_monitor() {
        assert!(contains_point(&primary(), [0, 0]));
        assert!(contains_point(&primary(), [1919, 1079]));
        // Exclusive on the far edges, so neighbouring displays cannot both claim
        // the same point and thrash the cached monitor back and forth.
        assert!(!contains_point(&primary(), [1920, 500]));
        assert!(!contains_point(&primary(), [500, 1080]));
        assert!(!contains_point(&primary(), [-1, 500]));
    }

    #[test]
    fn a_secondary_monitor_claims_points_the_primary_rejects() {
        let secondary = MonitorRect { x: -1920, y: 0, width: 1920, height: 1080 };
        assert!(contains_point(&secondary, [-1, 500]));
        assert!(!contains_point(&primary(), [-1, 500]));
    }

    #[test]
    fn escapes_quotes_and_backslashes_for_applescript() {
        assert_eq!(escape_applescript(r#"a "b" c\d"#), r#"a \"b\" c\\d"#);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tab_selection_script_fails_when_the_session_tab_is_missing() {
        let script = raise_and_select_tab_script("123", "herdr --remote mini");
        assert!(script.contains("error \"The terminal app did not become active\""));
        assert!(script.contains("error \"The Herdr session tab was not found\""));
        assert!(!script.contains("try\nrepeat with w in windows"));
    }
}
