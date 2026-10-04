# CLAUDE.md — liman

Product code for **liman** (MIT). Research, decisions (ADRs), roadmap and process rules live in the
`fm-research` repo (`~/Projects/fm-research`): read its `CLAUDE.md` and `STATUS.md` before working here.

Key rules (full versions in fm-research):
- Talk to the user in **Turkish**; code, comments and commit messages in English.
- **Commits and PRs carry no Claude attribution** (no `Co-Authored-By`, no "Generated with"). Strict rule.
- Claude writes the code in small, working, separately committed steps; the user reviews later.
- `liman-core` stays UI-independent (no ratatui). No GPL/LGPL code or dependencies (`cargo deny check`).
- Design decisions: ADR 0002 (half-block icons), 0003 (detailed / normal / grid views), 0004 (scope, target user).
