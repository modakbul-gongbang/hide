//! Bounded, read-only material for the session's issue preview.

use crate::error::{ErrorCode, HostError, HostResult};
use crate::root::Root;
use std::io::Read;
use std::path::Path;

const TEMPLATE_FILE_LIMIT: usize = 8;
const TEMPLATE_BYTES: usize = 16 * 1024;
const TEMPLATE_FILE_BYTES: usize = 8 * 1024;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Context {
    pub turn: hide_session::label_transcript::WorkTurn,
    pub templates: String,
}

pub fn read(
    home: &Path,
    request: &hide_session::label_transcript::WorkTurnRequest,
    checkout: &str,
) -> HostResult<Context> {
    let turn = hide_session::label_transcript::read_work_turn(home, request)
        .map_err(|reason| HostError::new(ErrorCode::Io, reason))?;
    let root = Root::open(Path::new(checkout))?;
    let mut paths = vec![
        ".github/ISSUE_TEMPLATE.md".to_owned(),
        "ISSUE_TEMPLATE.md".to_owned(),
    ];
    match root.dir().read_dir(".github/ISSUE_TEMPLATE") {
        Ok(entries) => {
            let mut names = Vec::new();
            for entry in entries {
                let entry =
                    entry.map_err(|error| HostError::io(&error, "issue_template_read_failed"))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if ![".md", ".yml", ".yaml"]
                    .iter()
                    .any(|extension| name.ends_with(extension))
                    || name == "config.yml"
                    || name == "config.yaml"
                {
                    continue;
                }
                if names.len() >= TEMPLATE_FILE_LIMIT {
                    return Err(HostError::new(
                        ErrorCode::TooLarge,
                        "issue_template_file_capacity",
                    ));
                }
                names.push(format!(".github/ISSUE_TEMPLATE/{name}"));
            }
            names.sort();
            paths.extend(names);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(HostError::io(&error, "issue_template_read_failed")),
    }
    let mut templates = String::new();
    let mut files = 0;
    for path in paths {
        let file = match root.dir().open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(HostError::io(&error, "issue_template_read_failed")),
        };
        if !file
            .metadata()
            .map_err(|error| HostError::io(&error, "issue_template_read_failed"))?
            .is_file()
        {
            return Err(HostError::new(
                ErrorCode::InvalidPath,
                "issue_template_not_file",
            ));
        }
        files += 1;
        if files > TEMPLATE_FILE_LIMIT {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                "issue_template_file_capacity",
            ));
        }
        let mut bytes = Vec::new();
        file.take((TEMPLATE_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| HostError::io(&error, "issue_template_read_failed"))?;
        if bytes.len() > TEMPLATE_FILE_BYTES
            || templates.len() + bytes.len() + path.len() + 8 > TEMPLATE_BYTES
        {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                "issue_template_byte_capacity",
            ));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| HostError::new(ErrorCode::InvalidPath, "issue_template_not_utf8"))?;
        templates.push_str(&format!(
            "\n<template name={path:?}>\n{text}\n</template>\n"
        ));
        if templates.len() > TEMPLATE_BYTES {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                "issue_template_byte_capacity",
            ));
        }
    }
    Ok(Context { turn, templates })
}
