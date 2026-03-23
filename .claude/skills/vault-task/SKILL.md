---
name: vault-task
description: Create or manage tasks in the Obsidian vault using the obsidian CLI
user-invocable: true
allowed-tools: Bash, Read, Edit
---

# Manage Obsidian Vault Tasks

The Obsidian vault is at `vault/meshcore-rs/`. Tasks use checkbox format.

## Operations

### List open tasks
```bash
obsidian tasks todo vault=meshcore-rs
```

### List completed tasks
```bash
obsidian tasks done vault=meshcore-rs
```

### Create a new task
Append a checkbox line to the appropriate file:
```bash
obsidian append path="Tasks/Backlog.md" content="- [ ] $ARGUMENTS"
```

### Mark a task as done
```bash
obsidian task done path="Tasks/Backlog.md" line=N
```

### Move task to Current
When starting work on a backlog task, move it from Backlog.md to Current.md by editing both files.

## Arguments

Use $ARGUMENTS to determine the operation:
- If it starts with "list" or "show": list tasks
- If it starts with "done" followed by text: find and mark matching task as done
- Otherwise: create a new task with $ARGUMENTS as the description

## Tags

Use tags for categorization:
- `#core` `#radio` `#dispatch` `#mesh` `#serial` `#app` — crate-specific
- `#adr` — needs architecture decision
- `#bug` `#feature` `#refactor` — work type
