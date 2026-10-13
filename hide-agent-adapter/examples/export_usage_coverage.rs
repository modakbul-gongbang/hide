fn main() {
    if std::env::args().any(|arg| arg == "--markdown") {
        print!("{}", hide_agent_adapter::usage::coverage_markdown());
        return;
    }
    let rows: Vec<_> = hide_agent_adapter::ADAPTERS
        .iter()
        .map(|row| {
            serde_json::json!({
                "agent": row.id,
                "cells": row.usage.cells().into_iter().collect::<std::collections::BTreeMap<_, _>>()
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&rows).expect("static declarations serialize")
    );
}
