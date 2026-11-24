use std::collections::{HashMap, HashSet, VecDeque};

use super::definitions::{Edge, Key, Record};

pub(super) struct Ancestor<'a> {
    pub(super) class: &'a Key,
    pub(super) predecessor: Option<&'a Key>,
    pub(super) distance: usize,
    pub(super) parent_rank: usize,
}

pub(super) fn walk<'a>(
    records: &'a HashMap<Key, Record>,
    source: &'a Key,
) -> impl Iterator<Item = Ancestor<'a>> {
    let mut seen = HashSet::from([source]);
    let mut pending = VecDeque::new();
    if let Some(record) = records.get(source) {
        pending.extend(
            record
                .edges
                .iter()
                .enumerate()
                .map(|(parent_rank, edge)| Ancestor {
                    class: &edge.target,
                    predecessor: None,
                    distance: 1,
                    parent_rank,
                }),
        );
    }
    std::iter::from_fn(move || {
        while let Some(step) = pending.pop_front() {
            if !seen.insert(step.class) {
                continue;
            }
            if let Some(record) = records.get(step.class) {
                pending.extend(record.edges.iter().map(|edge| Ancestor {
                    class: &edge.target,
                    predecessor: Some(step.class),
                    distance: step.distance + 1,
                    parent_rank: step.parent_rank,
                }));
            }
            return Some(step);
        }
        None
    })
}

pub(super) fn path(
    records: &HashMap<Key, Record>,
    source: &Key,
    target: &str,
) -> Option<Vec<Edge>> {
    if source.0 == target {
        return Some(Vec::new());
    }
    let mut predecessors = HashMap::new();
    let target = walk(records, source).find(|step| {
        predecessors.insert(step.class, step.predecessor.unwrap_or(source));
        step.class.0 == target
    })?;
    let mut path = Vec::with_capacity(target.distance);
    let mut class = target.class;
    while class != source {
        let &parent = predecessors.get(class)?;
        let edge = records
            .get(parent)?
            .edges
            .iter()
            .find(|edge| &edge.target == class)?;
        path.push(edge.clone());
        class = parent;
    }
    path.reverse();
    Some(path)
}
