# Valvet library ecosystem roadmap

Status: implementation started. The planning documents remain the design
record; completed work and evidence are tracked in
[09_completion_status.md](09_completion_status.md). This directory does not
contain ported code or external oracle data.

This is the master index for the macro-level library and package plan derived
from the ten Julia-to-Valvet ports, the cross-port duplication study, the
generated-C and machine-code audit, and the existing compiler and gap reports.

## Documents

| Document | Purpose |
| --- | --- |
| [00_executive_vision.md](00_executive_vision.md) | Assessment, target state, key decisions, expected results |
| [01_ecosystem_architecture.md](01_ecosystem_architecture.md) | Layered architecture, module boundaries, dependency rules, APIs |
| [02_library_opportunities.md](02_library_opportunities.md) | Detailed library proposals, evidence from all ten ports |
| [03_performance_and_machine_code.md](03_performance_and_machine_code.md) | Machine-code constraints, measured findings, benchmark gates |
| [04_developer_experience.md](04_developer_experience.md) | Friction inventory, before/after code, tooling and documentation |
| [05_implementation_roadmap.md](05_implementation_roadmap.md) | Phases, milestones, critical path, parallel workstreams |
| [06_actionable_task_list.md](06_actionable_task_list.md) | Executable task backlog with acceptance and validation |
| [07_impact_estimates.md](07_impact_estimates.md) | Measured, estimated, and unverified impact |
| [08_open_questions_and_research.md](08_open_questions_and_research.md) | Open decisions, assumptions, external references |
| [09_completion_status.md](09_completion_status.md) | Per-task status, files, tests, issues, and evidence |

## Evidence shorthand

The planning work used repository evidence first. The port working artifacts
were under `/tmp/opencode/ports` when this plan was written. They are not part
of the repository, so durable claims in these documents also name the relevant
repository file and symbol.

| Shorthand | Evidence |
| --- | --- |
| `XRD` | `/tmp/opencode/ports/cross_port_library_opportunities.md`, cross-port duplication and library gap report |
| `AUD` | `/tmp/opencode/ports/OPTIMIZATION_AUDIT.md`, generated C and machine-code audit |
| `GAPS` | `/tmp/opencode/ports/ECOSYSTEM_GAPS.md`, Julia feature gap log for ports 4 to 10 |
| `CFB` | `/tmp/opencode/ports/COMPILER_FIX_BATCH.md`, uncommitted compiler batch |
| `PORT-01` to `PORT-10` | `/tmp/opencode/ports/<port>/REPORT.md`; ports 01 to 04 retain reports only |
| `CODE` | Files under `/home/kenneth/valvet`, especially `stdlib/`, `crates/valvet_core/src/`, and `valvet-tests/` |

Evidence quality is not uniform:

- Ports 07, 08, 09, and 10 retain complete Valvet sources and oracles.
- Ports 05 and 06 retain partial Valvet sources; their full line counts and
  helper sizes come from their reports.
- Ports 01, 02, 03, and 04 retain reports only. Claims about their exact
  helper bodies are marked unverified.
- Ports 01 to 03 Julia sources were fetched from `main` or `master`; exact
  commit pins were not recorded. Their reported line counts match the source
  used, but this is weaker provenance than ports 04 to 10, which have MD5s.
- Performance numbers are from the audit host and toolchain. They are
  directional, not universal constants.

## How to use this plan

1. Read `00_executive_vision.md` for the problem statement and the decisions
   that constrain everything else.
2. Use `01_ecosystem_architecture.md` and `02_library_opportunities.md` to
   settle API shape before writing code.
3. Use `03_performance_and_machine_code.md` as the performance contract for
   every library change.
4. Execute `05_implementation_roadmap.md` in phase order. Pick tasks from
   `06_actionable_task_list.md`; do not reorder a task ahead of its listed
   dependencies without updating the plan.
5. Use `07_impact_estimates.md` for targets, not promises. Every estimate is
   labeled measured, estimated, or unverified.
6. Resolve or explicitly defer items in `08_open_questions_and_research.md`
   before turning a P2 idea into implementation work.

## Planning rules

- No source change is authorized by these documents alone.
- Each implementation batch still needs the normal gates: `cargo fmt
  --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `valvet test valvet-tests` in debug and release,
  ASan/UBSan/LSan on generated C, zero `<error>` markers, and numerical
  oracles where arithmetic changes.
- Ports and their oracles stay outside the repository. Migration of a port is
  evidence, not a reason to commit ported code.
- Do not use `-ffast-math`, approximate transcendental functions, reassociated
  arithmetic, or default `-march=native` for bit-exact work.
