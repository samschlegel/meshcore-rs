---
name: new-crate
description: Add a new no_std crate to the meshcore-rs workspace
user-invocable: true
allowed-tools: Read, Write, Edit, Bash, Glob
---

# Add a New Crate to the Workspace

## Steps

1. Determine the crate name from $ARGUMENTS (e.g., `/new-crate meshcore-crypto`)

2. Create the crate directory and files:

   **`crates/{name}/Cargo.toml`:**
   ```toml
   [package]
   name = "{name}"
   version = "0.1.0"
   edition.workspace = true
   license.workspace = true

   [features]
   default = []
   std = []

   [dependencies]
   heapless.workspace = true
   defmt.workspace = true
   ```

   **`crates/{name}/src/lib.rs`:**
   ```rust
   #![no_std]
   #![deny(unsafe_code)]
   ```

3. Add the crate to the workspace members in the root `Cargo.toml`:
   Read the current Cargo.toml, add `"crates/{name}"` to the `members` array.

4. Verify the workspace compiles:
   ```bash
   cargo check --workspace
   ```

5. If the check fails, diagnose and fix. If it passes, confirm success.

## Notes
- All library crates are `#![no_std]` by default
- Use workspace dependencies where possible
- Crate names follow the pattern `meshcore-{layer}`
