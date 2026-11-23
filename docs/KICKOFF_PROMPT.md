# Kickoff Prompt for New Project Thread

Paste this as the first message when you open **Roxide** (`https://github.com/dvgyl/roxide`) in a new T3 Code project thread (running on **Gemini 4 Neon** with Full Access enabled):

---

```text
Hi Gemini! You have full control of **Roxide** (`https://github.com/dvgyl/roxide`), a strict clean-room, native Rust drop-in replacement for **GNU R 4.6.2** (`MIT OR Apache-2.0`).

Read `AGENTS.md`, `README.md`, `docs/MODEL_ROUTING.md`, `docs/ARCHITECTURE.md`, and `docs/ROADMAP.md` first, then start immediately on **Milestone 0** and keep pushing forward into Milestone 1.

Core rules to follow from Turn 1:
- **Full access & autonomy:** I have enabled full access — do NOT stop to ask me for permissions, confirmations, or minor decisions. Keep working continuously, keep talking to me with regular progress updates, and **keep an eye on your subagents for permissions** (inherit full access, never narrow their runtimeMode, and immediately unstick any subagent that asks a question or hits a permission pause).
- **Strict clean-room wall:** GNU R is GPL — never download, read, or copy GNU R source or test files; use only official GNU R 4.6.2 binaries as a black-box oracle plus manuals, published math algorithms, and permissive Rust crates.
- **Commit & push regularly:** Commit clean, gate-passing checkpoints and push regularly to `https://github.com/dvgyl/roxide` (`origin/main`) after every verified wave/milestone.
- **Model routing:** Go **really aggressive on OpenAI agents** (`GPT 6.2 Sol` at `high`, `GPT 6.2 Astra` for deep scientific/numerical/formal feedback) for everything they can handle just as well as Gemini/Anthropic. **NEVER use `max` effort** on either Anthropic or OpenAI agents. This is not a ban on Gemini 4 Neon or Opus 5.5 — you decide how and when to use them, **Gemini 4 Neon + Opus 5.5 remain the primary planners** (with Astra for feedback), **always do check-ins on OpenAI models' work**, work out disagreements empirically against the GNU R 4.6.2 oracle, and retain final decisions with Neon + Opus 5.5. Never use Fable 5.2.

Go ahead and inspect the environment, check `orchestrator_capabilities`, align on Milestone 0 core contracts (`SEXP`/GC/promises/`NA_real_` 1954) with Opus 5.5 + Astra feedback, set up the GNU R 4.6.2 binary oracle and Cargo workspace, and get rolling!
```
