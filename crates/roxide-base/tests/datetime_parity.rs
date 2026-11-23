#![forbid(unsafe_code)]

//! Compare date-time values with GNU R 4.6.2 output.

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, CtxExt, Dispatch, EvalResult, Module, PrimDef, PrimFn,
    PrimKind, Visibility,
};
use roxide_gc::{GcPolicy, Trace, Tracer};
use roxide_host::{Host, NativeHost, PureHost};
use roxide_value::{DeparseOpts, GLOBAL_ENV, dput};

const CASES: &str = include_str!("../../../conformance/layer1/06_files_io_datetime.rcase");

#[derive(Default)]
struct Output(String);

impl Trace for Output {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Output {}

const fn primitive(name: &'static str, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Primitive,
        args: ArgEval::Eval,
        arity: Arity::Exact(1),
        dispatch: Dispatch::None,
        visibility: Visibility::Off,
    }
}

const TEST_MODULE: Module = Module {
    prims: &[primitive("dput", capture)],
    prelude: "",
};

fn capture(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    for line in dput(ctx.session(), x, DeparseOpts::default())? {
        let out = &mut ctx.ext::<Output>().0;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(x)
}

fn run(name: &str, script: &str, host: Box<dyn Host>) -> String {
    let mut ctx = Ctx::new(host, GcPolicy::Torture).expect("Create R session");
    roxide_base::register_all(&mut ctx).expect("Register base modules");
    ctx.register_module(&TEST_MODULE)
        .expect("Register test functions");
    ctx.run_script(name, script, GLOBAL_ENV, false)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    std::mem::take(&mut ctx.ext::<Output>().0)
}

#[test]
fn layer_one_datetime_cases_match_oracle_bytes() {
    let expected = [
        (
            "date_parsing_formatting_and_origins",
            concat!(
                "c(11015, 11016, NA)\n",
                "c(\"2000-02-28\", \"2000-02-29\", NA)\n",
                "c(\"1969-12-31\", \"1970-01-01\", \"1970-01-02\")\n",
            ),
        ),
        (
            "date_sequences_and_differences",
            concat!(
                "c(\"2024-02-27\", \"2024-02-29\", \"2024-03-02\")\n",
                "c(\"2024-01-01\", \"2024-02-01\", \"2024-03-01\")\n",
                "4\n\"days\"\n",
            ),
        ),
        (
            "difftime_in_seconds_and_days",
            "c(129600, 1.5)\nc(\"secs\", \"days\")\n",
        ),
        (
            "posixct_epoch_and_utc_format",
            concat!(
                "c(-1, 0, 951827696, NA)\n",
                "c(\"1969-12-31 23:59:59 +0000\", \"1970-01-01 00:00:00 +0000\", \"2000-02-29 12:34:56 +0000\", \n",
                "NA)\n",
            ),
        ),
        (
            "posixlt_utc_calendar_fields",
            concat!(
                "list(sec = c(56, 0), min = c(34L, 0L), hour = c(12L, 0L), mday = c(29L, \n",
                "1L), mon = 1:2, year = c(100L, 100L), wday = 2:3, yday = 59:60, \n",
                "    isdst = c(0L, 0L))\n",
                "c(951827696, 951868800)\n",
            ),
        ),
        (
            "strptime_and_strftime_with_utc_and_offset",
            concat!(
                "c(951782400, 951878700, NA)\n",
                "c(\"2000-02-29 00:00:00 +0000\", \"2000-03-01 02:45:00 +0000\", NA\n",
                ")\n",
            ),
        ),
    ];
    for (name, output) in expected {
        let marker = format!("=== case: {name} ===\n");
        let script = CASES.split_once(&marker).expect("Find conformance case").1;
        let script = script.split("\n=== case: ").next().expect("Read case body");
        assert_eq!(
            run(name, script, Box::new(PureHost::new())),
            output,
            "{name}"
        );
    }
}

#[test]
fn leap_years_month_rollover_and_units_match_oracle() {
    let script = r#"
dput(format(as.Date(c("1900-02-29", "2000-02-29", "2024-02-29", "2100-02-29", "1969-12-31"), format = "%F")))
dput(format(seq(as.Date("2024-01-31"), by = "month", length.out = 5)))
dput(format(seq(as.Date("2024-01-31"), by = "quarter", along.with = c(1, 2, 3))))
dput(diff(seq(as.Date("2024-01-31"), by = "month", length.out = 5), differences = 2))
delta <- as.difftime(1, units = "weeks")
units(delta) <- "hours"
dput(delta)
dput(difftime(as.POSIXct(c(3600, NA)), as.POSIXct(0)))
dput(format(strptime(c("Mon, 29 February 2000 @ 12:03:04.125 %", "bad"), "%a, %e %B %Y @ %H:%M:%OS %%", "UTC"), "%A %j %OS3 %z %Z"))
dput(format(strptime(c("00-060", "24-366"), "%y-%j", "UTC"), "%F"))
dput(format(as.POSIXct(c(0, 0.5, NA), tz = "UTC"), "%OS3"))
dput(as.difftime("24:00:00"))
dput(as.POSIXct(as.Date(0.5), tz = "UTC"))
"#;
    let expected = concat!(
        "c(NA, \"2000-02-29\", \"2024-02-29\", NA, \"1969-12-31\")\n",
        "c(\"2024-01-31\", \"2024-03-02\", \"2024-03-31\", \"2024-05-01\", \"2024-05-31\"\n)\n",
        "c(\"2024-01-31\", \"2024-05-01\", \"2024-07-31\")\n",
        "structure(c(-2, 2, -1), class = \"difftime\", units = \"days\")\n",
        "structure(168, units = \"hours\", class = \"difftime\")\n",
        "structure(c(1, NA), class = \"difftime\", units = \"hours\")\n",
        "c(\"Tuesday 060 04.125 +0000 UTC\", NA)\n",
        "c(\"2000-02-29\", \"2024-12-31\")\n",
        "c(\"00.000\", \"00.500\", NA)\n",
        "structure(1, class = \"difftime\", units = \"days\")\n",
        "structure(43200, class = c(\"POSIXct\", \"POSIXt\"), tzone = \"UTC\")\n",
    );
    assert_eq!(
        run("calendar and units", script, Box::new(PureHost::new())),
        expected
    );
}

#[test]
fn named_zone_uses_local_offsets_and_daylight_saving() {
    let host = NativeHost::new();
    if host.tzif("America/New_York").is_err() {
        return;
    }
    let script = r#"
dput(format(as.POSIXct(c(0, 1719792000), tz = "UTC"), "%F %T %z %Z", tz = "America/New_York"))
dput(format(strptime("2024-07-01 01:30:00 +0130", "%F %T %z", "America/New_York"), "%F %T %z %Z"))
dput(as.Date(as.POSIXct(0, tz = "UTC"), tz = "America/New_York"))
dput(as.Date(as.POSIXlt(as.POSIXct(0, tz = "UTC"), tz = "America/New_York")))
dput(format(as.POSIXct(c("2050-01-01 00:00", "2050-07-01 00:00"), tz = "America/New_York"), "%F %T %z %Z"))
"#;
    let expected = concat!(
        "c(\"1969-12-31 19:00:00 -0500 EST\", \"2024-06-30 20:00:00 -0400 EDT\"\n)\n",
        "\"2024-06-30 20:00:00 -0400 EDT\"\n",
        "structure(-1, class = \"Date\")\n",
        "structure(-1, class = \"Date\")\n",
        "c(\"2050-01-01 00:00:00 -0500 EST\", \"2050-07-01 00:00:00 -0400 EDT\"\n)\n",
    );
    assert_eq!(run("named time zone", script, Box::new(host)), expected);
}
