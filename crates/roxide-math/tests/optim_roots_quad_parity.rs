//! Check numerical solvers and callback error propagation.

#![forbid(unsafe_code)]

use std::convert::Infallible;

use roxide_math::{Objective, VecFn, fmin::fmin, optim, quadpack, roots::zeroin};

#[test]
fn scalar_solvers() {
    let (min, value) = fmin(|x| Ok::<_, Infallible>((x - 1.0).powi(2)), -4.0, 4.0, 1e-8).unwrap();
    assert!((min - 1.0).abs() < 1e-8);
    assert!(value < 1e-16);
    let out = zeroin(
        |x| Ok::<_, Infallible>(x * x - 2.0),
        0.0,
        2.0,
        -2.0,
        2.0,
        1e-12,
        100,
    )
    .unwrap();
    assert!(out.converged);
    assert!((out.root - 2.0_f64.sqrt()).abs() < 1e-12);
    assert_eq!(
        zeroin(|_| Err("callback"), 0.0, 1.0, -1.0, 1.0, 1e-8, 20).unwrap_err(),
        "callback"
    );
}

struct Singular;

impl VecFn for Singular {
    type Err = Infallible;

    fn eval(&mut self, x: &[f64], out: &mut [f64]) -> Result<(), Self::Err> {
        assert_eq!(x.len(), 21);
        for (y, &x) in out.iter_mut().zip(x) {
            *y = x.sqrt().recip();
        }
        Ok(())
    }
}

#[test]
fn endpoint_extrapolation() {
    let out = quadpack::dqags(&mut Singular, 0.0, 1.0, 1e-8, 1e-8, 100).unwrap();
    assert_eq!(out.status, quadpack::Status::Ok);
    assert_eq!(out.subdivisions, 6);
    assert!((out.value - 2.0).abs() < 1e-13);
}

struct Quadratic;

impl Objective for Quadratic {
    type Err = Infallible;

    fn value(&mut self, x: &[f64]) -> Result<f64, Infallible> {
        Ok((x[0] - 1.0).powi(2) + (x[1] + 2.0).powi(2))
    }

    fn gradient(&mut self, x: &[f64], g: &mut [f64]) -> Result<(), Infallible> {
        g.copy_from_slice(&[2.0 * (x[0] - 1.0), 2.0 * (x[1] + 2.0)]);
        Ok(())
    }
}

#[test]
fn box_constraints_and_hessian() {
    let out = optim::lbfgsb(
        &mut Quadratic,
        vec![0.0, 0.0],
        &[(-0.5, 0.5); 2],
        optim::BoxControl {
            maxit: 100,
            lmm: 5,
            factr: 1e7,
            pgtol: 0.0,
        },
    )
    .unwrap();
    assert_eq!(out.convergence, 0);
    assert_eq!(out.par, [0.5, -0.5]);
    assert_eq!(out.fncount, 2);
    let h = optim::optim_hess(&mut Quadratic, &[1.0, -2.0], &[1e-3, 1e-3]).unwrap();
    for (actual, expected) in h.iter().zip([2.0, 0.0, 0.0, 2.0]) {
        assert!((actual - expected).abs() < 1e-12);
    }
}

struct Rosenbrock;

impl Objective for Rosenbrock {
    type Err = Infallible;

    fn value(&mut self, x: &[f64]) -> Result<f64, Infallible> {
        Ok(100.0 * (x[1] - x[0].powi(2)).powi(2) + (1.0 - x[0]).powi(2))
    }

    fn gradient(&mut self, x: &[f64], g: &mut [f64]) -> Result<(), Infallible> {
        let mut point = x.to_vec();
        let h = 1e-3;
        for i in 0..g.len() {
            point[i] = x[i] + h;
            let above = self.value(&point)?;
            point[i] = x[i] - h;
            let below = self.value(&point)?;
            point[i] = x[i];
            g[i] = (above - below) / (2.0 * h);
        }
        Ok(())
    }
}

#[test]
fn limited_memory_search_with_numerical_gradients() {
    let out = optim::lbfgsb(
        &mut Rosenbrock,
        vec![-1.2, 1.0],
        &[(f64::NEG_INFINITY, f64::INFINITY); 2],
        optim::BoxControl {
            maxit: 100,
            lmm: 5,
            factr: 1e7,
            pgtol: 0.0,
        },
    )
    .unwrap();
    assert_eq!(out.convergence, 0);
    assert_eq!(out.fncount, 49);
    assert_eq!(out.grcount, Some(49));
    assert!((out.par[0] - 0.9998000378).abs() < 1e-7);
    assert!((out.par[1] - 0.9996001158).abs() < 1e-7);
}

struct InteriorSingularity;

impl VecFn for InteriorSingularity {
    type Err = Infallible;

    fn eval(&mut self, x: &[f64], out: &mut [f64]) -> Result<(), Self::Err> {
        for (y, &x) in out.iter_mut().zip(x) {
            *y = (x - 0.231).abs().powf(-0.7);
        }
        Ok(())
    }
}

#[test]
fn interior_extrapolation_reports_a_valid_error() {
    let tolerance = f64::EPSILON.sqrt().sqrt();
    let out = quadpack::dqags(
        &mut InteriorSingularity,
        0.0,
        1.0,
        tolerance,
        tolerance,
        100,
    )
    .unwrap();
    let exact = (0.231_f64.powf(0.3) + 0.769_f64.powf(0.3)) / 0.3;
    assert_eq!(out.status, quadpack::Status::Ok, "{out:?}");
    assert!((out.value - exact).abs() <= out.abs_error, "{out:?}");
    assert!(
        (out.value - 5.228_219_101_338_566).abs() <= 0.00061,
        "{out:?}"
    );
}

struct InfiniteGradient;

impl Objective for InfiniteGradient {
    type Err = Infallible;

    fn value(&mut self, x: &[f64]) -> Result<f64, Infallible> {
        Ok(x[0] * x[0])
    }

    fn gradient(&mut self, _: &[f64], g: &mut [f64]) -> Result<(), Infallible> {
        g.fill(f64::INFINITY);
        Ok(())
    }
}

#[test]
fn infinite_gradients_stop_without_an_infinite_backtrack() {
    let control = optim::Control {
        maxit: 10,
        abstol: f64::NEG_INFINITY,
        reltol: 1e-8,
    };
    let bfgs = optim::bfgs(&mut InfiniteGradient, vec![1.0], control).unwrap();
    let cg = optim::cg(
        &mut InfiniteGradient,
        vec![1.0],
        control,
        optim::CgFormula::FletcherReeves,
    )
    .unwrap();
    for out in [bfgs, cg] {
        assert_eq!(out.par, [1.0]);
        assert_eq!(out.fncount, 1);
    }
}
