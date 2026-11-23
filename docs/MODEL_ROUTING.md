# Model Routing Policy

These model routing rules are the **default for every session** in Roxide unless one of the two exceptions below applies.

## Exceptions (do not apply model routing when)

1. The user explicitly says not to apply them.
2. The task is menial (for example, a one-time folder arrangement or a small single-file edit).

## Repo-Specific Rule for Roxide

- **Primary coordinator & primary planners:** Run the main thread on **Gemini 4 Neon** (or **Opus 5.5** if the user starts there). **Gemini 4 Neon and Opus 5.5 remain the primary planners** (core architecture, GC/`SEXP`/promise/C-API contracts, milestone roadmaps), calling on **GPT 6.2 Astra** for deep scientific, numerical, and formal-semantics feedback. Final decisions always rest with **Gemini 4 Neon** and **Opus 5.5**.
- **Go REALLY aggressive on OpenAI usage (default workhorse rule):**
  - **Use OpenAI agents for everything that can be done on them where using Gemini or Anthropic wouldn't make a meaningful difference.**
  - **GPT 6.2 Sol (`codex`, `reasoningEffort: "high"`) — GO HARD:** Use **GPT 6.2 Sol** extensively and aggressively as the primary parallel implementation fleet, primary code/work reviewer, differential oracle triage engine, clean-room conformance author, and web-search co-lead. Keep multiple GPT 6.2 Sol workers running concurrently across disjoint crates and stdlib submodules (respecting the 2-Cargo-slot RAM cap).
  - **GPT 6.2 Astra — Deep scientific, numerical & formal semantics feedback (HUGE strength for GNU R!):** Invoke **GPT 6.2 Astra** aggressively *behind the scenes* for planning feedback and hard scientific, statistical, numerical, or formal-semantics analysis:
    - Numerical accuracy & tail stability for `d/p/q/r*` probability distributions, special functions, `rhyper()` large-population regimes, `lm`/`glm` IRLS + QR pivoting, `optim`/`nlminb` convergence, and PRNG bit-exactness
    - IEEE 754 `NA_real_` (payload `1954`) vs `NaN` propagation and Kleene three-valued logic
    - Formal lazy promise (`PROMSXP`) / `substitute()` / NSE / lexical-frame evaluation semantics
    - S3/S4 multiple-dispatch & group-generic resolution rules
    - Cycle-safe GC rooting across conditions (`tryCatch` / `on.exit`) and R 4.6.2's C API (`PROTECT`/`UNPROTECT`, `ALTREP`)
    - **Never** expose GPT 6.2 Astra directly to the user — it is poor at human communication and intent; the main agent evaluates its feedback and retains final decision authority.
- **Effort cap — NEVER use Max effort on Anthropic or OpenAI agents:**
  - Do **not** use `max` effort (`effort: "max"` / `reasoningEffort: "max"`) on either **Anthropic** (`Opus 5.5`) or **OpenAI** (`GPT 6.2 Sol`, `GPT 6.2 Astra`) agents. Stick to `high` (or lower where appropriate).
- **Mandatory check-ins on OpenAI models' work:**
  - **Always do check-ins on OpenAI models' work.** Do not blindly rubber-stamp or auto-merge OpenAI outputs: the main thread (**Gemini 4 Neon**, plus **Opus 5.5** on major slices) must actively inspect diffs, architectural & clean-room compliance, numerical tolerances, and black-box **GNU R 4.6.2** oracle results.
