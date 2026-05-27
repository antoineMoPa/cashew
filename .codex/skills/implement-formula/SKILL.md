---
name: implement-formula
description: Implement a new Cashew spreadsheet formula in this repo. Use when Codex needs to pick the next roadmap formula, add backend parsing/evaluation and formula docs, delegate implementation to a gpt-5.4-mini sub-agent, run thermonuclearcodequalityreview on the resulting diff, run tests, and remove the completed roadmap item.
---

# Implement Formula

Implement one roadmap formula end to end. Keep the work scoped to the easiest unfinished formula unless the easy list is empty, then move to medium, then hard.

## Workflow

1. Read `ROADMAP.md`.
2. Pick the first reasonable formula from `## Formulas` in this order:
   - `Easy`
   - `Medium`
   - `Hard`
3. Prefer formulas that fit the current architecture:
   - Single-cell return values are easier than multi-cell output ranges.
   - Local parsing/evaluation formulas are easier than new provider integrations.
   - Avoid formulas that require redesigning document mutation unless no simpler item remains.
4. Inspect the existing implementation surface before changing code:
   - `src/backend/formulas.rs`
   - `src/backend/formula_implementations.rs`
   - `src/backend/document.rs`
   - `src/frontend/components/bottom_panel.rs`
   - any provider module if the formula needs external requests

## Delegation

Use sub-agents only because this workflow explicitly requires them.

### Implementation agent

Spawn one `worker` agent with:
- model: `gpt-5.4-mini`
- ownership: the formula implementation files it edits
- instruction to edit files directly in its forked workspace
- instruction that it is not alone in the codebase and must not revert unrelated changes

Implementation scope should usually include:
- formula catalog entry in `src/backend/formulas.rs`
- evaluation/parsing in `src/backend/formula_implementations.rs`
- tests in the touched backend file
- roadmap update in `ROADMAP.md` only if the implementation is complete

Give the worker a concrete formula name and acceptance criteria. Require it to list changed files in its final answer.

### Review step

After implementation is ready, run `thermonuclearcodequalityreview` on the resulting diff. Use it as the review gate for bugs, regressions, and missing tests. Keep the input generic and do not pre-brief it with your own conclusions.

## Local integration rules

1. Review the worker output and inspect the changed files yourself.
2. Fix issues locally if the worker missed something.
3. Run targeted tests first when possible, then `cargo test -q`.
4. Update `ROADMAP.md` to remove the completed formula from its list only after tests pass.
5. Summarize:
   - chosen roadmap item
   - files changed
   - test results
   - `thermonuclearcodequalityreview` findings and follow-up fixes

## Repo-specific guidance

- Avoid normalization functions. Prefer explicit mapping functions, enums, or dedicated parsers.
- Keep frontend and backend concerns separated.
- Preserve the existing formula model: docs live in `src/backend/formulas.rs`, evaluation lives in `src/backend/formula_implementations.rs`.
- Add tests with the implementation. Coverage should include the happy path and at least one behavior edge relevant to the formula.
- If a roadmap item implies multi-cell output ranges or document-wide mutation, call that out as a complexity increase before choosing it over a simpler alternative.
