---
name: delete-comment
description: Audit source comments, move durable decisions into ADRs, file issues for implementation shortcuts, and tighten Bowser's no-new-comments lint baseline. Use for /delete-comment or a source comment cleanup.
---

# Delete source comments

1. Use the requested paths. When the request leaves the scope open, use the Settings UI files listed in `ui/comment-rules.mjs`. From the repository root, run `node ui/comment-ratchet.mjs --inventory` for the working tree or append a Git revision for the original source. For other paths, use a parser or tokenizer for the language to find standalone, inline, and block comments. Exclude generated and vendored files from the source audit.
2. Classify each comment in context. Delete **Tautology** comments that restate the code. Verify **Stale** comments against the current implementation or history and delete them. For an **Excuse**, confirm the shortcut and its impact, search the repository's issue tracker for a matching issue, and file or link a follow-up with the location, impact, and acceptance criteria before deleting the comment. Move a **Decision Record** into an existing or new document in `docs/adr/`. Capture the affected code, relevant constraints, decision, and consequences. Consolidate comments about the same decision.
3. Preserve behavior while editing. Check compiler and tooling directives, licenses, token boundaries, and line terminators before removing comment syntax. Record any deferred comment and its reason. Keep an audit by original file and line with the category, action, and issue or ADR reference where applicable.
4. Run `node ui/comment-ratchet.mjs --prune` to delete unused Settings baseline entries. Keep the baseline at or below its previous counts. If the requested work introduces lint to a new scope, inventory its existing comments and baseline only the ones that remain after review. Keep enforcement in the repository's normal lint entry point.
5. From `ui/`, run `npm run build`, `npm run lint`, `npm run typecheck`, and the tests for the affected code. Check for generated changes and include the current `ui/dist.manifest` for changes under `ui/src/`. Report the comment count by category, issues, ADRs, and remaining allowances. Estimate lines saved using comment-only physical lines and count an inline comment as zero deleted lines when code still occupies that line.
