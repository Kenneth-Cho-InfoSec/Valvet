# Valvet Preliminary Security Assessment and Penetration Test Plan

Status: preliminary, evidence-based, local-only assessment; pre-fix baseline, remediation recorded in section 17
Date: 2026-10-25
Checkout: `/home/kenneth/valvet`
Git HEAD: `d90e067` plus uncommitted working-tree changes
Assessor: OpenCode security review session

## 0. Rules of engagement and evidence labels

This assessment was performed only against the local Valvet checkout and locally generated binaries. No third-party system, production service, credential store, or shared infrastructure was targeted. Destructive commands were avoided. Tests that needed side effects used marker files under `/tmp/opencode`, `timeout`, and resource limits.

Every claim below carries one of these labels:

| Label | Meaning |
|---|---|
| Confirmed | Reproduced by executing the current local build. The exact repro is recorded. |
| Code-confirmed | Verified by reading the current source, but not executed as an exploit. |
| Candidate | Plausible from source review, but not yet proven against Valvet. Treat as a test target, not a vulnerability. |
| Planned | Proposed test or control, not yet executed. |
| Assumption | A trust-model or deployment assumption that the maintainers must confirm. |

I did not fabricate vulnerabilities. In particular, I did not confirm command execution through a malicious `valvet.toml` git URL, and I did not demonstrate C code injection through the string-literal escaping bugs. Those remain candidates or compile-failure findings, as stated.

## 1. Executive summary

Valvet is a compiled language, not an interpreter. The current implementation is a Rust front end that lexes, parses, type-checks, and borrow-checks Valvet source, lowers checked HIR to a single C11 translation unit, and invokes a C compiler and linker to produce a native executable. The runtime is a small embedded C prelude with manual ownership and deterministic frees. There is no garbage collector, no user FFI, no networking standard library, no build-script mechanism, and no public package registry in the current tree.

The most important security conclusion at the time of this assessment was that Valvet had strong language-level memory-safety goals, but no operating-system sandbox. `--hermetic` was a compile-time mode gate that blocked subprocesses and environment reads. It did not block file reads or writes, and it did not prevent a project-local `std/` tree from shadowing the standard library. Untrusted Valvet source must therefore be treated as equivalent to untrusted native code unless it is run inside an OS-level sandbox such as a container, namespace, seccomp, or Landlock policy. The working tree now denies file I/O and cwd under `--hermetic` and pins `std::` imports, but `--hermetic` is still a policy gate, not an OS sandbox.

Confirmed preliminary findings (pre-fix baseline):

1. `--target` command injection in the CLI. A crafted target triple executes arbitrary shell commands with the privileges of the user running `valvet`. Confirmed with a marker file.
2. Unchecked matrix dimension multiplication in generated C. `mat_zeros(4294967296, 4294967296)` followed by an element access segfaults. This is a denial-of-service and a potential memory-safety defect in the matrix runtime.
3. Compiler stack exhaustion from deeply nested input. 100,000 nested parentheses abort the compiler with `thread 'main' has overflowed its stack`.
4. At assessment time, `--hermetic` did not block file I/O. `read_file("/etc/hostname")` and `write_file("/tmp/...")` both succeeded under `valvet run --hermetic`. It did block `sys_run`, `sys_capture`, and `sys_getenv`. Fixed in section 17.
5. Project-local standard-library shadowing. A local `std/math.valvet` could replace a `std::math` import, including under `--hermetic`. Fixed in section 17.
6. C trigraph corruption in string literals. A valid Valvet string containing `??/` breaks the generated C string and fails the C compiler. C code injection was not demonstrated.
7. Embedded-NUL and C hex-escape corruption. `"a\0b"` is miscompiled to length 2 and the wrong bytes because the generated C uses `\x00b`, which C treats as one greedy hex escape.

Priority order for remediation is: CLI injection, matrix arithmetic, parser depth limits, hermetic trust-model documentation or enforcement, stdlib provenance, then codegen escaping and supply-chain hardening. In parallel, add sanitizer and fuzz jobs to CI, because the current CI does not run either.

## 2. Scope, method, and limitations

### 2.1 Scope

In scope:

- `crates/valvet_core/src/lexer.rs`
- `crates/valvet_core/src/parser.rs`
- `crates/valvet_core/src/ast.rs`
- `crates/valvet_core/src/sema.rs`
- `crates/valvet_core/src/ownership.rs`
- `crates/valvet_core/src/codegen.rs`
- `crates/valvet_core/src/resolver.rs`
- `crates/valvet_core/src/manifest.rs`
- `crates/valvet_core/src/pipeline.rs`
- `crates/valvet_core/src/sha256.rs`
- `crates/valvet_cli/src/main.rs`
- `stdlib/*.valvet`
- `crates/valvet_core/tests/fuzz_soup.rs`
- `.github/workflows/ci.yml`
- `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`
- Generated C and native binaries produced by the local build

Out of scope for this preliminary pass:

- Third-party systems or remote services
- Formal verification of the language semantics
- Full external penetration test
- Windows, macOS, 32-bit, or non-x86_64 platforms
- A complete audit of every standard-library algorithm
- Long-running fuzzing and full sanitizer campaigns
- Release-artifact signing and update-channel audit, which do not appear to exist yet

### 2.2 Method

1. Map the implementation from source, README, architecture notes, and tests.
2. Identify trust boundaries and attacker-controlled inputs.
3. Perform targeted, safe repros for high-value hypotheses.
4. Record exact commands, outputs, root causes, and affected source lines.
5. Separate confirmed findings from candidates and planned tests.
6. Build a risk-ranked test plan, fuzzing strategy, lab design, remediation playbook, and 30/60/90-day roadmap.

### 2.3 Limitations

This is a preliminary assessment. It is not proof that the remaining code is secure. The absence of a finding in a subsystem means that subsystem has not yet been tested deeply, not that it is safe. The current fuzz harness is small and bounded, and CI does not run sanitizers or continuous fuzzing. The compiler and runtime have many numeric, string, CSV, JSON, matrix, and threading paths that still need systematic fuzzing.

## 3. Confirmed architecture

### 3.1 Execution model

Confirmed from `docs/implementation/ARCHITECTURE.md` and the source tree:

```text
.valvet source
  -> lexer
  -> tokens
  -> recursive-descent parser
  -> AST with spans
  -> sema: module resolution, symbols, types, HIR
  -> ownership and borrow checking
  -> checked HIR
  -> C11 emitter
  -> external C compiler and linker
  -> native executable
```

Valvet is not an interpreter. `valvet run` builds a native executable and then executes it. `valvet check` stops before code generation. There is one C translation unit per program. The backend is a handwritten C11 emitter plus a user-local or PATH-provided C compiler. `cc_flags` uses `-std=c11 -O2 -fwrapv -pthread -fno-math-errno -ffp-contract=off` for release, and `-O0 -g` for the dev profile (`crates/valvet_core/src/codegen.rs:7874-7895`).

### 3.2 Components and data flow

| Area | Confirmed implementation | Evidence |
|---|---|---|
| Lexer | Byte-oriented lexer with spans, string interpolation, triple-quoted strings, numeric suffixes, nested block comments | `crates/valvet_core/src/lexer.rs` |
| Parser | Recursive-descent parser producing AST with source spans | `crates/valvet_core/src/parser.rs` |
| Type checker | Sema builds typed HIR, checks modules, types, modes, features, builtins | `crates/valvet_core/src/sema.rs` |
| Ownership | Move checking, borrow checking, scoped frees, use-after-move diagnostics | `crates/valvet_core/src/ownership.rs` |
| Codegen | HIR to C11, embedded runtime prelude, C compiler driver | `crates/valvet_core/src/codegen.rs` |
| Runtime | Manual memory, `VStr`, `Vec`, `Mat`, `ArrayD`, `Array<T,N>`, `StringIndex`, CSV/JSON, RNG, threading helpers | `crates/valvet_core/src/codegen.rs` |
| Modules | File-based, `::` separators, entry-directory root, dependency roots, stdlib roots | `crates/valvet_core/src/resolver.rs` |
| Manifest | `valvet.toml` with `[package]`, `[dependencies]`, `[profile]`, `[features]` | `crates/valvet_core/src/manifest.rs` |
| Lockfile | `valvet.lock` with source hashes and dependency versions; `--frozen` verifies | `crates/valvet_cli/src/main.rs:765-883` |
| CLI | `check`, `build`, `run`, `test`, `new`, `fmt`, `fix`, `tree`, `metadata`, `verify`, `package`, `install`, `vendor`, `update`, `repl`, `-e` | `crates/valvet_cli/src/main.rs` |
| Tests | Unit tests, front-end tests, fixture suite, codegen tests, doc tests, stable fuzz harness | `crates/valvet_core/tests`, `valvet-tests`, `docs/implementation/TEST_STRATEGY.md` |
| CI | fmt, clippy, build, unit tests, fixture suite, release build and fixture suite | `.github/workflows/ci.yml` |

### 3.3 Capability and trust surfaces

| Surface | Current behavior | Security relevance |
|---|---|---|
| File read | `read_file(path): Result<String, String>` builtin, implemented with `fopen`/`fseek`/`ftell`/`fread` | Arbitrary read as the invoking user; no sandbox |
| File write | `write_file(path, content): Result<Int64, String>` builtin, implemented with `fopen(..., "wb")` | Arbitrary write or truncation as the invoking user; no sandbox |
| Subprocess | `std::sys::run` uses C `system`; `std::sys::capture` uses `popen` | Shell command execution in imp mode; blocked in exp/hermetic mode |
| Environment | `std::sys::getenv` uses C `getenv`; blocked under `--hermetic` | Secret leakage if untrusted source runs without hermetic |
| Working directory | `std::sys::cwd` uses `getcwd` | Path disclosure; not blocked under `--hermetic` |
| Script arguments | `args` exposes argv | Input channel and possible secret leakage if callers pass secrets |
| Networking | No network standard library found | No direct language-level network attack surface in the current tree |
| FFI | `unsafe` and `extern` are rejected by the parser | No user FFI in the current language |
| Build scripts | No build-script mechanism found in the manifest | No dependency build-time execution path in the current tree |
| Public registry | No registry or publish command found | Git and path dependencies are the current supply-chain surface |
| C toolchain | `VALVET_CC`, `~/.local/vgcc`, then `gcc`/`cc` on PATH | Toolchain compromise is code execution in the build |
| Stdlib roots | `VALVET_STDLIB`, install layout, then source checkout | Environment or local-tree shadowing can change trusted imports |
| Git dependencies | `git clone`, `git fetch`, `git checkout`; cache under `~/.cache/valvet/git` | Supply-chain and cache-poisoning surface |

### 3.4 Existing security controls

- Rust memory safety in the front end. The workspace has no third-party Rust dependencies (`Cargo.lock` contains only `valvet_cli` and `valvet_core`).
- Ownership and borrow checking in Valvet source.
- Bounds checks in generated C for `Vec`, `Mat`, and `ArrayD` indexing, with a shared trap.
- Deterministic frees and no garbage collector.
- `mode exp` blocks subprocesses and top-level script statements.
- `--hermetic` forces strict entry, blocks `sys_run`, `sys_capture`, and `sys_getenv`.
- `--offline` avoids network dependency resolution when caches are warm.
- `valvet.lock` and `--frozen` provide source and dependency-version verification.
- `valvet test` runs a large fixture suite in debug and release profiles.
- A stable no-panic fuzz harness exists for the front end (`crates/valvet_core/tests/fuzz_soup.rs`).

### 3.5 What is not present

Code-confirmed by source and search:

- No user-callable FFI. `unsafe` and `extern` declarations are rejected with `E0800` (`crates/valvet_core/src/parser.rs:366-377`).
- No networking standard library. The only `http` string found under `stdlib/` is the SVG XML namespace in `stdlib/plot.valvet`.
- No dependency build scripts. The manifest parser accepts only `package`, `dependencies`, `profile`, and `features` sections (`crates/valvet_core/src/manifest.rs:98-168`).
- No public registry, publish command, or package signature verification.
- No OS sandbox, capability manifest, seccomp profile, namespace setup, or Landlock integration in the CLI.
- No sanitizer job and no continuous fuzzing job in CI.

## 4. Threat model

### 4.1 Assets

