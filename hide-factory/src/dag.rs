//! The dependency graph (D-29): cycles, start eligibility, slot order and the
//! reduced edges the graph view draws (D-36).

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Task, TaskState};

/// Edges as `task -> predecessors` within one Factory.
pub type Edges = BTreeMap<String, BTreeSet<String>>;

pub fn edges<'a>(tasks: impl Iterator<Item = &'a Task>) -> Edges {
    let mut edges = Edges::new();
    for task in tasks {
        edges
            .entry(task.id.clone())
            .or_default()
            .extend(task.card.depends_on.iter().cloned());
    }
    edges
}

/// A cycle the edge `from -> on` would close, as the path from `on` back to
/// `from`, or `None`.
pub fn cycle_with(edges: &Edges, from: &str, on: &str) -> Option<Vec<String>> {
    if from == on {
        return Some(vec![from.to_owned()]);
    }
    // Is `from` reachable from `on` following predecessor edges?
    let mut stack = vec![(on.to_owned(), vec![on.to_owned()])];
    let mut seen = BTreeSet::new();
    while let Some((node, path)) = stack.pop() {
        if node == from {
            return Some(path);
        }
        if !seen.insert(node.clone()) {
            continue;
        }
        for next in edges.get(&node).into_iter().flatten() {
            let mut path = path.clone();
            path.push(next.clone());
            stack.push((next.clone(), path));
        }
    }
    None
}

/// Whether the whole graph is acyclic; returns one cycle when not.
pub fn find_cycle(edges: &Edges) -> Option<Vec<String>> {
    for (task, predecessors) in edges {
        for predecessor in predecessors {
            let mut without = edges.clone();
            without
                .get_mut(task)
                .map(|set| set.remove(predecessor))
                .unwrap_or(false);
            if let Some(path) = cycle_with(&without, task, predecessor) {
                return Some(path);
            }
        }
    }
    None
}

/// The predecessors a Task still waits for: those not merged yet (B20).
/// Predecessors outside the Factory are handled before this (missing ids are
/// refused at intake); external waits never count (B64).
pub fn waiting_for(task: &Task, tasks: &BTreeMap<String, Task>) -> Vec<String> {
    task.card
        .depends_on
        .iter()
        .filter(|id| {
            tasks
                .get(*id)
                .is_none_or(|predecessor| !predecessor.state.merged())
        })
        .cloned()
        .collect()
}

/// Waiting Tasks that may start now, in slot order: priority high first,
/// then oldest first (D-29, B21).
pub fn startable(tasks: &BTreeMap<String, Task>) -> Vec<&Task> {
    let mut ready: Vec<&Task> = tasks
        .values()
        .filter(|task| task.state == TaskState::Waiting && waiting_for(task, tasks).is_empty())
        .collect();
    ready.sort_by(|a, b| slot_order(a, b));
    ready
}

pub fn slot_order(a: &Task, b: &Task) -> std::cmp::Ordering {
    b.human
        .priority
        .cmp(&a.human.priority)
        .then(a.created_at.cmp(&b.created_at))
        .then(a.seq.cmp(&b.seq))
}

/// Edges with every edge implied by a longer path removed; the data keeps
/// all of them (D-36).
pub fn transitive_reduction(edges: &Edges) -> Vec<(String, String)> {
    let mut reduced = Vec::new();
    for (task, predecessors) in edges {
        for predecessor in predecessors {
            let implied = predecessors
                .iter()
                .any(|other| other != predecessor && reaches(edges, other, predecessor));
            if !implied {
                reduced.push((predecessor.clone(), task.clone()));
            }
        }
    }
    reduced
}

fn reaches(edges: &Edges, from: &str, to: &str) -> bool {
    let mut stack = vec![from.to_owned()];
    let mut seen = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if node == to {
            return true;
        }
        if !seen.insert(node.clone()) {
            continue;
        }
        stack.extend(edges.get(&node).into_iter().flatten().cloned());
    }
    false
}

/// Tasks downstream of `task` (those that wait on it, directly or not).
pub fn dependents(edges: &Edges, task: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut frontier = vec![task.to_owned()];
    while let Some(node) = frontier.pop() {
        for (candidate, predecessors) in edges {
            if predecessors.contains(&node) && found.insert(candidate.clone()) {
                frontier.push(candidate.clone());
            }
        }
    }
    found
}

/// A topological order of the given Tasks, predecessors first; `None` when
/// the graph has a cycle.
pub fn topological(edges: &Edges) -> Option<Vec<String>> {
    let mut order = Vec::new();
    let mut placed = BTreeSet::new();
    let nodes: BTreeSet<String> = edges
        .keys()
        .cloned()
        .chain(edges.values().flatten().cloned())
        .collect();
    while placed.len() < nodes.len() {
        let next = nodes.iter().find(|node| {
            !placed.contains(*node)
                && edges
                    .get(*node)
                    .into_iter()
                    .flatten()
                    .all(|predecessor| placed.contains(predecessor))
        })?;
        placed.insert(next.clone());
        order.push(next.clone());
    }
    Some(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(pairs: &[(&str, &str)]) -> Edges {
        let mut edges = Edges::new();
        for (task, on) in pairs {
            edges
                .entry((*task).to_owned())
                .or_default()
                .insert((*on).to_owned());
            edges.entry((*on).to_owned()).or_default();
        }
        edges
    }

    #[test]
    fn an_edge_that_closes_a_loop_is_found_with_its_path() {
        let edges = graph(&[("b", "a"), ("c", "b")]);
        assert_eq!(
            cycle_with(&edges, "a", "c"),
            Some(vec!["c".into(), "b".into(), "a".into()])
        );
        assert_eq!(cycle_with(&edges, "c", "a"), None);
        assert_eq!(cycle_with(&edges, "a", "a"), Some(vec!["a".into()]));
        assert!(find_cycle(&edges).is_none());
        let looped = graph(&[("b", "a"), ("a", "b")]);
        assert!(find_cycle(&looped).is_some());
    }

    #[test]
    fn the_reduction_drops_only_edges_a_longer_path_implies() {
        // a -> b -> c and a -> c: the direct a -> c is implied.
        let edges = graph(&[("b", "a"), ("c", "b"), ("c", "a"), ("d", "a")]);
        let mut reduced = transitive_reduction(&edges);
        reduced.sort();
        assert_eq!(
            reduced,
            vec![
                ("a".to_owned(), "b".to_owned()),
                ("a".to_owned(), "d".to_owned()),
                ("b".to_owned(), "c".to_owned())
            ]
        );
    }

    #[test]
    fn dependents_and_order_follow_the_chain_only() {
        let edges = graph(&[("b", "a"), ("c", "b"), ("e", "d")]);
        assert_eq!(
            dependents(&edges, "a"),
            ["b", "c"].into_iter().map(str::to_owned).collect()
        );
        let order = topological(&edges).unwrap();
        let position = |id: &str| order.iter().position(|node| node == id).unwrap();
        assert!(position("a") < position("b") && position("b") < position("c"));
        assert!(position("d") < position("e"));
    }
}
