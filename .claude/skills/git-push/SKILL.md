---
name: git-push
description: Commit and push changes to git repository. Use when the user asks to commit changes, finalize work, or push to remote.
allowed-tools: Bash(git status:*), Bash(git add:*), Bash(git commit:*), Bash(git push:*), Bash(git diff:*), Bash(git log:*)
---

# Git Commit and Push

Commits staged and unstaged changes, then pushes to the remote repository.

## Instructions

When the user asks to commit and push:

1. **Review changes**:
   - Run `git status` to see modified files
   - Run `git diff` to see unstaged changes
   - Run `git diff --staged` to see staged changes

2. **Generate commit message**:
   - Use present tense, imperative mood ("Add feature", not "Added feature")
   - Be clear and descriptive about what changed
   - Follow format: `<type>: <description>` or `<type>(<scope>): <description>`
   - Common types: feat, fix, refactor, test, docs, chore
   - Add the Claude Code attribution footer (see examples below)

3. **Stage files** (if needed):
   - Run `git add .` to stage all changes, OR
   - Run `git add <specific-files>` for selective staging
   - Ask user if uncertain about what to stage

4. **Create commit**:
   - Use heredoc format for multi-line commit messages
   - Include the required attribution footer

5. **Push to remote**:
   - Run `git push` to push to current branch
   - If branch doesn't track remote yet, use `git push -u origin <branch>`
   - Show result to user

6. **Verify**:
   - Show `git log --oneline -1` to confirm commit
   - Report push status to user

## Commit Message Format

```
<type>: <concise description>

<optional detailed explanation>

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>
```

## Examples

### Example 1: Simple commit
```bash
git add .
git commit -m "$(cat <<'EOF'
Add compact recipients compression

Reduces transaction size from 41 to 13 bytes (68% reduction) by using
4-byte recipient account IDs instead of 32-byte public keys for
recipients with existing accounts.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>
EOF
)"
git push
```

### Example 2: Feature commit with scope
```bash
git add crates/coins-types/src/lib.rs crates/coins-types/tests/
git commit -m "$(cat <<'EOF'
feat(types): implement hybrid transaction compression

- Add CompactTransaction (13 bytes)
- Add SubBlockState trait for state access
- Support both compact and canonical formats

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>
EOF
)"
git push
```

### Example 3: Bug fix
```bash
git add crates/coins-publisher/src/engine.rs
git commit -m "$(cat <<'EOF'
fix(publisher): resolve IBD deserialization failure

Publisher PK must remain uncompressed to avoid bootstrapping issues
during initial block download.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>
EOF
)"
git push
```

## Important Notes

- **NEVER force push** unless explicitly requested by user
- **Check for uncommitted changes** before starting
- **Ask user to confirm** if commit message is complex or if many files changed
- **Follow existing conventions** in the repository's commit history
- **Always include the Claude Code attribution footer**
