//! Check arithmetic order and factorization results against GNU R.

use std::process::Command;

use roxide_math::{
    MatRef,
    linalg::{
        blas::matprod,
        factor::{dgeqp3, dgetrf, matrix_norm_inf, matrix_norm_one},
        linpack::{dqrdc2, dqrls},
    },
};

fn oracle(script: &str) -> Option<Vec<u64>> {
    let Some(path) = std::env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        eprintln!("Set ROXIDE_ORACLE_RSCRIPT to run the live comparisons.");
        return None;
    };
    let script = format!(
        "emit <- function(x) cat(sprintf('%02x', as.integer(writeBin(as.double(x), raw(), size=8, endian='big'))), sep='')
        {script}"
    );
    let output = Command::new(path)
        .args(["--vanilla", "-e", &script])
        .output()
        .expect("run oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len().is_multiple_of(16));
    Some(
        output
            .stdout
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| {
                u64::from_str_radix(std::str::from_utf8(b).expect("hexadecimal"), 16)
                    .expect("double bits")
            })
            .collect(),
    )
}

#[test]
fn matrix_product_retains_fused_low_bits() {
    let x = 1.0 + 2.0_f64.powi(-27);
    let mut out = [0.0];
    matprod(
        MatRef::new(&[-1.0, x], 1, 2).unwrap(),
        MatRef::new(&[1.0, x], 2, 1).unwrap(),
        &mut out,
    )
    .unwrap();
    assert_eq!(
        out[0].to_bits(),
        (2.0_f64.powi(-26) + 2.0_f64.powi(-54)).to_bits()
    );
}

#[test]
fn limited_pivoting_qr_matches_packed_bits() {
    let Some(values) = oracle(
        r#"
        set.seed(246)
        fixtures <- c(list(matrix(c(1,2,3,4,2,1,0,3,3,3,3,7),4),
            cbind(c(1,0),c(1,.001)), cbind(c(1,0),c(1,.0010000005)),
            cbind(a=0,b=c(1,0,0,0),c=0,d=c(0,1,0,0),e=c(1,1,0,0))),
            lapply(2:14, function(n) matrix(runif(n*3,-1,1),n)))
        for (x in fixtures) {
            q <- qr(x)
            emit(c(dim(x),x,q$qr,q$qraux,q$pivot,q$rank))
        }
    "#,
    ) else {
        return;
    };
    let mut values = values.as_slice();
    let mut fixtures = 0;
    while !values.is_empty() {
        let (n, k) = (
            f64::from_bits(values[0]) as usize,
            f64::from_bits(values[1]) as usize,
        );
        let (x, next) = values[2..].split_at(n * k);
        let (data, next) = next.split_at(n * k);
        let (aux, next) = next.split_at(k);
        let (pivot, next) = next.split_at(k);
        let q = dqrdc2(x.iter().copied().map(f64::from_bits).collect(), n, k, 1e-7).unwrap();
        let bits = |v: &[f64]| v.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&q.qr), data, "packed QR on fixture {fixtures}");
        assert_eq!(bits(&q.qraux), aux, "aux on fixture {fixtures}");
        assert_eq!(
            q.pivot,
            pivot
                .iter()
                .map(|&v| f64::from_bits(v) as usize - 1)
                .collect::<Vec<_>>()
        );
        assert_eq!(q.rank, f64::from_bits(next[0]) as usize);
        fixtures += 1;
        values = &next[1..];
    }
    assert_eq!(fixtures, 17);
}

#[test]
fn rcond_matches_reference_probes() {
    let h: Vec<_> = (0..6)
        .flat_map(|j| (0..6).map(move |i| 1.0 / (i + j + 1) as f64))
        .collect();
    let cases = [
        (vec![1.0, 2.0, 3.0, 7.0], 2),
        (h, 6),
        (vec![1.0, 2.0, 2.0, 4.0], 2),
        (vec![-1.0, 1.0, 1.0, -1.0, 1.0, -3.0, -3.0, -3.0, 0.0], 3),
    ];
    let expected = [
        0x3f86_c16c_16c1_6c16,
        0x3f86_c16c_16c1_6c17,
        0x3e62_77d1_19df_1f25,
        0x3e62_77d1_19df_20d7,
        0,
        0,
        (1.0_f64 / 3.0).to_bits(),
        (1.0_f64 / 5.0).to_bits(),
    ];
    if let Some(values) = oracle(
        r#"
        xs <- list(matrix(c(1,2,3,7),2),outer(1:6,1:6,function(i,j) 1/(i+j-1)),
                   matrix(c(1,2,2,4),2),matrix(c(-1,1,1,-1,1,-3,-3,-3,0),3))
        for (x in xs) emit(c(rcond(x,'O'),rcond(x,'I')))
    "#,
    ) {
        assert_eq!(values, expected);
    }
    for ((x, n), ref_bits) in cases.into_iter().zip(expected.as_chunks::<2>().0) {
        let view = MatRef::new(&x, n, n).unwrap();
        let (one, inf) = (matrix_norm_one(view), matrix_norm_inf(view));
        let lu = dgetrf(x, n).unwrap();
        let got = [
            lu.rcond(one, true).to_bits(),
            lu.rcond(inf, false).to_bits(),
        ];
        assert_eq!(
            got, *ref_bits,
            "order {n}, got {got:x?}, expected {ref_bits:x?}"
        );
    }
}

#[test]
fn least_squares_effects_and_lapack_signed_zero() {
    let a = vec![
        0., 0., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0., 1., 0., 0., 1., 1., 0., 0.,
    ];
    let y = [1., 2., 4., 3., 1., -1., 2., 5.];
    let fit = dqrls(a, 4, 5, MatRef::new(&y, 4, 2).unwrap(), 1e-7).unwrap();
    assert_eq!(fit.qr.pivot, [1, 3, 0, 2, 4]);
    assert_eq!(fit.coefficients, [1., 2., 0., 0., 0., 1., -1., 0., 0., 0.]);
    assert_eq!(fit.effects, [-1., -2., 4., 3., -1., 1., 2., 5.]);
    assert_eq!(fit.residuals, [0., 0., 4., 3., 0., 0., 2., 5.]);
    let q = dgeqp3(vec![-0., 4.], 2, 1).unwrap();
    assert_eq!(q.qr, [4., -1.]);
}