| Asset | Why it matters |
|---|---|
| Compiler and CLI integrity | A compromised compiler can inject code into every binary it builds |
| Compiler availability | A build service or developer workstation can be denied service by a crash or hang |
| Generated native binaries | They run with the invoking user's privileges and may handle sensitive data |
| User source and project files | Source code, data, and configuration may be confidential or integrity-sensitive |
| Host filesystem | File I/O builtins can read, write, truncate, or overwrite files as the user |
| Host process environment | Environment variables may contain tokens, credentials, or proxy configuration |
| Standard library source | Trusted imports are part of the trusted computing base |
| Dependency cache and lockfile | Cache poisoning or lockfile drift can substitute code |
| Build toolchain | C compiler, linker, libc, `git`, `sh`, and PATH are all code execution inputs |
| CI secrets and release artifacts | CI compromise can alter releases or exfiltrate credentials |
| User trust in diagnostics | Misleading diagnostics can hide malicious behavior or cause unsafe fixes |

### 4.2 Trust boundaries

| Boundary | Trusted side | Untrusted side | Current enforcement |
|---|---|---|---|
| B1: Source to compiler | Compiler binary | Valvet source, imports, source tree | Rust memory safety, parser/sema checks, fuzz harness; no OS isolation |
| B2: Program to host | Host OS | Generated native program | None beyond user privileges; `--hermetic` is compile-time only |
| B3: Manifest to build | Build environment | `valvet.toml`, path and git dependencies | Lockfile, optional `--frozen`, git cache; no signatures |
| B4: CLI arguments to compiler | Compiler process | `--target`, paths, flags | Incomplete; `--target` is passed to `sh -c` |
| B5: Project tree to stdlib | Installed stdlib | Local `std/` directory | None; local tree wins |
| B6: Environment to build | Trusted environment | `VALVET_CC`, `VALVET_STDLIB`, `PATH`, `HOME`, `LD_LIBRARY_PATH` | Environment is implicitly trusted |
| B7: Generated C to C compiler | Trusted C compiler | Generated C text | String escaping, C syntax; trigraph and NUL defects found |
| B8: Dependency network to cache | Local cache | Git remote, tags, URLs | Git and TLS trust; no signature or content pin beyond lockfile |
| B9: CI to repository | Repository | GitHub Actions, toolchain downloads, apt packages | Standard GitHub controls; actions not pinned by SHA |
| B10: Test harness to host | Test runner | Fixture programs and generated binaries | None; tests run with user privileges |

### 4.3 Attacker profiles and capabilities

| Actor | Capabilities | Likely goals |
|---|---|---|
| Untrusted source author | Supplies `.valvet` files, imports, huge or deeply nested input, string literals, matrix dimensions, data files | Compiler crash, build-service DoS, runtime crash, file access, command execution, policy bypass |
| Malicious dependency author | Supplies a path or git dependency, manifest, source files, symlinks, tags | Code substitution, cache poisoning, data exfiltration, build-time or runtime execution |
| Build-service operator or tenant | Controls source, manifest, some CLI arguments, possibly environment | Escape tenant isolation, read other tenants' files, exhaust shared resources |
| Local attacker | Can influence CLI arguments, environment, PATH, cache, or files | Code execution as the developer, credential theft, persistence |
| Compromised CI dependency | Controls an action, toolchain download, or package | Repository compromise, secret exfiltration, release tampering |
| Malicious data provider | Supplies CSV, JSON, plot data, or numeric dimensions to a Valvet program | Runtime crash, memory corruption, denial of service, misleading output |
| Insider | Has legitimate repository or CI access | Sabotage, data theft, hidden backdoor |

### 4.4 Threat scenarios and attack surfaces

| ID | Threat scenario | Primary path | Impact | Existing controls | Status |
|---|---|---|---|---|---|
| T1 | Malicious source causes compiler memory corruption | lexer, parser, sema, codegen | Compiler RCE or crash | Rust memory safety, no `unsafe` in front end, fuzz harness | Partially tested; no confirmed memory corruption yet |
| T2 | Malicious source exhausts compiler stack or memory | deep nesting, huge files, recursive types | Build-service DoS | None; fuzz harness caps input size | Confirmed stack overflow |
| T3 | Malicious data or dimensions cause runtime memory corruption | `Mat`, `Vec`, `ArrayD`, CSV, JSON, strings | Native RCE or crash | Bounds checks, ownership, manual tests | Confirmed matrix overflow crash |
| T4 | Untrusted source executes shell commands | `std::sys`, prelude, `system`/`popen` | Host compromise | `mode exp` and `--hermetic` compile-time gates | Design risk; not a sandbox |
| T5 | Untrusted source reads or writes host files | `read_file`, `write_file`, stdlib file helpers | Data theft, data destruction | None | Confirmed under `--hermetic` |
| T6 | CLI argument injects shell command | `--target` to `sh -c` | Code execution as user | None | Confirmed |
| T7 | Malicious manifest or git URL executes code | `valvet.toml`, `git clone`, cache | Code execution, cache poisoning | Lockfile, optional `--frozen`, no signatures | Candidate; option injection not confirmed through Valvet |
| T8 | Local `std/` shadows trusted stdlib | resolver entry-root lookup | Trusted code uses attacker code | None | Confirmed behavior |
| T9 | Build environment is malicious | `VALVET_CC`, PATH, `LD_LIBRARY_PATH`, `VALVET_STDLIB` | Code execution, wrong output | Environment is trusted | Design risk |
| T10 | CI supply chain is compromised | GitHub Actions, apt, toolchain | Repo or release compromise | Standard GitHub controls | Process risk |
| T11 | Secrets leak through environment or argv | `sys_getenv`, `args`, `cwd` | Credential disclosure | Hermetic blocks `sys_getenv`; not `args`/`cwd` | Partial |
| T12 | Resource exhaustion | allocations, loops, threads, file sizes | Denial of service | None | Partially confirmed |
| T13 | Generated C is corrupted by source text | string literals, identifiers, codegen | Compile failure, potential policy bypass | C escaping | Trigraph and NUL confirmed; injection not demonstrated |
| T14 | Dependency cache is poisoned | `~/.cache/valvet/git` | Arbitrary dependency code | None beyond local file permissions | Candidate |
| T15 | Release artifact is substituted | `valvet install`, distribution | Arbitrary code on user machines | No signature or update channel found | Candidate |

### 4.5 Assumptions

These assumptions are not security guarantees. If any is false, the risk ranking changes.

- A1: The Valvet compiler, C compiler, linker, libc, `git`, and `sh` are trusted code.
- A2: `stdlib/` and the installed standard library are trusted and not writable by the attacker.
- A3: `valvet.toml`, path dependencies, and git dependencies are trusted by default.
- A4: `--hermetic` is a reproducibility and subprocess-restriction flag, not an OS sandbox.
- A5: Generated binaries run with the invoking user's privileges, without setuid or extra capabilities.
- A6: The current deployment is a local Linux x86_64 CLI with GCC and glibc.
- A7: Untrusted Valvet source is out of scope unless the operator provides an external OS sandbox.
- A8: The attacker cannot write to the compiler binary, stdlib, dependency cache, or environment unless the scenario says so.

If A2, A3, or A7 are false, the severity of T5, T7, T8, and T9 rises sharply. If untrusted source is a supported use case, the current implementation does not provide a security boundary.

### 4.6 Security objectives

- Integrity: only trusted code and trusted dependencies influence the generated binary.
- Confidentiality: untrusted code cannot read secrets or unrelated files.
- Availability: malformed input cannot crash or hang the compiler or runtime beyond configured resource limits.
- Isolation: untrusted code runs with explicit, minimal capabilities.
- Reproducibility: builds can be verified against a lockfile and trusted toolchain.
- Auditability: security-relevant actions, inputs, and versions are recorded.
- Safe defaults: dangerous capabilities are opt-in and clearly documented.

## 5. Architecture information needed

These questions are the ones that materially change the threat model. They do not block the preliminary plan, but the answers should be recorded before the first production use.

| ID | Question | Why it matters |
|---|---|---|
| Q1 | Is Valvet source ever untrusted? Is the compiler used as a multi-tenant service? | Determines whether compiler DoS and runtime memory safety are critical or only developer-quality issues |
| Q2 | Is `--hermetic` intended as a security boundary or only a reproducibility and subprocess flag? | Determines whether the confirmed file I/O behavior is a vulnerability or a documentation gap |
| Q3 | Is the standard library trusted? Is `VALVET_STDLIB` allowed in production? Should local `std/` shadowing be allowed? | Determines whether stdlib shadowing is a supply-chain issue |
| Q4 | Are `valvet.toml` and git dependencies trusted? Are git dependencies allowed in CI? Is `rev` pinning mandatory? Is `--frozen` mandatory? | Determines the supply-chain attack surface |
| Q5 | Are build scripts planned? If yes, what sandbox and permission model will they use? | Build scripts are a common dependency RCE path; none exist now |
| Q6 | Is user FFI planned? If yes, what ABI, capability, and memory-safety model will it use? | FFI would add a large new attack surface |
| Q7 | Is networking planned? If yes, what TLS, CA, DNS, proxy, and redirect model will it use? | Networking would add remote attack surfaces |
| Q8 | What privileges does the generated binary run with? Root, user, container, setuid, capabilities? | Determines the impact of runtime memory-safety bugs |
| Q9 | Is an OS sandbox planned: seccomp, Landlock, namespaces, cgroups, containers, capability manifests? | The only credible isolation for untrusted code |
| Q10 | Who controls CLI arguments, environment, manifest, and source in production? | Determines whether `--target`, `VALVET_CC`, and PATH are attacker-controlled |
| Q11 | Which platforms are supported: Linux x86_64 only, 32-bit, Windows, macOS, big-endian? | Changes `long`, `size_t`, alignment, and ABI assumptions |
| Q12 | Is `VALVET_CC` trusted? Should the compiler verify the C compiler version or hash? | The C compiler is code execution in the build |
| Q13 | Is there a signed release or update channel? How are binaries distributed? | Prevents release substitution and update attacks |
| Q14 | Are CSV, JSON, plot, and other data inputs untrusted? What size and complexity limits are required? | Determines runtime parser and allocation threat model |
| Q15 | Who is the security contact? Is there a disclosure policy, CVE process, and response SLA? | Required for responsible handling of findings |
| Q16 | What compatibility guarantees apply to security fixes? Can `--hermetic` be tightened? Can local `std/` be rejected? | Determines remediation options |
| Q17 | Are the confirmed findings already known or fixed on another branch? | Avoids duplicate work and stale risk ranking |

## 6. Confirmed preliminary findings

### 6.1 VLV-001: `--target` shell command injection

Status: Confirmed
Severity: High for a local CLI, Critical if `--target` is reachable from untrusted remote input
Affected file: `crates/valvet_cli/src/main.rs:311-340`

Root cause:

`apply_target` builds a driver name from the user-supplied target triple and then interpolates it into a shell command:

```rust
let driver = format!("{}-gcc", triple);
let found = std::process::Command::new("sh")
    .arg("-c")
    .arg(format!("command -v {}", driver))
    .output()
```

Any shell metacharacters in `triple` are interpreted by `sh`. The command runs before the driver is used, so the attacker does not need a valid compiler.

Minimal repro:

```sh
rm -f /tmp/opencode/valvet_target_injection
target/debug/valvet --target 'x; touch /tmp/opencode/valvet_target_injection #' \
  check examples/hello.valvet
ls -l /tmp/opencode/valvet_target_injection
```

Observed result:

```text
ok: examples/hello.valvet is well-formed
-rw-r--r-- 1 kenneth kenneth 0 ... /tmp/opencode/valvet_target_injection
```

The compiler exited 0 and the marker file was created. This is arbitrary command execution with the privileges of the user running `valvet`.

Impact:

- A developer who pastes or scripts an untrusted target triple can be compromised.
- A build service that forwards a user-supplied target triple to `valvet` can be compromised, including access to CI secrets and workspace files.
- The injection occurs for commands that accept `--target`, including `check`, not only `build`.

Recommended fix:

1. Do not invoke a shell. Validate the triple and use `Command::new(driver)` directly.
2. Allow only a conservative character set, for example ASCII alphanumerics, `_`, `-`, and `.`. Reject `/`, whitespace, `;`, `|`, `&`, `$`, backticks, quotes, parentheses, redirections, and newlines.
3. Prefer searching `PATH` in Rust with `std::env::split_paths` instead of `command -v`.
4. If a shell must be used, pass the driver as a positional argument, never as part of the command string. For example, use `sh -c 'command -v -- "$1"' sh "$driver"`, but direct process spawning is safer.
5. Add a regression test that uses a marker file and asserts it is not created.

