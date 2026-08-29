#![cfg_attr(not(target_os = "macos"), allow(unused))]

#[cfg(not(target_os = "macos"))]
compile_error!("Herdr IDE is a macOS-only native application");

pub mod commands;
pub mod context_menu;
pub mod diagnostics;
pub mod domain;
pub mod files;
pub mod herdr;
pub mod herdr_contract;
pub mod layout;
pub mod navigator;
pub mod pet;
pub mod presentation;
pub mod remote;