- **Not a ban on Gemini 4 Neon or Opus 5.5 (normal/measured usage, coordinator's discretion):**
  - Still use **Gemini 4 Neon** and **Opus 5.5** whenever they genuinely add value — **the Gemini 4 Neon coordinator in the active chat decides how and when to use them**, while keeping Anthropic usage **normal and measured** (not spawning routine Opus build swarms when GPT 6.2 Sol can do the job just as well).
  - **Best uses for Opus 5.5:**
    1. **Primary co-planning & big architectural discussions with Gemini 4 Neon** (`roxide-gc`, `roxide-value`, `roxide-eval`, `roxide-capi` contracts, milestone blueprints, GC/ALTREP design — with GPT 6.2 Astra providing technical feedback).
    2. **High-leverage cross-model check-ins & reviews:** Reviewing major milestone merges, core contract changes, `unsafe` FFI audits in `roxide-capi`, or subtle GPT 6.2 Sol work where a second frontier model family catches blind spots.
    3. **Selective critical build slices** when the coordinator judges Opus 5.5 makes a real difference.
  - Spread Opus 5.5 usage across **Cursor Pro**, **Gemini Ultra**, and **Claude Ultra** allowances rather than hitting only the Claude plan, and use Gemini 4 Neon allowance from **Cursor Pro** as well.
- **Disagreement resolution (strict):** Never blindly agree with any model (**Opus 5.5**, **GPT 6.2 Sol**, or **GPT 6.2 Astra**). Verify factual, semantic, or numerical disputes empirically against the codebase and black-box **GNU R 4.6.2** oracle probes (`Rscript --vanilla`), work through disagreements to reach agreement, and if deadlocked, **Gemini 4 Neon and Opus 5.5 retain the final decision**.
- **Last resort only:** Do **not** use **Fable 5.2** unless the user explicitly asks for it.

## General Model Routing (Default Across Sessions)

### Harness and Active Subscriptions

- **Harness:** T3 Code
- **Subscriptions:** OpenAI ChatGPT Pro, Cursor Pro, Gemini Ultra, Claude Ultra
- Use **Opus 5.5** allowances across Cursor and Gemini plans as well, not only the Claude plan.
- Use **Gemini 4 Neon** allowance from the Cursor plan as well.

### Role Assignments

| Role | Model(s) | Usage Profile in Roxide |
| --- | --- | --- |
| Main thread, orchestration & final decisions | **Gemini 4 Neon** (primary) & **Opus 5.5** | Main thread runs on Gemini 4 Neon (or Opus 5.5). Big architectural discussions happen between Gemini 4 Neon and Opus 5.5. |
| Primary parallel builders, triage & work reviews | **GPT 6.2 Sol (`high`)** | **Aggressive / heavy usage.** Runs most implementation waves, differential oracle audits, conformance suites, and extensive code/work reviews. |
| Deep scientific / numerical / semantics advisor | **GPT 6.2 Astra** | **Aggressive for hard problems (internal only).** Consulted by the main agent for numerical algorithms, statistical math, formal R evaluation/GC semantics; never user-facing. |
| Strategic co-architect & selective cross-model review | **Opus 5.5** | **Normal / measured usage.** Big design discussions, milestone blueprints, `unsafe` FFI audits, and high-impact cross-model reviews of major GPT 6.2 Sol work. |
| Web search & spec lookup | Mix of **GPT 6.2 Sol** and **Sonnet 5.5** (or other agents) | OpenAI models excel at web search; pair GPT 6.2 Sol with Sonnet 5.5 when checking R 4.6.2 manuals, published numerical algorithms, or permissive crate licences. |
| Last resort only | **Fable 5.2** | Forbidden unless explicitly requested by the user. |

## Recommended Active Fleet Template (16 GB RAM Safe)

During active implementation waves, a balanced, high-throughput fleet looks like:
- **Main thread:** `Gemini 4 Neon` (coordinating, synthesizing, keeping the user updated, merging clean waves)
- **Cargo Build Slot 1:** `GPT 6.2 Sol (high)` implementing Crate/Submodule A
- **Cargo Build Slot 2:** `GPT 6.2 Sol (high)` implementing Crate/Submodule B
- **Zero-Cargo Slot 3:** `GPT 6.2 Sol (high)` reviewing the previous wave or running black-box GNU R 4.6.2 differential triage
- **Zero-Cargo Slot 4 (frequent in Roxide!):** `GPT 6.2 Astra` designing/verifying numerical algorithms, distribution tail precision, or tricky promise/S3/S4/GC invariants for the next wave
- **Zero-Cargo Slot 5 (selective / milestone-level):** `Opus 5.5 (high)` discussing big architectural contracts with Gemini 4 Neon or cross-model reviewing a major milestone/FFI slice