Compatibility impact:

Valid triples such as `x86_64-linux-gnu` continue to work. Invalid triples now produce a clean diagnostic instead of being passed to a shell. This is a security fix with no legitimate compatibility loss.

Regression test:

```rust
// Pseudocode for a CLI integration test.
let marker = temp_path("target-injection-marker");
let status = Command::new(env!("CARGO_BIN_EXE_valvet"))
    .args(["--target", &format!("x; touch {} #", marker.display()), "check", "examples/hello.valvet"])
    .status();
assert!(!status.success());
assert!(!marker.exists());
```

Verification:

- Run the repro and confirm the marker is absent.
- Run the full CLI test suite.
- Run `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`.

### 6.2 VLV-002: Matrix dimension overflow in generated C

Status: Confirmed
Severity: High for untrusted dimensions or a service that runs Valvet programs; Medium for local trusted programs
Affected code: `crates/valvet_core/src/codegen.rs:3427` and related matrix kernels

Root cause:

`vmat_zeros` computes the element count with signed multiplication and does not check overflow:

```c
static VMat vmat_zeros(int64_t r, int64_t c) {
    if (r < 0 || c < 0) { ... abort(); }
    VMat m = {NULL, r, c};
    int64_t n = r * c;
    if (n > 0) {
        m.data = (double *)calloc((size_t)n, sizeof(double));
        ...
    }
    return m;
}
```

The build uses `-fwrapv`, so signed overflow wraps in a defined but incorrect way. With `r = c = 4294967296`, `r * c` wraps to 0. `m.data` stays `NULL`, but `m.rows` and `m.cols` remain huge. The bounds check for `m[0, 0]` passes because the indices are in range, and the generated code dereferences `NULL`.

The same unchecked pattern appears in other matrix paths, including:

- `v_clone_Mat_`: `int64_t n = v.rows * v.cols;` (`codegen.rs:3423`)
- `vmat_identity`: calls `vmat_zeros(n, n)` and then writes `m.data[i * n + i]` (`codegen.rs:3430`)
- `vmat_det`: `malloc((size_t)(n * n + 1) * sizeof(double))` (`codegen.rs:3475`)
- `vmat_lu_solve`, `vmat_eigen_values`, `vmat_svd_*`: `n * n`, `m * n`, and similar products
- `vmat_kron`: `a->rows * b->rows` and `a->cols * b->cols`

Minimal repro:

```sh
cat > /tmp/opencode/mat_overflow.valvet <<'EOF'
fun main() {
    val m = mat_zeros(4294967296 as Int64, 4294967296 as Int64);
    println(m[0, 0]);
}
EOF
target/debug/valvet build /tmp/opencode/mat_overflow.valvet
timeout 10s target/valvet/mat_overflow
```

Observed result:

```text
built target/valvet/mat_overflow
Segmentation fault (core dumped)
exit 139
```

Note: `valvet run` returns 1 for a signal because `cmd_run` maps `st.code().unwrap_or(1)` (`crates/valvet_cli/src/main.rs:939-940`). Use the built binary or `--` to observe the real signal.

Impact:

- A Valvet program that accepts matrix dimensions from untrusted input can be crashed remotely.
- If a wrapped product becomes a small positive value, a small allocation can be followed by a much larger loop, creating a heap out-of-bounds write. I did not construct that case because the practical values require enormous allocations, but the arithmetic pattern is unsafe and must be treated as a memory-safety defect.
- The same pattern can cause huge allocations and `abort()` on out-of-memory, which is a denial of service.

Recommended fix:

1. Add checked arithmetic helpers, for example `checked_mul_i64(a, b) -> Option<i64>` and `checked_alloc_len(n, elem_size) -> Option<usize>`.
2. Reject dimensions whose product exceeds a configurable element limit, for example `MAX_MAT_ELEMENTS`.
3. Check `r != 0 && c > INT64_MAX / r` before multiplying.
4. Check `n > SIZE_MAX / sizeof(double)` before calling `calloc`.
5. Use a single checked constructor for all matrix shapes. Do not duplicate raw `rows * cols` or `n * n` expressions in kernels.
6. Prefer the checked shape-product logic already described for `ArrayD` in `ARCHITECTURE.md`.
7. Audit every `*` used in an allocation size, loop bound, or index calculation in `codegen.rs`.
8. Consider `-ftrapv` for generated C during tests, but keep explicit checks as the real fix.

Compatibility impact:

Very large matrices that currently trigger undefined behavior or `abort()` will instead produce a clean diagnostic or trap. Legitimate programs within the configured limit are unaffected. The exact limit should be documented.

Regression test:

- Compile-pass or runtime fixture with `mat_zeros(4294967296, 4294967296)` and an element access.
- Expected result: a clean `matrix dimension overflow` diagnostic or trap, not a segmentation fault.
- Add the same test for `mat_identity`, `mat_det`, `mat_transpose`, `mat_kron`, and `mat_from_rows`.
- Run under ASan and UBSan.

Verification:

- Run the built binary and confirm exit code is not 139 and no sanitizer report appears.
- Run the full fixture suite in debug and release.
- Fuzz matrix dimensions with boundary values: 0, 1, 2, `INT32_MAX`, `INT64_MAX`, `2^31`, `2^32`, and values near `sqrt(INT64_MAX)`.

### 6.3 VLV-003: Compiler stack exhaustion from deeply nested input

Status: Confirmed
Severity: High for an untrusted-source build service, Medium for a local developer CLI
Affected component: recursive-descent parser and downstream recursive passes

Root cause:

The parser is recursive descent and has no input-depth or stack budget. Deeply nested parentheses, blocks, types, or expressions consume native stack until the process aborts. The existing fuzz harness deliberately bounds token-soup size to keep recursion within stack limits (`crates/valvet_core/tests/fuzz_soup.rs:84-85`), so it does not cover this class.

Minimal repro:

```sh
python3 - <<'PY'
n = 100000
with open('/tmp/opencode/deep_parens.valvet', 'w') as f:
    f.write('fun main() {\n    val x = ' + '(' * n + '1' + ')' * n + ';\n    println(x);\n}\n')
PY
(ulimit -s 8192; timeout 20s target/debug/valvet check /tmp/opencode/deep_parens.valvet)
```

Observed result:

```text
thread 'main' (7050) has overflowed its stack
fatal runtime error: stack overflow, aborting
Aborted (core dumped)
exit 134
```

Impact:

- A single malicious source file can crash a compiler service worker.
- Repeated submissions can cause a denial of service.
- A stack overflow is not catchable with `catch_unwind`, so the existing no-panic fuzz harness cannot detect it.

Recommended fix:

1. Add an explicit nesting-depth counter to the parser. Increment on expression, block, type, pattern, and statement recursion; decrement on exit.
2. Set a conservative limit, for example 256 or 1024, and emit a normal diagnostic with a source span.
3. Add a token-count and source-size limit, or enforce them at the CLI boundary.
4. Audit sema, ownership, and resolver recursive passes for the same issue. The AST can be deep even if parsing is bounded, so downstream passes need either the same bound or iterative traversal.
5. Add a child-process regression test. Do not rely on `catch_unwind` for stack-overflow tests.
6. Extend the fuzz harness with a deep-nesting generator and a subprocess timeout. A crash should be recorded as a signal, not as a Rust panic.

Compatibility impact:

Programs nested beyond the limit are rejected. This is a deliberate availability tradeoff. The limit should be documented and configurable if needed.

Regression test:

```sh
python3 - <<'PY'
n = 100000
with open('/tmp/opencode/deep_parens.valvet', 'w') as f:
    f.write('fun main() { val x = ' + '(' * n + '1' + ')' * n + '; println(x); }')
PY
(ulimit -s 8192; target/debug/valvet check /tmp/opencode/deep_parens.valvet)
# Expect: error[E....]: nesting too deep, exit 1, no "stack overflow" text.
```

Verification:

- Run with a normal and a reduced stack limit.
- Confirm a clean diagnostic and no core dump.
- Run the full parser test suite and fuzz corpus.

### 6.4 VLV-004: `--hermetic` does not block file I/O

Status: Confirmed
Severity: High if `--hermetic` is presented as a sandbox; Medium if it is only a subprocess and environment restriction
Affected code: `crates/valvet_core/src/sema.rs`, `crates/valvet_cli/src/main.rs:28-31`

Root cause:

`--hermetic` sets `VALVET_HERMETIC=1`. The resolver uses that flag to force exp mode, and sema uses it to deny `sys_getenv`; `sys_run` and `sys_capture` are denied because exp mode is forced. It does not deny `read_file`, `write_file`, or `sys_cwd`. The file builtins have no hermetic check (`sema.rs:6994` and `sema.rs:7783`). The C implementations use arbitrary `fopen` paths (`codegen.rs:267` and `codegen.rs:2631`).

Minimal repro, read:

```sh
cat > /tmp/opencode/hermetic_read.valvet <<'EOF'
fun main() {
    println(match (read_file("/etc/hostname")) {
        Ok(s) -> s,
        Err(e) -> e,
    });
}
EOF
target/debug/valvet run --hermetic /tmp/opencode/hermetic_read.valvet
```

Observed result:

```text
archlinux
exit 0
```

Minimal repro, write:

```sh
cat > /tmp/opencode/hermetic_write.valvet <<'EOF'
fun main() {
    val r = write_file("/tmp/opencode/hermetic_write.txt", "written");
    match (r) {
        Ok(n) -> println(n),
        Err(e) -> println(e),
    }
}
EOF
target/debug/valvet run --hermetic /tmp/opencode/hermetic_write.valvet
cat /tmp/opencode/hermetic_write.txt
```

Observed result:

```text
7
written
```

Control test, subprocess denial:

```sh
cat > /tmp/opencode/hermetic_sys.valvet <<'EOF'
fun main() {
    println(sys_run("id"));
}
EOF
target/debug/valvet run --hermetic /tmp/opencode/hermetic_sys.valvet
```

Observed result:

```text
error[E0401]: `sys_run` needs imp mode (`mode exp` is hermetic)
```

Impact:

- Untrusted code can read arbitrary files readable by the user, including SSH keys, cloud credentials, source code, and environment files.
- Untrusted code can write, truncate, or overwrite arbitrary files writable by the user.
- `--hermetic` does not provide confidentiality or integrity isolation.
- `sys_cwd` also remains available by the same sema path (code-confirmed, not executed in this pass), disclosing the working directory.

Recommended fix:

Choose one of two explicit policies:

Policy A, make `--hermetic` a real security boundary:

- Run the generated binary inside an OS sandbox with a read-only or allowlisted filesystem view.
- Add a path allowlist or virtual filesystem for `read_file` and `write_file`.
- Deny `sys_cwd`, `args`, and any other host metadata by default.
- Add integration tests that attempt to read `/etc/hostname` and write outside the sandbox.

Policy B, keep `--hermetic` as a reproducibility flag:

- Rename it or document it as `--no-subprocess` or `--strict-entry`.
- State clearly in `README.md` and `--help` that it does not restrict filesystem access.
- Require an external sandbox for untrusted code.
- Add a warning when untrusted source is run without an OS sandbox.

Compatibility impact:

Policy A may break scripts that read data files under hermetic mode. Policy B is documentation-only but preserves current behavior. In either case, the help text should not imply sandboxing.

Regression test:

- A test that runs a program attempting to read a known host file under `--hermetic`.
- Expected under Policy A: denial.
- Expected under Policy B: documented success with a warning, or the test asserts the documented behavior.

Verification:

- Run both repros.
- Confirm the chosen policy is reflected in `README.md`, `--help`, and diagnostics.

### 6.5 VLV-005: Project-local `std/` shadows the standard library

Status: Confirmed behavior, security impact depends on trust model
Severity: Medium for namespace integrity; High if a trusted entry is compiled alongside attacker-controlled files
Affected code: `crates/valvet_core/src/resolver.rs:674-695`

Root cause:

For a `std::` import, the resolver first checks `self.root.join(&rel)`, where `self.root` is the entry file's directory. Only if that misses does it search `stdlib_roots`. A local `std/math.valvet` therefore wins over the installed or checkout stdlib.

