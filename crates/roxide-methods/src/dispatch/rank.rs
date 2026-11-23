use std::collections::HashMap;

type Rank = (usize, usize);
pub(super) type Distances = HashMap<String, Rank>;

pub(super) fn distances(class: &str, ancestors: Vec<(String, usize, usize)>) -> Distances {
    let mut distances = Distances::new();
    let mut depth = 0;
    for (name, distance, rank) in ancestors {
        depth = depth.max(distance);
        distances
            .entry(name)
            .and_modify(|prior| *prior = (*prior).min((distance, rank)))
            .or_insert((distance, rank));
    }
    distances.insert("ANY".to_owned(), (depth.saturating_add(1), 0));
    distances.insert(class.to_owned(), (0, 0));
    distances
}

pub(super) struct Ranked {
    ranks: Vec<Rank>,
    total: usize,
    exact: bool,
}

impl Ranked {
    pub(super) fn score(signature: &[String], distances: &[Distances]) -> Option<Self> {
        let ranks = signature
            .iter()
            .zip(distances)
            .map(|(class, distances)| distances.get(class).copied())
            .collect::<Option<Vec<_>>>()?;
        let total = ranks
            .iter()
            .fold(0_usize, |sum, &(distance, _)| sum.saturating_add(distance));
        let exact = ranks.iter().any(|&(distance, _)| distance == 0);
        Some(Self {
            ranks,
            total,
            exact,
        })
    }

    pub(super) fn total(&self) -> usize {
        self.total
    }

    pub(super) fn order(&self, other: &Self) -> std::cmp::Ordering {
        self.total
            .cmp(&other.total)
            .then_with(|| other.exact.cmp(&self.exact))
            .then_with(|| {
                self.ranks
                    .iter()
                    .map(|p| p.0)
                    .cmp(other.ranks.iter().map(|p| p.0))
            })
            .then_with(|| {
                self.ranks
                    .iter()
                    .map(|p| p.1)
                    .cmp(other.ranks.iter().map(|p| p.1))
            })
    }

    pub(super) fn ambiguous_with(&self, other: &Self) -> bool {
        // A better parent rank can settle a tie without a selection note.
        self.total == other.total
            && (self.ranks == other.ranks
                || self
                    .ranks
                    .iter()
                    .zip(&other.ranks)
                    .any(|(chosen, alt)| chosen > alt))
    }
}
