# Desktop Regression Gate Evidence

Run from repository root: `/Users/wingwogus/orca/workspaces/hanbeon/feat-android-tauri`

## 1. Workspace tests

Command:
```sh
PATH=$HOME/.cargo/bin:$PATH cargo test --workspace 2>&1 | tail -15
```

Exit code: `0`
Verdict: **PASS**

Last output lines:
```text
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests hanbeon_lib

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests hanbeon_core

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## 2. Workspace Clippy

Command:
```sh
PATH=$HOME/.cargo/bin:$PATH cargo clippy --workspace -- -D warnings 2>&1 | tail -10
```

Exit code: `0`
Verdict: **PASS**

Last output lines:
```text
    Checking window-vibrancy v0.6.0
    Checking global-hotkey v0.8.0
    Checking enigo v0.6.1
    Checking tray-icon v0.24.2
    Checking tauri-runtime v2.11.3
    Checking wry v0.55.1
    Checking tauri-runtime-wry v2.11.4
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 10s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use `cargo report future-incompatibilities --id 1`
```

## 3. Typecheck

Command:
```sh
bun run typecheck 2>&1 | tail -5
```

Exit code: `0`
Verdict: **PASS**

Last output lines:
```text
$ tsc --noEmit -p apps/desktop/tsconfig.json && tsc --noEmit -p apps/admin/tsconfig.json
```

## 4. Bun tests

Command:
```sh
bun run test --bail 1 2>&1 | tail -10
```

Exit code: `1`
Verdict: **FAIL**

Last output lines:
```text
$ BUN_RUNTIME_TRANSPILER_CACHE_PATH=0 bun test --bail "1"
bun test v1.3.14 (0d9b296a)
The following filters did not match any test files:
 1
12230 files were searched [113.00ms]

note: Tests need ".test", "_test_", ".spec" or "_spec_" in the filename (ex: "MyApp.test.ts")

Learn more about bun test: https://bun.com/docs/cli/test
error: script "test" exited with code 1
```

## 5. Desktop package build

Command:
```sh
PATH=$HOME/.cargo/bin:$PATH cargo build -p hanbeon 2>&1 | tail -3
```

Exit code: `0`
Verdict: **PASS**

Last output lines:
```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 43.18s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use `cargo report future-incompatibilities --id 1`
```

# GATE: FAIL

Overall: 4 of 5 commands passed. The Bun test command failed because the `1` filter matched no test files.