Minimal repro:

```sh
rm -rf /tmp/opencode/std_shadow
mkdir -p /tmp/opencode/std_shadow/std
cat > /tmp/opencode/std_shadow/std/math.valvet <<'EOF'
module std.math;

pub fun foo(): Int64 {
    return 999
}
EOF
cat > /tmp/opencode/std_shadow/main.valvet <<'EOF'
import std::math::foo;

fun main() {
    println(foo());
}
EOF
cd /tmp/opencode/std_shadow
/home/kenneth/valvet/target/debug/valvet run main.valvet
/home/kenneth/valvet/target/debug/valvet run --hermetic main.valvet
```

Observed result:

```text
999
999
```

The local module is used in both normal and hermetic runs.

Impact:

- A project tree can replace trusted standard-library functions with attacker-controlled Valvet code.
- `--hermetic` does not pin the standard library, so it does not prevent this substitution.
- A trusted `main.valvet` that imports `std::checked`, `std::validate`, or a similar safety helper could silently use a local replacement if the project directory is writable by an attacker.
- A build service that compiles a trusted entry from an untrusted checkout can be affected.

Recommended fix:

1. Resolve `std::` imports only from explicit standard-library roots. Do not consult the entry directory by default.
2. If local shadowing is desired for development, require an explicit flag such as `--local-stdlib` or a manifest opt-in.
3. Record the resolved stdlib root and a hash of each stdlib file in the lockfile or metadata output.
4. Add a `--stdlib` provenance check and reject a local `std/` directory when `--hermetic` is used.
5. Document the resolution order and the trust model.

Compatibility impact:

Projects that intentionally shadow stdlib modules will break unless they opt in. This is a security-positive change but should be released with a clear migration note.

Regression test:

- Create a local `std/math.valvet` with a unique function.
- Import it from `main.valvet`.
- Expected default: resolution error or use of the real stdlib, not the local file.
- Expected with the opt-in flag: local file is used.

Verification:

- Run the repro with and without `--hermetic`.
- Check `valvet tree` or `valvet metadata` reports the stdlib provenance.

### 6.6 VLV-006: C trigraph corruption in string literals

Status: Confirmed compile failure; code injection not demonstrated
Severity: Medium for availability and codegen integrity; potential higher severity if a reliable injection primitive is found
Affected code: `crates/valvet_core/src/codegen.rs:7823-7839` (`c_escape`), string literal emission at `codegen.rs:5565`

Root cause:

`c_escape` escapes `"`, `\`, newline, tab, carriage return, and control characters. It does not escape `?`. GCC with `-std=c11` processes C trigraphs, so the sequence `??/` becomes a backslash. In the generated C string literal, that backslash escapes the closing quote and breaks the literal.

Minimal repro:

```sh
cat > /tmp/opencode/trigraph.valvet <<'EOF'
fun main() {
    println("??/");
}
EOF
target/debug/valvet run /tmp/opencode/trigraph.valvet
```

Observed result:

```text
error: backend linking failed:
C compiler failed:
target/valvet/trigraph.c:730:25: warning: missing terminating " character
  730 | VStr _vt1 = vstr_from_c("??/");
      |                         ^
...
```

Impact:

- A valid Valvet string can make code generation fail.
- A malicious source file can cause a build failure with a confusing C-level diagnostic.
- The trigraph transformation is an unescaped lexical operation in generated C. I did not construct a reliable C code injection, but the escaping is not safe and should be fixed before any sandbox claim relies on generated C being well-formed.

Recommended fix:

1. Escape every `?` as `\?` in `c_escape`, or specifically break trigraph sequences by inserting an escape.
2. Alternatively, compile with trigraphs disabled if the C compiler supports a portable flag. Do not rely on a non-portable flag alone.
3. Add tests for all nine trigraph sequences: `??=`, `??/`, `??'`, `??(`, `??)`, `??!`, `??<`, `??>`, `??-`.
4. Consider emitting string literals as byte arrays with explicit lengths instead of C string literals. This also fixes the NUL problem below.

Compatibility impact:

Generated C changes, but Valvet string values are preserved. No language-level compatibility change.

Regression test:

```valvet
fun main() {
    println("??/");
    println("??= ??/ ??' ??( ??) ??! ??< ??> ??-");
}
```

Expected output is the exact source text. The generated C must compile without warnings.

Verification:

- Run the repro and compare stdout.
- Run with `-Wall -Wextra -Werror`.
- Fuzz all byte values in string literals and compile the generated C.

### 6.7 VLV-007: Embedded NUL and C hex-escape corruption

Status: Confirmed data corruption; security impact limited but real for data integrity
Severity: Low to Medium
Affected code: `crates/valvet_core/src/codegen.rs:7823-7839` (`c_escape`), `codegen.rs:52-57` (`vstr_from_c`)

Root cause:

`c_escape` emits control characters as `\xNN`. C hexadecimal escapes consume as many hexadecimal digits as possible. A Valvet string containing NUL followed by `b` becomes `\x00b` in C, which is the single character `0x0B`, not NUL followed by `b`. In addition, `vstr_from_c` uses `strlen`, so any embedded NUL would truncate the string even if the escape were split correctly.

Minimal repro:

```sh
cat > /tmp/opencode/nul_escape.valvet <<'EOF'
fun main() {
    val s = "a\0b";
    println(s.len());
    println(s);
}
EOF
target/debug/valvet run /tmp/opencode/nul_escape.valvet
sed -n '725,740p' target/valvet/nul_escape.c
```

Observed result:

```text
2
a
```

The generated C contains:

```c
VStr _vt1 = vstr_from_c("a\x00b");
```

Impact:

- Valvet strings with embedded NUL or control characters do not round-trip.
- Data written to files or passed to C APIs can differ from the source literal.
- If a string is used in a security check and a subsequent operation, both see the corrupted value, so a direct allowlist bypass is not proven. The defect is still a data-integrity and codegen-correctness issue.
- A crafted control character followed by a hex digit can produce an out-of-range C escape and a compile failure.

Recommended fix:

1. Emit fixed-width octal escapes, for example `\000` for NUL and `\177` for DEL. Octal escapes consume at most three digits, so a following hex digit cannot merge.
2. Or emit a byte array plus an explicit length and construct `VStr` with `memcpy`, avoiding `strlen` entirely.
3. Add tests for every byte value from 0 to 255, especially NUL followed by `a` to `f`, and control characters followed by hex digits.
4. Audit all uses of `vstr_from_c` for embedded NUL assumptions.

Compatibility impact:

Strings with control characters now round-trip exactly. Normal strings are unchanged.

Regression test:

```valvet
fun main() {
    val s = "a\0b";
    println(s.len());
    println(s);
}
```

Expected: length 3 and the exact bytes `61 00 62`. A byte-level test is better than printing, because the terminal may hide NUL.

Verification:

- Add a codegen unit test that inspects the generated C and the emitted bytes.
- Run under ASan/UBSan.
- Fuzz string literals across all byte values.

### 6.8 Confirmed finding summary

| ID | Finding | Status | Severity | Primary impact | Fix priority |
|---|---|---|---|---|---|
| VLV-001 | `--target` shell injection | Confirmed | High/Critical | Code execution as user | P0 |
| VLV-002 | Matrix dimension overflow | Confirmed | High | DoS, potential memory corruption | P0 |
| VLV-003 | Parser stack overflow | Confirmed | High/Medium | Compiler DoS | P0 |
| VLV-004 | Hermetic file I/O | Confirmed | High/Medium | Data theft or destruction | P0/P1 |
| VLV-005 | Local stdlib shadowing | Confirmed behavior | Medium/High | Namespace integrity | P1 |
| VLV-006 | C trigraph corruption | Confirmed | Medium | Compile failure, codegen integrity | P1 |
| VLV-007 | NUL/hex escape corruption | Confirmed | Low/Medium | Data integrity | P2 |

## 7. Risk register

The risk register includes confirmed findings, candidates, design risks, and process gaps. Severity is contextual. The CVSS vectors are examples for the stated deployment; they are not a substitute for a maintainer decision.

| ID | Risk | Status | Severity | Likelihood | Impact | Priority | Mitigation |
|---|---|---|---|---|---|---|---|
| VLV-001 | `--target` shell injection | Confirmed | High local, Critical remote | Medium | Code execution | P0 | Remove `sh -c`, validate triple, add regression test |
| VLV-002 | Matrix dimension overflow | Confirmed | High | Medium | DoS, potential OOB write | P0 | Checked multiplication and allocation limits |
| VLV-003 | Parser stack overflow | Confirmed | High/Medium | High | Compiler DoS | P0 | Depth budget and child-process fuzz test |
| VLV-004 | Hermetic file I/O | Confirmed | High if sandbox | High | Data theft or destruction | P0/P1 | OS sandbox or rename and document |
| VLV-005 | Local stdlib shadowing | Confirmed behavior | Medium/High | Medium | Trusted import substitution | P1 | Pin stdlib roots, opt-in local override |
| VLV-006 | C trigraph corruption | Confirmed | Medium | Medium | Compile failure, codegen confusion | P1 | Escape `?`, add trigraph tests |
| VLV-007 | NUL/hex escape corruption | Confirmed | Low/Medium | Low | Data corruption | P2 | Octal escapes or length-aware literals |
| VLV-008 | Git dependency option injection | Candidate | High if reachable | Low/Medium | Code execution | P1 | Validate URL, reject leading `-`, add `--`, test protocols |
| VLV-009 | Vendor/package symlink and path traversal | Candidate | High | Medium | Data leak, outside write | P1 | Sanitize names, do not follow symlinks |
| VLV-010 | Lockfile bypass in CI | Process gap | Medium | Medium | Supply-chain drift | P1 | Require `--frozen`, lock all inputs, CI check |
| VLV-011 | C toolchain compromise | Design risk | High | Low/Medium | Code execution | P1 | Trusted toolchain, hash/version pin |
| VLV-012 | No sanitizer or fuzz CI | Process gap | Medium | High | Regressions | P1 | Add ASan/UBSan and fuzz jobs |
| VLV-013 | `std::sys` command execution by design | Design risk | Critical if untrusted | High | Host compromise | P0 | OS sandbox and explicit capabilities |
| VLV-014 | Arbitrary file I/O by design | Design risk | Critical if untrusted | High | Data theft or destruction | P0 | OS sandbox and path allowlist |
| VLV-015 | Environment and argv leakage | Partial control | Medium | Medium | Secret disclosure | P2 | Block env, cwd, and args under hermetic |
| VLV-016 | CI actions not pinned by SHA | Process gap | Medium | Low/Medium | Supply-chain compromise | P2 | SHA pins, least privilege, no secrets on forks |
| VLV-017 | Custom SHA-256 unverified | Unverified | Low/Medium | Low | Integrity failure | P2 | Differential tests against `sha256sum` |
| VLV-018 | CSV/JSON parser DoS or memory bug | Candidate | Medium/High | Medium | Runtime DoS or corruption | P1 | Fuzz, size limits, ASan |
| VLV-019 | String kernel overflow | Candidate | Medium | Low | Memory safety | P1 | Fuzz, checked arithmetic |
| VLV-020 | No runtime resource limits | Gap | Medium | High | DoS | P1 | cgroups, `ulimit`, timeouts |
| VLV-021 | Release substitution | Candidate | High | Low/Medium | Arbitrary code | P2 | Sign releases, publish checksums, SBOM |
| VLV-022 | Git cache poisoning | Candidate | High | Low | Arbitrary dependency code | P1 | Cache integrity, clean builds, permissions |
| VLV-023 | Compiler diagnostics leak host paths | Low | Low | Low | Information disclosure | P3 | Normalize paths in diagnostics |
| VLV-024 | C compiler warnings not treated as errors | Gap | Low/Medium | Medium | Hidden defects | P2 | `-Wall -Wextra -Werror` in security builds |

Example CVSS vectors:

- VLV-001, local CLI: `CVSS:3.1/AV:L/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H` (7.8 High).
- VLV-001, remote build service: `CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H` (9.8 Critical).
- VLV-002, untrusted dimensions: `CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H` (7.5 High). If a reliable out-of-bounds write is demonstrated, confidentiality and integrity may rise.
- VLV-003, untrusted source build service: `CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H` (7.5 High).
- VLV-004, if `--hermetic` is treated as a sandbox: `CVSS:3.1/AV:N/AC:L/PR:L/UI:N/S:U/C:H/I:H/A:N` (8.1 High) is a starting point, but the maintainers must define the intended boundary before assigning a final score.
- VLV-006, compile failure: `CVSS:3.1/AV:L/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:L` (3.3 Low) until code injection is demonstrated.

