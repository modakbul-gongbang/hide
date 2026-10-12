fn main() {
    let rows: Vec<_> = hide_agent_adapter::ADAPTERS.iter().map(|row| {
        serde_json::json!({
            "agent": row.id,
            "cells": row.usage_declaration.cells().into_iter().collect::<std::collections::BTreeMap<_, _>>()
        })
    }).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&rows).expect("static declarations serialize")
    );
}
