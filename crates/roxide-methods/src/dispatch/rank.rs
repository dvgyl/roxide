use std::collections::HashMap;

type Rank = (usize, usize);
pub(super) type Distances = HashMap<String, Rank>;

pub(super) fn distances(class: &str, ancestors: Vec<(String, usize, usize)>) -> Distances {
    let mut distances = Distances::new();
    for (name, distance, rank) in ancestors.into_iter().filter(|(name, _, _)| name != "ANY") {
        distances
            .entry(name)
            .and_modify(|prior| *prior = (*prior).min((distance, rank)))
            .or_insert((distance, rank));
    }
    distances.insert(class.to_owned(), (0, 0));
    distances
}

pub(super) fn add_any_distance(distances: &mut [Distances]) {
    let distance = distances
        .iter()
        .flat_map(|classes| classes.values())
        .map(|rank| rank.0)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    for classes in distances {
        classes.entry("ANY".into()).or_insert((distance, 0));
    }
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
        self.ranks
            .iter()
            .zip(&other.ranks)
            .any(|(chosen, alt)| alt < chosen)
    }
}
