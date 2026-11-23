//! Exercise lazy-load databases and archive extraction through an in-memory host.

use std::io::{Cursor, Write};
use std::path::Path;

use roxide_base::serialize::{PersistHook, serialize_bytes, unserialize_bytes};
use roxide_eval::{Ctx, EvalResult};
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::{Host, OpenMode, PureHost};
use roxide_value::{GLOBAL_ENV, ValueApi};

fn context(host: PureHost) -> Ctx {
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).expect("create context");
    roxide_base::register_all(&mut ctx).expect("register base");
    roxide_pkg::register(&mut ctx).expect("register package functions");
    ctx
}

#[test]
fn lazy_bindings_preserve_environment_identity_and_frame_locks() {
    let mut ctx = context(PureHost::new());
    ctx.run_global(
        "fixture",
        r#"
        e <- new.env()
        a <- new.env(parent = baseenv())
        b <- new.env(parent = baseenv())
        a$value <- 11L
        a$b <- b
        b$a <- a
        a$f <- eval(quote(function() value), a)
        attr(a, "label") <- "test"
        lockEnvironment(a, bindings = TRUE)
        e$both <- list(a, a, b)
        e$other <- b
        e$skip <- 0L
    "#,
    )
    .expect("set up database");
    let sym = ctx.session_mut().intern("e").expect("intern e");
    let source = ctx
        .get_local(sym, GLOBAL_ENV)
        .expect("read e")
        .expect("find e");
    let base = Path::new("/work/test");
    roxide_pkg::lazyload::write_lazyload_db(&mut ctx, base, source).expect("write database");
    ctx.run_global(
        "check",
        r#"
        dst <- new.env()
        lazyLoad("/work/test", dst, function(n) n != "skip")
        stopifnot(!exists("skip", dst, inherits = FALSE))
        stopifnot(identical(dst$other, dst$both[[3L]]))
        x <- dst$both[[1L]]
        stopifnot(identical(x, dst$both[[2L]]), identical(x, dst$other$a))
        stopifnot(identical(x$b, dst$other), identical(environment(x$f), x))
        stopifnot(x$f() == 11L, attr(x, "label") == "test")
        stopifnot(environmentIsLocked(x), !bindingIsLocked("value", x))
        x$value <- 12L
        stopifnot(x$f() == 12L)
        stopifnot(identical(lazyLoadDBexec("/work/test", function(s) s$vars,
                                        function(n) n == "other"), "other"))
    "#,
    )
    .expect("read database");
}

struct Hook {
    input: SexpId,
    output: SexpId,
    count: usize,
}

impl PersistHook for Hook {
    fn write(&mut self, ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<SexpId>> {
        assert_eq!(value, self.input);
        Ok(Some(ctx.session_mut().scalar_str("env::1")?))
    }

    fn read(&mut self, ctx: &mut Ctx, key: SexpId) -> EvalResult<SexpId> {
        self.count += 1;
        assert_eq!(ctx.session().str_slice(key)?.len(), 1);
        ctx.safepoint();
        Ok(self.output)
    }
}

#[test]
fn persistent_references_contribute_to_symbol_reference_numbers() {
    let mut ctx = context(PureHost::new());
    let a = ctx
        .session_mut()
        .alloc_env(GLOBAL_ENV)
        .expect("allocate input");
    let b = ctx
        .session_mut()
        .alloc_env(GLOBAL_ENV)
        .expect("allocate output");
    ctx.protect(b);
    let sym = ctx.session_mut().intern("x").expect("intern x").as_sexp();
    let data = ctx.session_mut().alloc_list(4).expect("allocate data");
    for (i, value) in [a, a, sym, sym].into_iter().enumerate() {
        ctx.session_mut()
            .list_set(data, i, value)
            .expect("set value");
    }
    let mut hook = Hook {
        input: a,
        output: b,
        count: 0,
    };
    let mut data = serialize_bytes(&mut ctx, data, 3, Some(&mut hook)).expect("encode");
    assert!(data.ends_with(&[0, 0, 3, 255]));
    let actual = unserialize_bytes(&mut ctx, &data, Some(&mut hook)).expect("decode");
    assert_eq!(
        ctx.session().list_slice(actual).expect("read data"),
        &[b, b, sym, sym]
    );
    assert_eq!(hook.count, 2);
    // Replace the second persistent item with a reference to the first.
    data.splice(57..83, [0, 0, 1, 255]);
    let len = data.len();
    data[len - 2] = 2;
    let actual = unserialize_bytes(&mut ctx, &data, Some(&mut hook)).expect("decode reference");
    assert_eq!(
        ctx.session().list_slice(actual).expect("read data"),
        &[b, b, sym, sym]
    );
    assert_eq!(hook.count, 3);
    for len in 0..data.len() {
        assert!(unserialize_bytes(&mut ctx, &data[..len], Some(&mut hook)).is_err());
    }
}

#[test]
fn malformed_records_fail_when_forced() {
    let mut ctx = context(PureHost::new());
    ctx.run_global(
        "malformed database",
        r#"
        p <- "/work/bad"
        writeBin(as.raw(c(255L, 255L, 255L, 255L, 0L)), paste0(p, ".rdb"))
        saveRDS(list(variables = list(a = c(0L, 5L), b = c(5L, 9L)),
                     references = list(), compressed = TRUE), paste0(p, ".rdx"))
        e <- new.env()
        lazyLoad(p, e)
        failed <- function(x) tryCatch({ force(x); FALSE }, error = function(e) TRUE)
        stopifnot(failed(e$a), failed(e$b))
    "#,
    )
    .expect("reject malformed records");
}

fn tar_entry(path: &str, kind: u8, data: &[u8]) -> Vec<u8> {
    let mut header = [0; 512];
    header[..path.len()].copy_from_slice(path.as_bytes());
    header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    let checksum: u32 = header.iter().map(|&byte| u32::from(byte)).sum();
    header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(data);
    bytes.resize(bytes.len().next_multiple_of(512), 0);
    bytes
}

#[test]
fn archive_extracts_supported_compression_and_rejects_unsafe_paths() {
    let mut archive = tar_entry("mypkg/", b'5', &[]);
    archive.extend(tar_entry("mypkg/DESCRIPTION", b'7', b"Package: mypkg\n"));
    archive.extend([0; 512]);
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&archive).expect("compress gzip");
    let mut bzip = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    bzip.write_all(&archive).expect("compress bzip");
    let mut xz = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(0))
        .expect("create xz encoder");
    xz.write_all(&archive).expect("compress xz");
    for input in [
        archive,
        gzip.finish().expect("finish gzip"),
        bzip.finish().expect("finish bzip"),
        xz.finish().expect("finish xz"),
    ] {
        let mut host = PureHost::new();
        host.insert_file("/work/pkg.tar", input);
        let root = Path::new("/work/out");
        host.create_dir(root, false, 0o700)
            .expect("create extraction root");
        let dest = roxide_pkg::tar::extract(&mut host, Path::new("/work/pkg.tar"), root)
            .expect("extract archive");
        let mut file = host
            .open(&dest.join("DESCRIPTION"), OpenMode::read_only())
            .expect("open file");
        let mut actual = Cursor::new(Vec::new());
        std::io::copy(&mut file, &mut actual).expect("read file");
        assert_eq!(actual.into_inner(), b"Package: mypkg\n");
    }

    for (path, kind) in [("../escape", b'0'), ("/escape", b'0'), ("mypkg/link", b'2')] {
        let mut bytes = tar_entry(path, kind, &[]);
        bytes.extend([0; 1024]);
        let mut host = PureHost::new();
        host.insert_file("/work/pkg.tar", bytes);
        let root = Path::new("/work/out");
        host.create_dir(root, false, 0o700)
            .expect("create extraction root");
        assert!(roxide_pkg::tar::extract(&mut host, Path::new("/work/pkg.tar"), root).is_err());
        assert!(host.metadata(Path::new("/work/escape"), false).is_err());
    }
}

