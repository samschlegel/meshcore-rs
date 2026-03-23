---
name: verify-build
description: Run the full verification checklist for meshcore-rs (check, clippy, test, cross-compile)
user-invocable: true
allowed-tools: Bash
---

# Run Verification Checklist

Execute the meshcore-rs verification steps in order. Stop and report on the first failure.

## Steps

1. **Workspace check:**
   ```bash
   cargo check --workspace
   ```

2. **Clippy (strict):**
   ```bash
   cargo clippy --workspace -- -D warnings
   ```

3. **Tests:**
   ```bash
   cargo test --workspace
   ```

4. **Cross-compile ESP32** (only if boards/esp32 exists):
   ```bash
   cargo build -p meshcore-esp32 --target xtensa-esp32s3-none-elf
   ```

5. **Cross-compile nRF52840** (only if boards/nrf52840 exists):
   ```bash
   cargo build -p meshcore-nrf52840 --target thumbv7em-none-eabihf
   ```

## Output

Report pass/fail for each step. If a step fails, include the error output and suggest fixes. Do not continue past a failure unless the user asks.