## 8. Prioritized penetration testing roadmap

The roadmap is ordered by exploitability, impact, and how much the result changes the trust model. It assumes a local lab with no external network and no real credentials.

### Phase 0: Lab baseline and evidence capture

Objective: make every later test reproducible and safe.

Tasks:

- Freeze the commit and record the working-tree diff.
- Record `rustc --version`, `gcc --version`, `ld --version`, `git --version`, and the container digest.
- Build debug and release binaries with frame pointers and symbols.
- Create the isolated lab image and resource limits.
- Collect the existing fixture and fuzz corpus.
- Add a structured test log with test ID, command, input hash, exit code, signal, sanitizer output, and duration.

Exit criteria:

- Every test can be replayed from a recorded command and input hash.
- No test can reach the host network, host home directory, or real credentials.
- Crash artifacts are collected outside the container.

### Phase 1: P0 trust boundaries and CLI

Objective: eliminate or bound the paths that turn an argument or source file into host code execution.

Tests:

- CLI-01 `--target` injection (confirmed, retest after fix).
- CLI-02 `VALVET_CC` and PATH substitution.
- CLI-03 `valvet new`, `fmt`, `fix` path handling.
- CLI-04 `valvet vendor` dependency-name traversal and symlink following.
- CLI-05 `valvet package` archive path handling.
- CLI-06 `valvet install` overwrite and symlink behavior.
- MOD-05 `--hermetic` file I/O and `sys_cwd`.
- MOD-06 `std::sys` command execution by design.
- SUP-01 git URL option injection.
- SUP-02 git cache poisoning.

Exit criteria:

- No tested CLI argument can execute a shell command.
- No tested path can write outside the intended directory.
- The `--hermetic` contract is either enforced or explicitly documented and renamed.

### Phase 2: Compiler robustness and denial of service

Objective: make malformed or hostile input produce diagnostics, not aborts or hangs.

Tests:

- PAR-01 deep nesting (confirmed, retest after fix).
- PAR-02 huge file and token counts.
- PAR-03 nested comments, long strings, interpolation, triple quotes.
- PAR-04 Unicode, control characters, NUL, invalid escapes.
- PAR-05 numeric literal boundaries and suffixes.
- SEM-01 recursive types, generic nesting, borrow-checker recursion.
- SEM-02 mode and prelude bypass attempts.
- CG-01 string-literal escaping and C lexical hazards.
- CG-02 identifier mangling collisions.
- CG-03 generated C under `-Wall -Wextra -Werror -fanalyzer`.

Exit criteria:

- No compiler abort, panic, or stack overflow on the corpus.
- Every rejected input has a stable diagnostic code.
- Resource limits are enforced and documented.

### Phase 3: Runtime memory safety

Objective: find memory-safety defects in generated C before an attacker does.

Tests:

- RUN-01 matrix dimension overflow (confirmed, retest after fix).
- RUN-02 `Vec`, `ArrayD`, and `Array<T,N>` allocation and shape overflow.
- RUN-03 bounds checks for negative, huge, and multi-dimensional indices.
- RUN-04 use-after-free and double-free under ASan/LSan.
- RUN-05 CSV and JSON parser fuzzing.
- RUN-06 string kernels: concat, replace, substring, trim, split.
- RUN-07 threading races under TSan.
- RUN-08 `sys_capture` truncation and large-output behavior.
- RUN-09 file I/O symlinks, special files, and TOCTOU.

Exit criteria:

- ASan, UBSan, LSan, and TSan are clean on the corpus.
- Every memory-safety test has a minimized repro and a regression test.
- Runtime traps are clean and do not expose memory contents.

### Phase 4: Language, module, and capability security

Objective: ensure the language's own isolation and capability rules cannot be bypassed.

Tests:

- MOD-01 local stdlib shadowing (confirmed, retest after fix).
- MOD-02 import path traversal and symlink resolution.
- MOD-03 namespace isolation, private items, duplicate modules, cycles.
- MOD-04 feature gating and `cfg` bypass.
- MOD-05 hermetic file I/O and metadata leakage.
- MOD-06 subprocess capability gating.
- LANG-01 `unsafe` and `extern` rejection.
- LANG-02 `pub` enforcement across modules.
- LANG-03 ownership and borrow-checker bypass attempts.

Exit criteria:

- Capability checks are enforced at the correct boundary.
- No module or feature path can load code from outside the intended roots.
- The language rejects unsafe constructs with stable diagnostics.

### Phase 5: Supply chain and dependency security

Objective: prevent a malicious dependency, manifest, cache, or release from executing code.

Tests:

- SUP-01 git URL option and protocol injection.
- SUP-02 git cache poisoning.
- SUP-03 lockfile drift and `--frozen` enforcement.
- SUP-04 vendor/package symlink exfiltration.
- SUP-05 C toolchain substitution.
- SUP-06 stdlib provenance and integrity.
- SUP-07 SHA-256 differential testing.
- SUP-08 CI action pinning and permissions.
- SUP-09 release artifact signing and checksum verification.

Exit criteria:

- A malicious manifest cannot execute code during dependency resolution.
- A changed dependency is detected by the lockfile.
- Release artifacts have a verifiable provenance story.
- CI uses least privilege and pinned actions.

### Phase 6: Fuzzing, differential testing, and automation

Objective: turn one-off tests into a continuous feedback loop.

Tasks:

- Extend the stable harness with grammar-aware and deep-nesting generators.
- Add `cargo-fuzz`/libFuzzer targets when the toolchain is available.
- Add AFL++ targets for the CLI and file-based front end.
- Add runtime harnesses for CSV, JSON, matrix dimensions, and strings.
- Add differential runs: debug vs release, GCC vs Clang, generated C vs expected output.
- Add coverage measurement and corpus minimization.
- Add sanitizer jobs to CI.
- Add nightly fuzz jobs with artifact upload.

Exit criteria:

- Every parser, codegen, and runtime target has a fuzz harness.
- Crashes are automatically minimized and stored in a regression corpus.
- CI blocks on new sanitizer or fuzz findings.

### Phase 7: Remediation, regression, and release gates

Objective: fix, verify, and prevent recurrence.

Tasks:

- Fix P0 findings first.
- Add regression tests for every confirmed finding.
- Add security notes to release documentation.
- Run the full gate: fmt, clippy, unit tests, front-end tests, fixtures debug/release, sanitizers, fuzz smoke.
- Update the threat model and risk register.
- Review compatibility and migration notes.
- Publish a security advisory if appropriate.

Exit criteria:

- All P0 findings are fixed or explicitly accepted with a documented risk owner.
- No sanitizer findings on the release corpus.
- The release passes `valvet verify --frozen` and the security gate.

## 9. Security testing matrix

The matrix is grouped by subsystem. Each row is a planned test unless marked Confirmed. The "Automation" column says whether the test can be automated in CI or the lab.

### 9.1 CLI and build

| Test ID | Component | Attack scenario | Test method | Expected secure behavior | Success criteria | Severity | Automation |
|---|---|---|---|---|---|---|---|
| CLI-01 | `apply_target` | Shell metacharacters in `--target` | Run with `--target 'x; touch marker #'` | Reject invalid triple, never invoke a shell | Marker absent, nonzero exit, diagnostic | Critical/High | Yes |
| CLI-02 | C driver selection | Malicious `VALVET_CC` or PATH `gcc` | Set `VALVET_CC` to a marker script, build | Trusted toolchain or explicit warning | No unexpected execution, or documented trust boundary | High | Yes |
| CLI-03 | `new`, `fmt`, `fix` | `../` and absolute paths | Run with paths outside a temp root | Confine writes or require explicit absolute path | No outside writes | Medium | Yes |
| CLI-04 | `vendor` | Dependency name traversal, symlink to host file | Manifest with `../evil` and a symlink dependency | Sanitize names, do not follow symlinks | No outside write or secret copy | High | Yes |
| CLI-05 | `package` | `..` or absolute archive entries | Package a tree with hostile paths | Safe archive names | No `..` or absolute entries | Medium | Yes |
| CLI-06 | `install` | Pre-existing symlink in `~/.local/bin` | Install over a symlink | Atomic write, do not follow symlink | Target not overwritten | Medium | Yes |
| CLI-07 | `test` | Untrusted fixture executes `sys_run` or file I/O | Run a fixture with marker side effects | Sandbox or explicit trust | No host side effects | High | Yes |
| CLI-08 | `--target` parser | Missing value, flag as value, `--` separator | Fuzz argument sequences | Clean usage error | No panic, no shell | Medium | Yes |

### 9.2 Lexer, parser, and compiler

| Test ID | Component | Attack scenario | Test method | Expected secure behavior | Success criteria | Severity | Automation |
|---|---|---|---|---|---|---|---|
| PAR-01 | Parser | Deeply nested parentheses or blocks | 100k nested input under `ulimit -s 8192` | Depth limit diagnostic | No stack overflow | High | Yes |
| PAR-02 | Lexer/parser | Huge source or token count | 1 GB file, 10M tokens, resource cap | Clean size error | No OOM or hang | Medium | Yes |
| PAR-03 | Lexer | Nested comments, long strings, interpolation | Generate adversarial but valid input | Bounded work | No hang or panic | Medium | Yes |
| PAR-04 | Lexer | Unicode, control bytes, invalid escapes | Fuzz all byte values and escapes | Stable diagnostics | No panic, exact round-trip where valid | Medium | Yes |
| PAR-05 | Lexer/parser | Integer literal overflow, suffix abuse | Max/min values, huge decimals, hex | Clean diagnostic | No wrap or panic | Low | Yes |
| SEM-01 | Sema | Recursive types, generic nesting, borrow recursion | Generate nested types and borrows | Depth limit | No stack overflow | High | Yes |
| SEM-02 | Sema | Mode and prelude bypass | `mode imp` under hermetic, prelude in exp | E0604 | Rejected | High | Yes |
| CG-01 | Codegen | C string escaping, trigraphs, NUL, quotes | Fuzz all byte values and compile | Valid C, exact bytes | No C compile failure, exact output | High | Yes |
| CG-02 | Codegen | Identifier mangling collisions | Names with `__`, overloads, Unicode | Unique C symbols | No collision or wrong binding | Medium | Yes |
| CG-03 | Codegen | C warnings and undefined behavior | Compile with `-Wall -Wextra -Werror -fanalyzer` | Clean C | No warnings | Medium | Yes |

### 9.3 Runtime and standard library

| Test ID | Component | Attack scenario | Test method | Expected secure behavior | Success criteria | Severity | Automation |
|---|---|---|---|---|---|---|---|
| RUN-01 | `Mat` | Dimension product overflow | `mat_zeros(2^32, 2^32)` then access | Checked multiplication, clean trap | No segfault or OOB | High | Yes |
| RUN-02 | `Vec`, `ArrayD`, `Array` | Allocation and shape overflow | Huge lengths and shapes | Checked arithmetic and limits | No overflow or OOB | High | Yes |
| RUN-03 | Indexing | Negative and huge indices | Fuzz indices for all containers | Bounds trap | No OOB | High | Yes |
| RUN-04 | Ownership/runtime | Use-after-free, double-free | ASan/LSan on fixtures and fuzz corpus | No invalid frees | Clean sanitizer output | High | Yes |
| RUN-05 | CSV/JSON | Malformed, huge, deeply nested data | Fuzz parsers with size limits | Clean error, bounded memory | No crash, leak, or hang | High | Yes |
| RUN-06 | String kernels | Huge concat, replace, substring, trim | Boundary and fuzz tests | Checked lengths | No overflow or OOB | High | Yes |
| RUN-07 | Threading | Data races in `par_sum`/`par_map` | TSan on varied lengths and thread counts | Race-free | TSan clean | Medium | Yes |
| RUN-08 | `sys_capture` | Output larger than 1 MB, binary output | Run a command producing large output | Bounded capture, correct length | No overflow or hang | Medium | Yes |
| RUN-09 | File I/O | Symlinks, `/proc`, special files, TOCTOU | Read/write hostile paths in lab | Documented capability or denial | No surprise host effect | Medium | Yes |
| RUN-10 | `StringIndex` | Hash collisions, growth, clone | Adversarial keys and sizes | Correct positions, bounded memory | No corruption or DoS | Medium | Yes |

