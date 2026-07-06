# Multi-Repo Bootstrap for Cloud Sessions — Implementation Plan

## Problem

Cloud sessions root into one repo (`lex-fmt/lex` in the common case). The GitHub MCP server is hard-scoped to that repo — any `mcp__github__*` call against a sibling repo returns "Access denied". Many planning and cross-cutting tasks need to read sibling repos (`comms`, `vscode`, `lexed`, `nvim`, `tree-sitter-lex`, `zed-lex`, `mkdocs-lex`).

`gh` works cross-repo (verified — see session log), but each agent reinvents the wheel: where to clone, what depth, whether to bootstrap deps. Result: redundant clones in random `/tmp` paths per session, no shared convention.

## Design at a glance

Three pieces, each as small as possible:

1. **`clone-lex-repos`** — env-level shell script installed by `arthur-debert/release env/setup.sh`. On `$PATH` in every cloud session.
2. **Standard checkout location** — `/tmp/lex-fmt/<repo>`. Documented in user-home `~/.claude/CLAUDE.md` (distinct from this repo's `CLAUDE.md`) so agents grep here before re-cloning.
3. **`lex-multirepo` skill** — installed at `~/.claude/skills/lex-multirepo/SKILL.md`. Triggers on tasks that name a sibling repo or span ≥2 lex-fmt repos. Tells the agent to run the script with the right subset, then read from `/tmp/lex-fmt/`.

The script is the load-bearing piece; the skill is just a thin instruction layer that points agents at it.

---

## The script: `clone-lex-repos`

### Signature

```sh
clone-lex-repos [--all] [--with-setup] [--depth=N] [--refresh] <repo>...
```

### Behavior

- Target directory: `/tmp/lex-fmt/<repo>`. Created if missing.
- Default: full clone (preserves `git log`/`blame` for planning tasks).
- `--depth=N`: shallow clone via `git clone --depth=N`. Use `--depth=1` for fast read-only grep tasks.
- `--all`: clone every repo in the canonical list (see below). Mutually exclusive with positional args.
- `--with-setup`: after clone, run `scripts/setup-dev-env.sh` in each clone if it exists. **Opt-in only** — most tasks don't need full toolchain bootstrap.
- `--refresh`: if the target dir exists, `git fetch && git reset --hard origin/<default-branch>` instead of skipping. Default behavior is skip-if-exists (fast no-op on re-run).
- Auth: relies on `GH_TOKEN` already exported in the session. No flag needed.

### Canonical repo list

Hard-coded in the script (one source of truth):

```text
lex comms mkdocs-lex vscode lexed nvim tree-sitter-lex zed-lex
```

When a new sibling repo joins the org, this list is updated and the script is re-published via the env-setup PR. No per-consumer-repo update needed.

### Output contract

The script prints a tab-separated summary on stdout that agents can parse:

```text
repo            path                            status
comms           /tmp/lex-fmt/comms              cloned
vscode          /tmp/lex-fmt/vscode             skipped-exists
lexed           /tmp/lex-fmt/lexed              refreshed
```

Stderr gets human-readable progress; stdout is machine-friendly. Exit code 0 if all requested repos succeeded, nonzero (with which repos failed listed on stderr) otherwise.

### Edge cases

- Unknown repo name → fail fast with the canonical list printed.
- `--with-setup` on a repo with no `scripts/setup-dev-env.sh` → log skipped, continue (don't fail).
- `--with-setup` failure → record in summary as `setup-failed`, exit nonzero but leave the clone in place (partial usefulness > nothing).
- Disk pressure: `lexed` is ~46M unsetup, much larger with `node_modules`. Script does not preemptively check disk; we accept that `--with-setup --all` is the heavy case agents only run when they mean it.
- Concurrent runs: per-repo lock file at `/tmp/lex-fmt/.<repo>.lock` (one lock per repo, not a single global lock) so two parallel agent invocations clone different repos in parallel but never the same one twice. Lock is `flock`-based on file descriptor — released automatically when the script exits, including on crash, so no stale-lock cleanup is needed. Probably overkill for the actual use case but cheap.

### Where it lives

`arthur-debert/release env/setup.sh` installs it to `/usr/local/bin/clone-lex-repos`. Source kept in `arthur-debert/release scripts/` next to other env-level tools.

**Rejected alternative**: putting the script in each repo's `scripts/` and copying it across via the gh-repo-setup policy sweep. Eight copies to keep in sync, and only useful from rooted sessions where that repo happens to be the root. Env-level wins.

---

## The skill: `lex-multirepo`

### Location

`~/.claude/skills/lex-multirepo/SKILL.md` — user-level skill, available across all cloud sessions.

### Trigger description

Skill description (the bit the harness uses to decide when to invoke):

> Bootstrap sibling lex-fmt repos for multi-repo tasks. Use when:
> (1) The task explicitly names a sibling repo by name (`comms`, `vscode`, `lexed`, `nvim`, `tree-sitter-lex`, `zed-lex`, `mkdocs-lex`) and the current rooted repo is different.
> (2) Task is a planning / proposal / cross-repo analysis spanning ≥2 lex-fmt repos.
> (3) Task requires reading release-cascade config or shared specs that live in `comms`.
>
> Do NOT trigger on: tasks scoped entirely to the rooted repo, monorepo-internal work inside `lex/crates/`, or generic "multi-file" keyword matches.

### Skill body (what it tells the agent)

1. Identify which sibling repos the task needs. Default to a minimal subset. Only run `--all` when the task is genuinely cross-cutting (release-cascade audit, org-wide policy sweep, etc.).
2. Run `clone-lex-repos <names>` (omit `--with-setup` unless the task needs to build/test; most planning tasks only need to read source).
3. Read from `/tmp/lex-fmt/<repo>/` — grep, cat, etc. Don't re-clone.
4. For GitHub operations on those repos (PR reads, issue creates), use `gh` against `lex-fmt/<repo>` — the MCP server is still scoped to the rooted repo and will deny cross-repo calls.
5. When done, do not clean up `/tmp/lex-fmt/`. Subsequent steps in the same session benefit from the cache. (Cloud containers are ephemeral, so there is no cross-session reuse — `/tmp/lex-fmt/` is a within-session cache only.)

### Fallback

If `clone-lex-repos` is not on `$PATH` (env-setup hasn't propagated yet to the running session), the skill instructs the agent to fall back to a manual `gh repo clone` loop into `/tmp/lex-fmt/`. Same end state, just less ergonomic.

---

## Where each piece lives

| Piece | Repo | Path | Distribution |
| --- | --- | --- | --- |
| `clone-lex-repos` source | `arthur-debert/release` | `scripts/clone-lex-repos` | versioned in release repo |
| Install step | `arthur-debert/release` | `env/setup.sh` | runs on session start |
| Canonical-list update protocol | `arthur-debert/release` | (commit to scripts/) | PR + redeploy env |
| Skill | (user) | `~/.claude/skills/lex-multirepo/SKILL.md` | user-level, syncs via existing mechanism |
| Doc reference | (user) | `~/.claude/CLAUDE.md` | one paragraph pointing at the skill + standard location |

Nothing lives in the per-stack lex-fmt repos themselves. This is intentional — the bootstrap is a session-environment concern, not a repo concern.

---

## Cost & tradeoff summary

| Concern | Mitigation |
| --- | --- |
| Full clones can be heavy (lexed 46M, lex 9M) | Default to full; expose `--depth=1` flag for fast read-only |
| `--with-setup` runs 8 different toolchain bootstraps | Opt-in only; agents must add the flag when they need it |
| Stale clones across long-running sessions | `--refresh` flag; skip-if-exists is the default but documented |
| Canonical list drift when new repos join | Single source of truth in `arthur-debert/release scripts/`; PR + redeploy |
| Disk pressure in long sessions | Out of scope for v1; revisit if it actually becomes an issue |

---

## Open questions for review

1. **Location root**: `/tmp/lex-fmt/` (proposed) vs `~/.cache/lex-fmt/` vs `~/lex-fmt/` vs `/workspace/lex-fmt/`? `/tmp` matches existing user-level conventions and survives within a session. Cloud containers are ephemeral, so neither `/tmp` nor `~/.cache` survives container death — both options give the same lifetime. `~/.cache/lex-fmt/` is more XDG-conventional and signals "cache" intent more clearly; `/tmp/lex-fmt/` matches what's already in `~/.claude/CLAUDE.md` as the scratch convention. Tie-breaker is the user's call.
2. **Setup-script semantics under `--with-setup`**: should it pass through env vars (e.g., `SKIP_HEAVY_DEPS=1`) to siblings, or run each setup with its repo's default behavior? Proposal: default behavior, no special env. Sibling setup scripts can opt-in to skip-flags later.
3. **Should the skill always run, or be agent-invoked?** Proposal: agent-invoked (the harness matches the skill description against the task). Auto-running on every session is wasteful — most tasks don't need siblings.
4. **Naming**: `clone-lex-repos` is descriptive but verbose. Alternatives: `lex-clone`, `lex-siblings`. Going with `clone-lex-repos` for greppability and zero ambiguity.
5. **Should `--with-setup` be parallel?** Eight `setup-dev-env.sh` runs serially could be slow. Proposal: serial for v1, parallel via `xargs -P` if it becomes a pain point.
6. **Canonical repo list — hardcoded vs data file vs dynamic?** Three options:
   (a) Hardcoded in the script (proposed): one PR to update when a new repo joins. Simple, explicit, but coupled to env-setup redeploy cadence.
   (b) Data file in `comms` (e.g., `comms/lex-stack-repos.txt`): list is decoupled from the script, updated independently. Script does `gh api repos/lex-fmt/comms/contents/lex-stack-repos.txt` at startup. Adds one network hop per invocation.
   (c) Dynamic enumeration via `gh api orgs/lex-fmt/repos`: zero maintenance but sweeps in archived repos, internal tooling repos, and anything else in the org that isn't part of the canonical stack. Would need a filter convention (topic? naming?) that doesn't currently exist.
   Proposal: hardcoded for v1 (8 repos, low churn), revisit if the list crosses ~15 entries or churn becomes noticeable.

---

## Implementation order (if approved)

1. PR to `arthur-debert/release`: add `scripts/clone-lex-repos`, install line in `env/setup.sh`. (~1 hour, mostly testing edge cases.)
2. PR to user-level `~/.claude/`: add `skills/lex-multirepo/SKILL.md`, one paragraph in `CLAUDE.md` pointing at it. (~30 min.)
3. Trigger one new cloud session, verify `clone-lex-repos --all --depth=1` lands eight clones in `/tmp/lex-fmt/` and the skill fires on a synthetic multi-repo task prompt.
4. Update this doc with verification notes, close.

Total est. effort: half a day, mostly waiting on env-setup propagation between PRs.
