---
name: adr-create
description: Create a new Architecture Decision Record (ADR) in docs/decisions/ using MADR format
user-invocable: true
allowed-tools: Read, Write, Glob, Bash
---

# Create a New ADR

When invoked, create a new Architecture Decision Record.

## Steps

1. Find the next ADR number by listing existing files in `docs/decisions/`:
   ```bash
   ls docs/decisions/[0-9]*.md 2>/dev/null | sort -n | tail -1
   ```
   Extract the number and increment by 1. If no ADRs exist, start at 0001.

2. Read the template at `docs/decisions/template.md`

3. Ask the user for:
   - Short title (will become the filename and heading)
   - Context and problem statement
   - Decision drivers
   - Considered options (at least 2)
   - Chosen option and justification

4. Create the file at `docs/decisions/NNNN-short-title.md` using the template, with:
   - Status: proposed
   - Date: today's date (YYYY-MM-DD)
   - All sections filled in from user input

5. Use $ARGUMENTS as the short title if provided (e.g., `/adr-create composable-serial-protocol`)

6. Confirm creation and print the file path