### 9.4 Language, modules, and capabilities

| Test ID | Component | Attack scenario | Test method | Expected secure behavior | Success criteria | Severity | Automation |
|---|---|---|---|---|---|---|---|
| MOD-01 | Resolver | Local `std/` shadows stdlib | Local `std/math.valvet` with unique item | Stdlib pinned or explicit opt-in | Local shadow rejected by default | Medium/High | Yes |
| MOD-02 | Resolver | Import path traversal or symlink | `..`, absolute paths, symlink modules | Confine to roots | No outside file load | High | Yes |
| MOD-03 | Sema | Private item access, duplicate modules, cycles | Negative fixtures | E0800/E0701 | Rejected | Medium | Yes |
| MOD-04 | Features | Undeclared or injected feature | `cfg`, `--features`, `VALVET_FEATURES` | Only declared features | No bypass | Medium | Yes |
| MOD-05 | Hermetic | File read/write, `cwd`, args | Run under `--hermetic` | Deny or document | Contract enforced | High | Yes |
| MOD-06 | `std::sys` | Untrusted source runs shell command | `sys_run("touch marker")` | OS sandbox or explicit trust | No host command without opt-in | Critical | Yes |
| LANG-01 | Parser | `unsafe` and `extern` | Negative fixtures | E0800 | Rejected | High | Yes |
| LANG-02 | Modules | `pub` enforcement | Cross-module private access | Compile error | Rejected | Medium | Yes |
| LANG-03 | Ownership | Borrow-checker bypass | Adversarial aliasing and moves | Reject | No unsound acceptance | High | Yes |

### 9.5 Supply chain and CI

| Test ID | Component | Attack scenario | Test method | Expected secure behavior | Success criteria | Severity | Automation |
|---|---|---|---|---|---|---|---|
| SUP-01 | Git deps | URL option or protocol injection | Manifest with `--upload-pack`, `ext::`, `file://` | Reject unsafe URLs, use `--` | No command execution | High | Yes |
| SUP-02 | Git cache | Pre-poisoned cache repository | Populate cache with hostile repo | Detect or clean cache | No untrusted code | High | Yes |
| SUP-03 | Lockfile | Dependency drift | Modify dep, run with and without `--frozen` | Frozen detects drift | Build fails on drift | Medium | Yes |
| SUP-04 | Vendor/package | Symlink to `/etc/passwd` | Vendor or package a hostile dep | No symlink follow | No secret in output | High | Yes |
| SUP-05 | Toolchain | Malicious `VALVET_CC`, PATH, `LD_LIBRARY_PATH` | Substitute a marker compiler | Trusted toolchain or warning | No unexpected execution | High | Yes |
| SUP-06 | Stdlib | Modified `stdlib/` or `VALVET_STDLIB` | Hash and provenance checks | Detect modification | Mismatch detected | High | Yes |
| SUP-07 | SHA-256 | Hash implementation error | Differential vs `sha256sum` on random inputs | Exact match | All vectors match | Medium | Yes |
| SUP-08 | CI | Mutable action tag, excessive permissions | Audit workflow, simulate fork PR | SHA pins, least privilege | No secrets on forks | Medium | Yes |
| SUP-09 | Release | Substituted binary or archive | Verify signature and checksum | Verifiable provenance | Verification fails on tamper | High | Yes |

### 9.6 Fuzzing, lab, and continuous testing

| Test ID | Component | Attack scenario | Test method | Expected secure behavior | Success criteria | Severity | Automation |
|---|---|---|---|---|---|---|---|
| FUZ-01 | Lexer/parser | Arbitrary bytes and token soup | Stable harness and libFuzzer/AFL++ | No panic, hang, or stack overflow | No unique crashes | High | Yes |
| FUZ-02 | Codegen/runtime | Generated programs and data | Compile and run with ASan/UBSan | No UB or memory errors | Clean sanitizer output | High | Yes |
| FUZ-03 | Differential | Compiler or optimizer difference | Debug vs release, GCC vs Clang | Same observable behavior | No unexplained diff | Medium | Yes |
| LAB-01 | Lab | Host escape or network access | Run with no network, read-only root, cgroups | Contained | No host side effects | High | Yes |
| CI-01 | CI | Missing sanitizer gate | Add ASan/UBSan/LSan job | Gate on findings | Job fails on finding | High | Yes |
| CI-02 | CI | Missing fuzz gate | Add PR smoke and nightly fuzz | Regression corpus | New crash blocks merge | Medium | Yes |

## 10. Fuzzing strategy

### 10.1 Targets and harnesses

| Target | Harness | Input model | Oracles |
|---|---|---|---|
| Lexer | `cargo-fuzz` or AFL++ binary target | Arbitrary bytes, valid UTF-8 and edge escapes | No panic, no hang, stable token spans |
| Parser | Grammar-aware generator plus mutation | Valid and invalid programs, deep nesting, imports | No panic, no stack overflow, stable diagnostics |
| Sema | AST or source generator | Types, generics, borrows, modes, features | No panic, no unsound acceptance |
| Codegen | Source generator plus C compiler | String literals, identifiers, numeric edge cases | Generated C compiles, output matches |
| Runtime | Generated programs plus data | Matrix dimensions, CSV, JSON, strings, indices | No sanitizer finding, no hang, expected output |
| CLI | Argument fuzzer | `--target`, paths, flags, manifests | No shell execution, no outside write |
| Supply chain | Manifest and git URL fuzzer | Option-shaped URLs, protocols, paths | No command execution, clean rejection |

The existing stable harness (`crates/valvet_core/tests/fuzz_soup.rs`) is a good starting point. It uses a deterministic xorshift PRNG, token soup, and fixture mutations inside `catch_unwind`. It deliberately caps token-soup length at 120, so it does not exercise deep recursion. Extend it in two ways:

1. A separate deep-nesting generator that runs each case in a child process with `timeout` and `ulimit -s`. A stack overflow is a signal, not a catchable panic.
2. A grammar-aware generator that produces semantically valid programs with controlled depth, imports, types, and modes.

### 10.2 Corpus and seeds

- All `valvet-tests/compile-pass` and `compile-fail` fixtures.
- All `examples/*.valvet`.
- All `stdlib/*.valvet`.
- Hand-written seeds for: 0, 1, `INT32_MAX`, `INT64_MAX`, `2^31`, `2^32`, negative values, empty strings, NUL, all control bytes, trigraphs, quotes, backslashes, Unicode, CRLF, nested comments, nested interpolation, deep blocks, deep types, local `std/` trees, malicious manifests.
- CSV seeds: empty, one field, quoted fields, embedded commas, embedded newlines, escaped quotes, ragged rows, huge fields, invalid UTF-8 in data.
- JSON seeds: deeply nested arrays and objects, huge strings, duplicate keys, invalid escapes, truncated input.
- Matrix seeds: zero dimensions, 1x1, large dimensions, overflow candidates, singular matrices, non-square matrices.
- CLI seeds: `--target` with metacharacters, missing values, `--` separators, absolute and relative paths, symlinks.

### 10.3 Tools and sanitizers

- `cargo-fuzz`/libFuzzer when a nightly toolchain and LLVM are available.
- AFL++ for file-based and CLI targets when libFuzzer is unavailable.
- The stable Rust harness for CI environments without nightly.
- AddressSanitizer, UndefinedBehaviorSanitizer, LeakSanitizer, and ThreadSanitizer where supported.
- `-O1 -g -fno-omit-frame-pointer` for sanitizer builds.
- `-fsanitize=address,undefined` for generated C tests.
- `-fsanitize=thread` for `par_sum`, `par_map`, and any shared runtime state.
- `-Wall -Wextra -Werror` and `-fanalyzer` for generated C.
- Optional: `cargo miri` for the Rust front end if the code remains Miri-compatible.
- Optional: `cargo llvm-cov` for coverage, or `-C instrument-coverage`.

### 10.4 Metrics and triage

Track:

- Executions per second per target.
- Line and branch coverage.
- Corpus size and growth.
- Unique crashes after deduplication by stack hash.
- Time to first crash.
- Minimized input size.
- Sanitizer findings by type.
- Regression corpus pass rate.
- Time from crash to fix.

Triage workflow:

1. Reproduce in the lab with the recorded seed and input hash.
2. Classify as panic, abort, stack overflow, hang, sanitizer finding, or wrong output.
3. Minimize with `cargo fuzz tmin`, `creduce`, or a custom reducer.
4. Add the minimized input to the regression corpus.
5. File a private issue with the repro, root cause, and impact.
6. Fix and rerun the full gate.
7. Verify the original crash no longer reproduces.

### 10.5 CI cadence

- Per pull request: fmt, clippy, build, unit tests, fixture suite, sanitizer smoke on a small corpus, fuzz smoke for 5 to 10 minutes per target.
- Nightly: 30 to 60 minutes per fuzz target, ASan/UBSan/LSan, TSan for threaded runtime, coverage report, corpus minimization.
- Weekly: longer fuzz run, differential GCC/Clang, dependency and action audit.
- Release: full fuzz corpus, sanitizer clean, `valvet verify --frozen`, SBOM, signed artifacts.

### 10.6 Existing harness gaps

- No deep-recursion coverage because of the 120-token cap.
- No child-process signal handling for stack overflow.
- No grammar-aware generator.
- No runtime fuzzing.
- No CSV/JSON fuzzing.
- No CLI argument fuzzing.
- No differential testing.
- No coverage measurement.
- No continuous fuzz job in CI.

## 11. Security test laboratory design

### 11.1 Container baseline

Use a pinned container image and a non-root user. Example:

```dockerfile
FROM rust:1.83-bookworm@sha256:<pin>

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      gcc clang lld git curl jq gdb strace \
 && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 10001 lab
USER lab
WORKDIR /work
```

Run with:

```sh
docker run --rm -it \
  --network none \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=1g \
  --tmpfs /work/out:rw,nosuid,size=1g \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  --pids-limit=256 \
  --memory=2g \
  --cpus=2 \
  -v "$PWD:/work:ro" \
  valvet-security-lab:<digest>
```

For tests that need a writable worktree, copy the repository into a tmpfs or a dedicated writable volume. Do not mount the host home directory.

### 11.2 Resource limits

- `timeout 30s` for compiler tests; shorter for known fast inputs.
- `ulimit -s 8192` or lower to expose stack recursion.
- `ulimit -v 2097152` to cap virtual memory.
- `ulimit -c unlimited` in the container if core collection is enabled.
- cgroup v2 memory, CPU, and PID limits.
- `nice` or `ionice` for long fuzz runs.
- No unbounded output: redirect stdout/stderr to files with size limits.

### 11.3 Network and filesystem

- Default: no network.
- Git dependency tests: use local `file://` repositories or a local `git daemon` in a separate network namespace. Do not contact public remotes.
- Filesystem: read-only root, writable tmpfs only for test artifacts.
- Provide a dummy `HOME`, `TMPDIR`, and `PATH`.
- Do not mount SSH keys, cloud credentials, browser profiles, or source trees with secrets.
- For file I/O tests, use a dedicated directory with harmless marker files.
- For `sys_run` tests, use a restricted `sh` wrapper or run only in the container with no host mounts.

### 11.4 Sanitizers and debug builds

- Rust front end: debug build with `-C debuginfo=2`, `RUST_BACKTRACE=1`.
- Generated C: `-O1 -g -fno-omit-frame-pointer -fsanitize=address,undefined`.
- Leak checks: `ASAN_OPTIONS=detect_leaks=1:abort_on_error=1:strict_string_checks=1`.
- UBSan: `UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1`.
- TSan: `TSAN_OPTIONS=halt_on_error=1`.
- C static analysis: `gcc -fanalyzer`, `clang --analyze`, `cppcheck` where available.
- Optional: Valgrind for uninitialized reads and leaks.

### 11.5 Crash collection and logging

- Set `core_pattern` to a container-local path or disable core dumps and rely on sanitizer output.
- Capture exit code, signal, stdout, stderr, sanitizer report, and input hash.
- Store the exact command, environment allowlist, commit, toolchain versions, and container digest.
- Use a structured JSON log per test case.
- Keep crash inputs in a read-only artifact directory and upload them from CI.

