use serde::Deserialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserAction {
    Launch,
    Reuse,
    Parked(String),
}

#[derive(Debug, Deserialize)]
struct ProcessList {
    #[serde(default)]
    profiles: Vec<Profile>,
}

#[derive(Debug, Deserialize)]
struct Profile {
    #[serde(alias = "name")]
    profile: String,
    #[serde(alias = "state")]
    status: String,
}

pub fn plan_open(
    profile: &str,
    process_json: Option<&str>,
    known_profiles: Option<&[String]>,
) -> BrowserAction {
    if profile.trim().is_empty() {
        return BrowserAction::Parked("A non-empty existing profile is required".to_owned());
    }
    let Some(process_json) = process_json else {
        return BrowserAction::Parked(
            "Runtime execution is parked; chromux was not inspected or launched".to_owned(),
        );
    };
    let Ok(processes) = serde_json::from_str::<ProcessList>(process_json) else {
        return BrowserAction::Parked("chromux process output could not be parsed".to_owned());
    };
    match processes
        .profiles
        .iter()
        .find(|candidate| candidate.profile == profile)
    {
        Some(candidate) if candidate.status == "running" => BrowserAction::Reuse,
        Some(_) => BrowserAction::Launch,
        None if known_profiles
            .is_some_and(|profiles| profiles.iter().any(|candidate| candidate == profile)) =>
        {
            BrowserAction::Launch
        }
        None => BrowserAction::Parked(
            "The profile is not in the known profile list; it will not be created".to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_planner_reuses_running_and_launches_known_stopped_profiles() {
        let json = r#"{"ok":true,"profiles":[{"profile":"known","status":"running"},{"profile":"stopped","status":"locked"}]}"#;
        assert_eq!(plan_open("known", Some(json), None), BrowserAction::Reuse);
        assert_eq!(
            plan_open("stopped", Some(json), None),
            BrowserAction::Launch
        );
    }

    #[test]
    fn absent_runtime_input_and_unknown_profiles_remain_parked() {
        assert!(matches!(
            plan_open("known", None, None),
            BrowserAction::Parked(_)
        ));
        assert!(matches!(
            plan_open("missing", Some(r#"{"profiles":[]}"#), Some(&[])),
            BrowserAction::Parked(_)
        ));
        assert_eq!(
            plan_open(
                "stopped",
                Some(r#"{"profiles":[]}"#),
                Some(&["stopped".to_owned()])
            ),
            BrowserAction::Launch
        );
    }
}
