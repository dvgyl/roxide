# Kickoff / handoff prompt

> Milestones M0 through M3 are complete and M4 is at Wave 5. Use [`docs/HANDOFF_PROMPT.md`](HANDOFF_PROMPT.md) for the active continuation prompt.

```text
Continue Roxide (`https://github.com/dvgyl/roxide`) from `main`, working backwards from `docs/VISION.md`.

First read `AGENTS.md`, `docs/VISION.md`, `docs/ROADMAP.md`, `docs/ARCHITECTURE.md`, `docs/ADR.md` (especially ADRs 0021-0025), and `PROGRESS.md`.

Main rules:
- Follow the Top 3 priorities in `docs/VISION.md` (1. CRAN/Bioconductor pass rate, 2. binary repo + bundled toolchain + `faer` BLAS/LAPACK, 3. multi-session compiled packages) and the cut list in ADRs 0021-0025.
- Full autonomy (never pause for routine approvals), keep the user updated concisely, monitor delegated agents for stalls, and commit + push to `origin/main` after every verified wave.
- Enforce the strict clean-room MIT/Apache-2.0 wall, `docs/MODEL_ROUTING.md` (aggressive GPT 6.2 Sol `high`, behind-the-scenes Astra, Neon/Opus planning + check-ins, never `max` effort, never Fable 5.2, max 2 Cargo slots), and `scripts/gate.sh`.

Resume at M4 Wave 5 (`PROGRESS.md`), close M4, and keep pushing through M5-M8.
```