### 11.6 Reproducibility

- Pin the Rust toolchain with `rust-toolchain.toml`.
- Record the C compiler version and path.
- Pin the container image by digest.
- Record the commit and working-tree diff.
- Record the corpus hash and PRNG seed.
- Record the exact environment variables that affect behavior: `VALVET_CC`, `VALVET_STDLIB`, `VALVET_OFFLINE`, `VALVET_HERMETIC`, `VALVET_FEATURES`, `PATH`, `HOME`, `LD_LIBRARY_PATH`.
- Re-run the same input twice and compare output and crash signature.

### 11.7 Safety rules

- Test only Valvet components and environments you own or are authorized to assess.
- Do not target third-party systems.
- Do not use real credentials or production data.
- Use marker files and harmless commands for injection tests.
- Do not run untrusted code outside the lab.
- Do not leave a `sys_run` test running on the host.
- Clean up temporary directories and processes after each run.

## 12. Remediation playbook

### 12.1 General workflow

1. Triage: confirm the finding against the current commit, record the exact input and environment.
2. Classify: memory safety, command injection, path traversal, denial of service, trust-boundary gap, or process gap.
3. Scope: identify affected versions, platforms, and deployment modes.
4. Root cause: locate the smallest code path that causes the behavior.
5. Fix: prefer a central invariant or helper over a local patch.
6. Regression test: add a test that fails before the fix and passes after it.
7. Compatibility: document any language, ABI, or CLI behavior change.
8. Verification: run the full gate, including sanitizers and fuzz smoke.
9. Release: include a security note, advisory, or CVE if appropriate.
10. Follow-up: update the threat model, risk register, and fuzzing corpus.

### 12.2 Fix patterns for confirmed issues

| Finding | Fix pattern | Regression test |
|---|---|---|
| VLV-001 | Remove `sh -c`, validate target triple, search PATH in Rust | Marker file is not created |
| VLV-002 | Checked multiplication and allocation helpers, element limit | Overflow input produces clean trap, not segfault |
| VLV-003 | Parser depth budget, child-process fuzz test | Deep input produces diagnostic, no abort |
| VLV-004 | OS sandbox or rename and document; block file I/O if hermetic is a boundary | File read/write denied or documented |
| VLV-005 | Resolve `std::` only from pinned stdlib roots | Local shadow rejected by default |
| VLV-006 | Escape `?`, test all trigraphs | `"??/"` compiles and prints exactly |
| VLV-007 | Octal escapes or length-aware string literals | `"a\0b"` has length 3 and exact bytes |
| VLV-008 | Reject leading `-`, use `--`, allowlist protocols | Malicious URL does not execute |
| VLV-009 | Sanitize dependency names, do not follow symlinks | No outside write or secret copy |
| VLV-010 | Require `--frozen` in CI, lock all inputs | Drift fails the build |
| VLV-011 | Trusted toolchain, version/hash pin, hardened flags | Substituted compiler is detected |
| VLV-012 | Add sanitizer and fuzz CI jobs | New finding blocks merge |

### 12.3 Verification checklist

- The original repro no longer triggers the finding.
- A regression test fails on the old code and passes on the new code.
- The full test suite passes in debug and release.
- `cargo fmt --all --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `valvet test valvet-tests` passes in debug and release.
- ASan, UBSan, LSan, and TSan are clean on the affected subsystem.
- The fuzz smoke test and the regression corpus pass.
- The fix does not introduce a new warning or sanitizer finding.
- The compatibility note is written.
- The threat model and risk register are updated.

### 12.4 Compatibility and release notes

- VLV-001: valid triples unchanged; invalid triples now rejected.
- VLV-002: very large matrices now produce a clean error; document the limit.
- VLV-003: deeply nested programs beyond the limit now produce a diagnostic; document the limit.
- VLV-004: tightening `--hermetic` may break scripts that read files; provide a migration path or rename the flag.
- VLV-005: rejecting local `std/` may break projects that rely on shadowing; require an opt-in flag.
- VLV-006 and VLV-007: generated C changes, Valvet source behavior is corrected; no migration needed.
- VLV-008 and VLV-009: unsafe dependency shapes are rejected; document the allowed manifest forms.

### 12.5 Regression test policy

- Every confirmed security finding gets a named test.
- Every test has a stable ID matching the matrix.
- Tests that can crash the process run in a child process with a timeout.
- Tests that need host side effects use a sandbox or a marker file in a temp directory.
- Crash inputs are stored in the repository or a dedicated regression corpus.
- The test suite records the expected diagnostic code, exit code, or signal.
- Security tests are part of the normal CI gate, not a separate manual checklist.

## 13. Continuous security testing

### 13.1 CI gates

Current CI (`ci.yml`) runs:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo build --workspace`
- `cargo test --workspace`
- `valvet test valvet-tests` in debug and release

Add:

- ASan/UBSan/LSan job for the front end and generated C.
- TSan job for threaded runtime tests.
- Fuzz smoke job per pull request.
- Nightly fuzz job with corpus caching.
- Generated C compile check with `-Wall -Wextra -Werror -fanalyzer`.
- `valvet verify --frozen` on release trees.
- SHA-256 differential test against `sha256sum`.
- Action pinning audit and least-privilege review.

### 13.2 Static analysis

- `cargo clippy` with `-D warnings`.
- `cargo fmt --check`.
- `cargo miri test` if the front end remains compatible.
- `gcc -fanalyzer` and `clang --analyze` on generated C.
- `cppcheck` on generated C.
- `semgrep` or CodeQL rules for Rust command execution, path handling, and unsafe code.
- Search for `Command::new("sh")`, `system(`, `popen(`, `fopen(`, `getenv(`, and raw `malloc`/`realloc` in code review.

### 13.3 Dependency and supply-chain controls

- Keep the Rust dependency set at zero unless necessary.
- If dependencies are added, use `cargo audit`, `cargo deny`, and a reviewed lockfile.
- Pin GitHub Actions by commit SHA, not mutable tags.
- Use least-privilege `permissions:` in workflows.
- Do not expose secrets to pull requests from forks.
- Pin the container image and toolchain by digest or version.
- Verify release artifacts with checksums and signatures.
- Publish an SBOM for releases.
- Use `--frozen` in CI and release builds.
- Treat `VALVET_CC`, `VALVET_STDLIB`, and `PATH` as trusted inputs and document that.

### 13.4 Release security gates

- All P0 findings fixed or accepted with a risk owner.
- No open P1 finding without a mitigation.
- Sanitizer clean on the release corpus.
- Fuzz smoke and regression corpus pass.
- `valvet verify --frozen` passes.
- Generated C compiles with warnings as errors.
- Release artifacts are signed or checksummed.
- Security notes and compatibility notes are written.
- The threat model and risk register are current.

### 13.5 Metrics

- Number of open security findings by severity.
- Mean time to remediate by severity.
- Sanitizer clean runs.
- Fuzz coverage and unique crash count.
- Regression corpus size and pass rate.
- Percentage of dependencies pinned and locked.
- CI security gate pass rate.
- Number of security tests in the suite.
- Time to reproduce and verify a finding.

### 13.6 Responsible disclosure

- Provide a private security contact, for example a `SECURITY.md` and a GitHub private vulnerability reporting channel.
- Acknowledge reports within a documented window, for example 3 business days.
- Triage and provide an initial assessment within 10 business days.
- Agree on a disclosure timeline, commonly 90 days, with extensions for complex fixes.
- Request a CVE when appropriate.
- Credit the reporter unless they request anonymity.
- Do not publish exploit details before a fix is available.
- Publish an advisory with affected versions, fixed versions, workarounds, and severity.
- Keep a security advisory archive and a `security.txt` file.

## 14. 30/60/90-day roadmap

### Days 0-30: stop the bleeding

| Week | Milestone | Deliverables |
|---|---|---|
| 1 | Fix P0 injection and runtime overflow | Remove `sh -c` from `apply_target`; add checked matrix arithmetic; regression tests |
| 1 | Bound compiler recursion | Parser depth limit; child-process deep-input test |
| 2 | Define the hermetic contract | Decide sandbox vs rename; update `README.md` and `--help`; add file I/O tests |
| 2 | Pin stdlib resolution | Resolve `std::` only from stdlib roots; add opt-in local override |
| 3 | Fix codegen escaping | Escape trigraphs; fix NUL/hex escapes; byte-level string tests |
| 3 | Stand up the lab | Container image, resource limits, no-network baseline, crash collection |
| 4 | Add CI security jobs | ASan/UBSan/LSan smoke, fuzz smoke, generated C warnings |
| 4 | Publish initial risk register | Confirmed findings, owners, target dates |

Exit criteria:

- No confirmed P0 finding remains exploitable in the default configuration.
- Every confirmed finding has a regression test.
- The `--hermetic` contract is explicit.
- CI runs at least one sanitizer and one fuzz smoke job.

### Days 31-60: harden the boundaries

| Week | Milestone | Deliverables |
|---|---|---|
| 5 | Supply-chain audit | Git URL validation, cache permissions, vendor/package symlink tests |
| 5 | Lockfile enforcement | `--frozen` in CI, dependency content hashes, drift tests |
| 6 | Runtime fuzzing | CSV, JSON, matrix, string, and index fuzz targets |
| 6 | Parser and sema fuzzing | Grammar-aware generator, deep-nesting corpus, coverage |
| 7 | C hardening | PIE, RELRO, stack protector, FORTIFY, `-Werror`, `-fanalyzer` |
| 7 | Toolchain provenance | Record compiler version and path; optional hash check |
| 8 | Release security | Signed artifacts or checksums, SBOM, security notes |
| 8 | Disclosure process | `SECURITY.md`, `security.txt`, response SLA |

Exit criteria:

- A malicious manifest cannot execute code during dependency resolution.
- A changed dependency is detected by `--frozen`.
- Runtime and parser fuzz targets run in CI.
- Release artifacts have a verifiable integrity story.

### Days 61-90: prove and sustain

| Week | Milestone | Deliverables |
|---|---|---|
| 9 | Full test matrix execution | All P0/P1 tests executed and recorded |
| 9 | External review | Independent review of parser, codegen, and runtime |
| 10 | Long fuzz campaign | Nightly 1-hour runs, minimized regression corpus |
| 10 | Differential testing | Debug vs release, GCC vs Clang, output comparison |
| 11 | Threat model review | Maintainer sign-off, updated assumptions, risk acceptance |
| 11 | Security release | Fixed version, advisory, migration notes |
| 12 | Continuous improvement | Metrics dashboard, quarterly review, next roadmap |

Exit criteria:

- No known P0 or P1 finding without a documented mitigation.
- Sanitizer and fuzz gates are stable.
- The threat model is reviewed and owned.
- The release has a security advisory and a verifiable artifact.
- The team has a repeatable security process.

## 15. Test execution log

These are the tests actually executed during this preliminary assessment. Everything not in this table is planned or code-confirmed.

| ID | Command or input | Result | Status |
|---|---|---|---|
| CLI-01 | `target/debug/valvet --target 'x; touch /tmp/opencode/valvet_target_injection #' check examples/hello.valvet` | Exit 0; marker file created | Confirmed |
| RUN-01 | `mat_zeros(4294967296 as Int64, 4294967296 as Int64)` then `m[0, 0]` | Built binary segfaults, exit 139 | Confirmed |
| PAR-01 | 100,000 nested parentheses, `ulimit -s 8192`, `valvet check` | `thread 'main' has overflowed its stack`, abort 134 | Confirmed |
| MOD-05 | `read_file("/etc/hostname")` under `valvet run --hermetic` | Read succeeded, printed `archlinux`, exit 0 | Confirmed |
| MOD-05 | `write_file("/tmp/opencode/hermetic_write.txt", "written")` under `--hermetic` | Wrote 7 bytes, file present, exit 0 | Confirmed |
| MOD-06 | `sys_run("id")` under `--hermetic` | `error[E0401]`, rejected | Confirmed |
| MOD-01 | Local `std/math.valvet` with `pub fun foo`, imported as `std::math::foo` | Printed `999` in normal and `--hermetic` runs | Confirmed |
| CG-01 | `println("??/")` | Generated C failed to compile: missing terminating `"` | Confirmed |
| CG-02 | `val s = "a\0b"; println(s.len()); println(s)` | Printed length 2 and corrupted bytes | Confirmed |
| ARCH-01 | Source search for FFI, networking, build scripts, registry | None found in the current tree | Code-confirmed |
| SUP-01 | Direct `git clone --upload-pack='touch marker; echo' <local repo>` | Marker created; not reached through `valvet.toml` | Candidate |
| SUP-01 | `valvet check` with a manifest `git = "--upload-pack=..."` | Git failed before executing the payload; cache path used as repository | Candidate |
| CI-01 | Read `.github/workflows/ci.yml` | fmt, clippy, build, tests, fixtures, release; no sanitizer/fuzz | Code-confirmed |