#[test]
fn bytecode_rejects_a_reference_to_an_uninitialized_slot() {
    let mut bytes = b"X\n".to_vec();
    for word in [2u32, 0x40602, 0x20300, 21, 1, 13, 1, 12, 1, 243, 0] {
        bytes.extend(word.to_be_bytes());
    }
    let mut ctx = context(PureHost::new());
    assert!(unserialize_bytes(&mut ctx, &bytes, None).is_err());
}

#[test]
fn archive_uses_pax_and_gnu_long_paths() {
    let name = format!("mypkg/{}", "a".repeat(125));
    let mut pax = format!(" path={name}\n");
    let length = pax.len() + (pax.len() + 3).to_string().len();
    pax.insert_str(0, &length.to_string());
    let long = format!("{name}\0");
    for (kind, metadata) in [(b'x', pax.as_bytes()), (b'L', long.as_bytes())] {
        let mut archive = tar_entry("metadata", kind, metadata);
        archive.extend(tar_entry("placeholder", b'0', b"payload"));
        archive.extend([0; 1024]);
        let mut host = PureHost::new();
        host.insert_file("/work/pkg.tar", archive);
        let root = Path::new("/work/out");
        host.create_dir(root, false, 0o700)
            .expect("create extraction root");
        roxide_pkg::tar::extract(&mut host, Path::new("/work/pkg.tar"), root)
            .expect("extract long path");
        assert_eq!(
            host.metadata(&root.join(&name), false)
                .expect("long path")
                .len,
            7
        );
    }
}

#[test]
fn lazy_bindings_resolve_deep_environment_chains() {
    let mut ctx = context(PureHost::new());
    ctx.run_global(
        "fixture",
        r#"
        root <- new.env(parent = baseenv())
        root$value <- 12L
        node <- root
        for (i in 1:80) node <- new.env(parent = node)
        source <- new.env()
        source$leaf <- node
        source$root <- root
    "#,
    )
    .expect("create environment chain");
    let sym = ctx.session_mut().intern("source").expect("intern source");
    let source = ctx
        .get_local(sym, GLOBAL_ENV)
        .expect("read source")
        .expect("find source");
    roxide_pkg::lazyload::write_lazyload_db(&mut ctx, Path::new("/work/deep"), source)
        .expect("write environment chain");
    ctx.run_global(
        "check",
        r#"
        dst <- new.env()
        lazyLoad("/work/deep", dst)
        last <- dst$leaf
        for (i in 1:80) last <- parent.env(last)
        stopifnot(identical(last, dst$root), last$value == 12L)
    "#,
    )
    .expect("read environment chain");
}

#[test]
fn bytecode_consumes_attributes_before_the_next_item() {
    let mut bytes = b"X\n".to_vec();
    for word in [
        2u32,
        0x40602,
        0x20300,
        19,
        2,
        21 | (1 << 9),
        0,
        13,
        0,
        1,
        13,
        13,
        1,
        7,
        254,
        13,
        1,
        9,
    ] {
        bytes.extend(word.to_be_bytes());
    }
    let mut ctx = context(PureHost::new());
    let list = unserialize_bytes(&mut ctx, &bytes, None).expect("read bytecode in a list");
    let first = ctx.session().list_get(list, 0).expect("read first item");
    let next = ctx.session().list_get(list, 1).expect("read next item");
    assert_eq!(ctx.session().int(first).expect("source constant").get(0), 7);
    assert_eq!(ctx.session().int(next).expect("integer").get(0), 9);
}
