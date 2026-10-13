//! Bounded, read-only material for the session's issue preview.

use crate::error::{ErrorCode, HostError, HostResult};
use crate::root::Root;
use hide_node_link::sessions::WorkContext;
use std::io::Read;
use std::path::Path;

const TEMPLATE_FILE_LIMIT: usize = 8;
const TEMPLATE_BYTES: usize = 16 * 1024;
const TEMPLATE_FILE_BYTES: usize = 8 * 1024;

pub fn read(
    home: &Path,
    request: &hide_session::label_transcript::WorkTurnRequest,
    checkout: &str,
) -> HostResult<WorkContext> {
    let turn = hide_session::label_transcript::read_work_turn(home, request)
        .map_err(|reason| HostError::new(ErrorCode::Io, reason))?;
    let templates = read_templates(Path::new(checkout))?;
    Ok(WorkContext { turn, templates })
}

fn read_templates(checkout: &Path) -> HostResult<String> {
    let root = Root::open(checkout)?;
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
        let file = match crate::bytes::open_regular(root.dir(), Path::new(&path)) {
            Ok(file) => file,
            Err(error) if error.code == ErrorCode::NotFound => continue,
            Err(_) => {
                return Err(HostError::new(
                    ErrorCode::InvalidPath,
                    "issue_template_read_failed",
                ));
            }
        };
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
    Ok(templates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn templates_read_sorted_regular_files_and_reject_directories_and_oversized_content() {
        let directory = tempfile::tempdir().unwrap();
        let templates = directory.path().join(".github/ISSUE_TEMPLATE");
        fs::create_dir_all(&templates).unwrap();
        fs::write(templates.join("b.md"), "Second template").unwrap();
        fs::write(templates.join("a.yml"), "First template").unwrap();
        fs::write(
            templates.join("config.yml"),
            "Contact links are not a template",
        )
        .unwrap();
        let text = read_templates(directory.path()).unwrap();
        assert!(text.find("First template").unwrap() < text.find("Second template").unwrap());
        assert!(!text.contains("Contact links"));
        fs::create_dir(templates.join("c.md")).unwrap();
        assert_eq!(
            read_templates(directory.path()).unwrap_err().message,
            "issue_template_read_failed"
        );
        fs::remove_dir(templates.join("c.md")).unwrap();
        fs::write(templates.join("a.yml"), vec![b'x'; TEMPLATE_FILE_BYTES + 1]).unwrap();
        assert_eq!(
            read_templates(directory.path()).unwrap_err().message,
            "issue_template_byte_capacity"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_or_symlink_outside_the_checkout_is_refused_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().unwrap();
        let template = directory.path().join("ISSUE_TEMPLATE.md");
        let status = std::process::Command::new("mkfifo")
            .arg(&template)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            read_templates(directory.path()).unwrap_err().message,
            "issue_template_read_failed"
        );
        fs::remove_file(&template).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), &template).unwrap();
        assert_eq!(
            read_templates(directory.path()).unwrap_err().message,
            "issue_template_read_failed"
        );
    }
}