## 16. Recommended immediate next actions

Status: the seven confirmed findings below were fixed in the working tree; see section 17 for the implementation and regression coverage.

1. Fix VLV-001 now. Remove `sh -c` from `apply_target`, validate the triple, and add a marker-file regression test.
2. Fix VLV-002 now. Add checked multiplication and allocation helpers for all matrix shapes, and audit every allocation and index product in `codegen.rs`.
3. Fix VLV-003 now. Add a parser depth budget and a child-process deep-input test.
4. Decide what `--hermetic` means. Either implement an OS sandbox or rename and document it as a subprocess and environment restriction. Do not call it a sandbox unless it restricts filesystem and host metadata.
5. Pin standard-library resolution. Resolve `std::` only from explicit stdlib roots by default, and require an opt-in for local shadowing.
6. Fix C string escaping. Escape trigraphs and use fixed-width octal escapes or length-aware string literals.
7. Add sanitizer and fuzz jobs to CI. Start with ASan/UBSan/LSan on the existing fixtures, then add runtime and parser fuzzing.
8. Audit the supply chain. Validate git URLs, use `--` separators, test vendor/package symlink handling, and require `--frozen` in CI.
9. Write the trust model down. Add a `SECURITY.md` that states whether Valvet source is trusted, what `--hermetic` guarantees, and how to run untrusted code safely.
10. Re-run the matrix after the P0 fixes and update this document with the new evidence.

## Appendix A: Source references

| Topic | Reference |
|---|---|
| Pipeline and backend | `docs/implementation/ARCHITECTURE.md` |
| CLI and `--target` | `crates/valvet_cli/src/main.rs:311-340` |
| `--hermetic` setup | `crates/valvet_cli/src/main.rs:28-31` |
| Hermetic mode selection | `crates/valvet_core/src/resolver.rs:90-108`, `568-583` |
| `sys_run`/`sys_capture` gating | `crates/valvet_core/src/sema.rs:7118-7153` |
| `sys_getenv` gating | `crates/valvet_core/src/sema.rs:7155-7185` |
| `read_file` sema | `crates/valvet_core/src/sema.rs:6994-6997` |
| `write_file` sema | `crates/valvet_core/src/sema.rs:7783-7816` |
| `v_read_file` | `crates/valvet_core/src/codegen.rs:267-288` |
| `v_write_file_wrap` | `crates/valvet_core/src/codegen.rs:2631-2647` |
| `vsys_run`/`vsys_capture` | `crates/valvet_core/src/codegen.rs:113-138` |
| `vmat_zeros` | `crates/valvet_core/src/codegen.rs:3427` |
| `vmat_identity` | `crates/valvet_core/src/codegen.rs:3430` |
| `vmat_det` | `crates/valvet_core/src/codegen.rs:3475` |
| `c_escape` | `crates/valvet_core/src/codegen.rs:7823-7839` |
| String literal emission | `crates/valvet_core/src/codegen.rs:5565` |
| Stdlib resolution | `crates/valvet_core/src/resolver.rs:674-695` |
| Stdlib roots | `crates/valvet_core/src/resolver.rs:121-137` |
| Manifest parser | `crates/valvet_core/src/manifest.rs:98-168` |
| Git dependency resolution | `crates/valvet_core/src/resolver.rs:221-272` |
| Lockfile | `crates/valvet_cli/src/main.rs:765-883` |
| Vendor | `crates/valvet_cli/src/main.rs:652-695` |
| Package | `crates/valvet_cli/src/main.rs:509-585` |
| Install | `crates/valvet_cli/src/main.rs:587-617` |
| Fuzz harness | `crates/valvet_core/tests/fuzz_soup.rs` |
| CI | `.github/workflows/ci.yml` |
| C flags | `crates/valvet_core/src/codegen.rs:7874-7895` |
| No FFI | `crates/valvet_core/src/parser.rs:366-377` |
| No network stdlib | `stdlib/` inventory and search |
| No build scripts | `crates/valvet_core/src/manifest.rs` section parser |

## Appendix B: Suggested test IDs for the first implementation pass

Use these IDs in the test suite and in the risk register:

- `SEC-CLI-001`: `--target` injection
- `SEC-CLI-002`: `VALVET_CC` substitution
- `SEC-CLI-003`: vendor path traversal
- `SEC-CLI-004`: vendor symlink
- `SEC-PAR-001`: deep nesting
- `SEC-PAR-002`: huge input
- `SEC-CG-001`: trigraph escaping
- `SEC-CG-002`: NUL and control bytes
- `SEC-RUN-001`: matrix dimension overflow
- `SEC-RUN-002`: array shape overflow
- `SEC-RUN-003`: CSV parser fuzz
- `SEC-RUN-004`: JSON parser fuzz
- `SEC-MOD-001`: local stdlib shadowing
- `SEC-MOD-002`: import path traversal
- `SEC-MOD-003`: hermetic file I/O
- `SEC-SUP-001`: git URL option injection
- `SEC-SUP-002`: lockfile drift
- `SEC-SUP-003`: cache poisoning
- `SEC-CI-001`: sanitizer gate
- `SEC-CI-002`: fuzz smoke gate

These IDs are suggestions. The maintainers should keep one naming scheme across the matrix, code, and release notes.

## 17. Remediation status (2026-10-25)

All seven confirmed findings from this assessment are fixed in the working tree. This section records the change and the regression coverage. The rest of the document remains the pre-fix evidence baseline.

| ID | Fix | Regression coverage |
|---|---|---|
| VLV-001 | `--target` validates the triple as ASCII letters, digits, `-`, `_`, `.`, and `+`, starting alphanumeric and at most 128 bytes. The driver is resolved with a direct `PATH` search (`codegen::find_program`), never `sh -c`. `find_cc` uses the same shell-free lookup. | `crates/valvet_cli/tests/security.rs::target_triple_is_not_a_shell_fragment` passes a marker-file payload and asserts exit 2 with no marker. |
| VLV-002 | Matrix shapes go through `vmat_checked_mul`, `vmat_checked_elems`, `vmat_alloc`, and `vmat_alloc_i64`. Overflow, negative dimensions, `SIZE_MAX` overflow, and a `VMAT_MAX_ELEMENTS` cap of `1 << 28` abort with a controlled diagnostic. Clone, zeros, identity, Kron, det, LU solve, mat-vec, inverse, diag, col, eigen, and SVD paths were audited. SVD wrappers reject `m < n` before allocating `n * n`, so a wide matrix returns a shape error instead of aborting. | `mat_zeros_overflow.valvet` and `mat_zeros_too_large.valvet` abort with exit -1 instead of segfaulting. `mat_svd_wide.valvet` returns the expected shape error for a 1 x 100000 matrix. `matrix_shapes_use_checked_helpers` checks the generated C. |
| VLV-003 | The parser has a `MAX_PARSE_DEPTH` budget of 64 and reports `E0006` from `with_depth`. The guard covers expressions, types, patterns, statements, blocks, and prefix operators. The limit is low enough for the 2 MiB stack used by Rust test threads in debug builds. | Three front-end depth tests, a CLI child-process test, and `fuzz_deep_nesting_is_bounded` cover parentheses, types, unary operators, and nested blocks. |
| VLV-004 | Under `VALVET_HERMETIC`, sema rejects `read_file`, `read_file_limit`, `write_file`, and `sys_cwd` with `E0401`, in addition to the existing subprocess and environment denials. The flag remains a compile-time policy gate, not an OS sandbox. | `hermetic_denies_file_io_and_cwd` checks all four denials and confirms `read_file` still works without the flag. `read_file_limit_and_bytes_to_string_are_bounded` checks byte limits, negative limits, byte range checks, and invalid lengths. |
| VLV-005 | `std::` imports resolve only from ordered stdlib roots by default. A project-local `std/` tree requires the explicit `--allow-stdlib-shadow` opt-in, and hermetic runs ignore the opt-in. | `stdlib_shadowing_requires_explicit_opt_in` checks default rejection, opt-in acceptance, and hermetic rejection. |
| VLV-006 | `c_escape` emits `\?` for every question mark, so `??/` cannot form a C trigraph. | `string_literals_preserve_nul_and_escape_trigraphs` and the `string_nul_trigraph.valvet` fixture. |
| VLV-007 | User string literals use the length-aware `vstr_from_bytes` helper and fixed-width three-digit octal escapes for control bytes. Embedded NUL bytes keep their length and value. | The same codegen test and fixture assert length 3 and bytes 97, 0, 98 for `"a\0b"`. |

Validation performed on the working tree:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`: 223 front-end tests, 9 codegen tests, 3 fuzz tests, 5 CLI security tests, and 6 core unit tests, 0 failed.
- `./target/debug/valvet test valvet-tests`: 432 passed, 0 failed.
- `./target/release/valvet test valvet-tests`: 432 passed, 0 failed.
- `./target/debug/valvet test --doc` and the release driver: 17 passed, 0 failed.
- `python3 scripts/fuzz_binary_formats.py`: 266 deterministic mutation cases, 0 crashes or hangs.
- ASan/UBSan/LSan through a GCC wrapper: clean for 88 typed-frame, grouped, join, window, Parquet, Arrow, bytes, Snappy, and lazy-plan fixtures, with output identical to each `.stdout` sidecar. Clean for `string_nul_trigraph`, `mat_basic`, and `mat_linalg`.

Residual risk and follow-up:

- `--hermetic` still does not sandbox the network or other host capabilities. It is not a replacement for containers, namespaces, seccomp, or Landlock.
- The matrix cap of `1 << 28` elements is a denial-of-service bound, not a proof that every possible allocation will succeed. Allocation failure still aborts.
- The parser depth limit of 64 is deliberately conservative. Valid programs that need deeper nesting will receive `E0006`.
- CI still does not run a dedicated sanitizer job. Local ASan/UBSan/LSan runs were clean for the 88 typed/binary/lazy fixtures and the earlier security regression set through a GCC sanitizer wrapper.
- No commit, tag, push, or publish was performed as part of this remediation.

## 17.1 Typed-frame and binary-format review (2026-10-26)

The typed-frame, Parquet/Arrow, and lazy-plan work was reviewed as a local,
non-destructive extension of the same security baseline.

Controls and evidence:

- Binary readers use `read_file_limit` and `bytes_to_string`; negative
  limits, oversized files, out-of-range bytes, and invalid lengths return
  `Err`. `--hermetic` denies `read_file_limit`.
- Parquet and Arrow readers validate lengths, offsets, counts, and
  FlatBuffers/Thrift nesting before indexing. Malformed, truncated, nested,
  dictionary-encoded, and unsupported inputs return controlled errors.
- Writers emit one row group or one record batch and write only to the
  caller-provided output path. They do not execute external tools.
- `scripts/fuzz_binary_formats.py` mutates every checked-in binary fixture
  in a fixed order. The current smoke run covers 266 cases with no signal,
  timeout, or non-zero exit.
- ASan/UBSan/LSan is clean on the 88 typed/binary/lazy fixtures listed in
  `docs/implementation/TEST_STRATEGY.md`, with stdout byte-exact.
- `valvet-tests/data/README.md` records the generator scripts, byte sizes,
  and SHA256 values for every binary fixture.

Residual risk:

- The readers implement narrow subsets. Unsupported but otherwise valid
  Parquet/Arrow files may fail with `Err`; this is a compatibility limit,
  not a memory-safety claim.
- The fuzz smoke is deterministic mutation testing, not coverage-guided
  fuzzing. It does not prove the absence of parser bugs.
- VLV-008 through VLV-015 remain open candidate, process, or design items.
  They are not remediated by the typed-frame work: git dependency option
  injection, vendor/package path traversal, lockfile bypass, C toolchain
  compromise, missing sanitizer/fuzz CI, `std::sys` command execution,
  arbitrary file I/O, and environment/argv leakage under untrusted use.
