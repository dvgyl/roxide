use super::{Point, delta, finite};

#[derive(Clone, Copy)]
pub(super) struct Box2 {
    pub(super) low: Point,
    pub(super) high: Point,
}

impl Box2 {
    pub(super) fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        if ![x, y, w, h].into_iter().all(f64::is_finite) {
            return Self {
                low: (f64::NAN, f64::NAN),
                high: (f64::NAN, f64::NAN),
            };
        }
        Self {
            low: (x.min(x + w), y.min(y + h)),
            high: (x.max(x + w), y.max(y + h)),
        }
    }

    pub(super) fn corners(self) -> [Point; 4] {
        let (x0, y0) = self.low;
        let (x1, y1) = self.high;
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }

    pub(super) fn width(self) -> f64 {
        self.high.0 - self.low.0
    }

    pub(super) fn height(self) -> f64 {
        self.high.1 - self.low.1
    }

    pub(super) fn overlaps(self, other: Self) -> bool {
        self.low.0 < other.high.0
            && self.high.0 > other.low.0
            && self.low.1 < other.high.1
            && self.high.1 > other.low.1
    }
}

#[derive(Default)]
pub(super) struct Bounds(pub(super) Option<Box2>);

impl Bounds {
    pub(super) fn add(&mut self, points: impl IntoIterator<Item = Point>) {
        for (x, y) in points.into_iter().filter(|&pt| finite(pt)) {
            let b = self.0.get_or_insert(Box2::new(x, y, 0.0, 0.0));
            b.low = (b.low.0.min(x), b.low.1.min(y));
            b.high = (b.high.0.max(x), b.high.1.max(y));
        }
    }
}

fn cross(a: Point, b: Point) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn hull_half(points: impl Iterator<Item = Point>) -> Vec<Point> {
    let mut hull = Vec::new();
    for point in points {
        while let [.., a, b] = hull.as_slice() {
            if cross(delta(*b, *a), delta(point, *b)) > 0.0 {
                break;
            }
            hull.pop();
        }
        hull.push(point);
    }
    hull.pop();
    hull
}

pub(super) fn boundary(bounds: Box2, mut points: Vec<Point>, angle: f64) -> Point {
    let center = (
        (bounds.high.0 + bounds.low.0) / 2.0,
        (bounds.high.1 + bounds.low.1) / 2.0,
    );
    let (sin, cos) = angle.to_radians().sin_cos();
    let ray = (cos, sin);
    points.retain(|&pt| finite(pt));
    points.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    points.dedup();
    let mut hull = hull_half(points.iter().copied());
    hull.extend(hull_half(points.iter().rev().copied()));
    let mut distance = f64::INFINITY;
    for (&a, &b) in hull.iter().zip(hull.iter().cycle().skip(1)) {
        let start = delta(a, center);
        let edge = delta(b, a);
        let denominator = cross(ray, edge);
        if denominator.abs() <= 1e-12 {
            continue;
        }
        let t = cross(start, edge) / denominator;
        let u = cross(start, ray) / denominator;
        if t >= 0.0 && (-1e-12..=1.0 + 1e-12).contains(&u) {
            distance = distance.min(t);
        }
    }
    if !distance.is_finite() && hull.len() == 2 && cross(ray, delta(hull[0], center)).abs() <= 1e-12
    {
        distance = (hull[0].0 - center.0).hypot(hull[0].1 - center.1);
    }
    if distance.is_finite() {
        (center.0 + distance * cos, center.1 + distance * sin)
    } else {
        center
    }
}
