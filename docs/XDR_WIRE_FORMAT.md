# GNU R 4.6.2 XDR v2 / v3 Wire Format Reference (Clean-Room Empirical Spec)

Empirically verified via black-box probes against official `R version 4.6.2 (2026-10-29)` (UTF-8 locale) and R Internals §1.8 documentation. Zero GNU R source code consulted.

## 1. Stream & File Headers

| Stream / File | Exact Leading Bytes |
| --- | --- |
| XDR v2 (`serialize(..., version = 2L)`) | `58 0a` (`X\n`), `00000002` (v2), `00040602` (4.6.2), `00020300` (min 2.3.0) |
| XDR v3 (`serialize(..., version = 3L)`) | `58 0a` (`X\n`), `00000003` (v3), `00040602` (4.6.2), `00030500` (min 3.5.0), `00000005` + `55 54 46 2d 38` (`UTF-8`) |
| `.rds` | Exact `serialize()` stream (optionally wrapped in gzip `1f8b08`, bzip2 `425a68`, xz `fd377a585a00`, or zstd `28b52ffd`) |
| `.RData` v2 | `52 44 58 32 0a` (`RDX2\n`) + complete XDR v2 stream of tagged `LISTSXP` (`00000402`) bindings ending in `000000fe` |
| `.RData` v3 | `52 44 58 33 0a` (`RDX3\n`) + complete XDR v3 stream of tagged `LISTSXP` bindings |

## 2. Flags Word (Big-Endian `u32`)

- Bits `0..7` (`0x0000_00ff`): `SEXPTYPE` or special wire tag (`238..=255`)
- Bit `8` (`0x0000_0100`): `IS_OBJECT`
- Bit `9` (`0x0000_0200`): `HAS_ATTR`
- Bit `10` (`0x0000_0400`): `HAS_TAG`
- Bits `12..27` (`0x0fff_f000`): `gp` / `LEVELS` shifted left by 12
  - `CHARSXP` `gp` bits: bit 1 (`0x0000_2000`) `BYTES`, bit 2 (`0x0000_4000`) `LATIN1`, bit 3 (`0x0000_8000`) `UTF8`, bit 6 (`0x0004_0000`) `ASCII`; bit 5 (`0x0002_0000` cache) is cleared on write.
  - `NA_character_`: `00000009 ffffffff` (length `-1`).
- Vector length: `0..=0x7fff_ffff` as one `i32`; long vectors write `0xffff_ffff` followed by `u32` high + `u32` low. No padding after byte payloads.

## 3. Special Wire Tags (`238..=255`)

| Dec | Hex | Name | Meaning |
| ---: | --- | --- | --- |
| 255 | `ff` | `REFSXP` | Reference-table lookup: inline `(idx << 8) | 0xff` for `1..=0x007fffff`, else `000000ff` + `u32` index |
| 254 | `fe` | `NILVALUE_SXP` | `NULL` / pairlist terminator |
| 253 | `fd` | `GLOBALENV_SXP` | `.GlobalEnv` |
| 252 | `fc` | `UNBOUNDVALUE_SXP` | Unforced promise value sentinel |
| 251 | `fb` | `MISSINGARG_SXP` | `R_MissingArg` |
| 250 | `fa` | `BASENAMESPACE_SXP` | `.BaseNamespaceEnv` |
| 249 | `f9` | `NAMESPACESXP` | `00000000` + `u32` count + `CHARSXP`s |
| 248 | `f8` | `PACKAGESXP` | `00000000` + `u32` count + `CHARSXP`s |
| 247 | `f7` | `PERSISTSXP` | `00000000` + `u32` count + `CHARSXP`s |
| 242 | `f2` | `EMPTYENV_SXP` | `emptyenv()` |
| 241 | `f1` | `BASEENV_SXP` | `baseenv()` |
| 238 | `ee` | `ALTREP_SXP` | Class-info pairlist (`compact_intseq`, `base`, `13L`), state `REALSXP(len, start, incr)`, attrs |

## 4. Payload & Reference Table Ordering

- Reference table is 1-based in traversal order; records `SYMSXP`, ordinary `ENVSXP` (registered **before** reading frame/enclos to support cycles!), `EXTPTRSXP`, `WEAKREFSXP`, `NAMESPACESXP`, `PACKAGESXP`, `PERSISTSXP`. Singletons, atomic vectors, `CHARSXP`, closures, and pairlists do **not** consume ref indices.
- `LISTSXP`/`LANGSXP`/`DOTSXP`: optional attr (if `HAS_ATTR`), optional tag (if `HAS_TAG`), `car`, `cdr`.
- `CLOSXP`: optional attr, `env` (in tag slot, `HAS_TAG`), `formals`, `body`.
- `ENVSXP`: pre-register ref, `u32` locked (`0`/`1`), `enclos`, `frame`, `hashtab`, `attrs`.
- `PROMSXP`: optional attr, `env` (if `HAS_TAG`), `value`, `expr`.
- Ordinary vectors: length, elements (`NA_real_` = `7ff00000000007a2`, `NaN` = `7ff8000000000000`, `-0.0` = `8000000000000000`), then optional attr pairlist if `HAS_ATTR`.
