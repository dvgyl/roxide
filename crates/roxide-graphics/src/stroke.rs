use crate::engine::{LineEnd, LineJoin};

pub(crate) const LINE_TYPES: [&str; 7] = [
    "blank", "solid", "dashed", "dotted", "dotdash", "longdash", "twodash",
];

pub(crate) fn line_end(x: &str) -> LineEnd {
    match x {
        "butt" => LineEnd::Butt,
        "square" => LineEnd::Square,
        _ => LineEnd::Round,
    }
}

pub(crate) fn line_join(x: &str) -> LineJoin {
    match x {
        "mitre" => LineJoin::Mitre,
        "bevel" => LineJoin::Bevel,
        _ => LineJoin::Round,
    }
}

pub(crate) fn dash_pattern(lty: &str) -> Vec<f64> {
    let pattern = match lty {
        "solid" | "blank" => "",
        "dashed" => "44",
        "dotted" => "13",
        "dotdash" => "1343",
        "longdash" => "73",
        "twodash" => "2262",
        other => other,
    };
    pattern
        .chars()
        .filter_map(|c| c.to_digit(16))
        .map(f64::from)
        .collect()
}
