# Codex Working Rules

## Dev Server

- Codex must not start the frontend dev server.
- Another shell is already responsible for keeping the dev environment running and visually checking changes.
- Do not run `pnpm dev`, `pnpm run dev`, `pnpm --filter @monkeybot/web dev`, or equivalent Vite/SvelteKit dev-server commands unless the user explicitly overrides this rule in the current conversation.
- For validation, use non-server commands such as `pnpm check`, `pnpm lint`, `pnpm test`, and `pnpm build`.
- If runtime confirmation is needed, ask the user to check the already-running dev server instead of starting a new one.

## Backend Runtime

- A backend route change is not complete while the local Axum process still serves the previous binary.
- A migration that renames or removes a column used by the running API is a maintenance migration, not a rolling migration. Stop every API instance before applying it, then start only the matching binary after migration and runtime grants succeed.
- After backend route work passes its tests, run `scripts/local-api-reload.sh '/api/<changed-route>'`.
- The reload script must pass readiness plus direct `30020` and same-origin `5173` proxy smoke checks. A `401`, `403`, or other route-level response is acceptable for an unauthenticated smoke request; `404`, connection failure, and `5xx` are failures.
- Do not report a backend endpoint as integrated before both smoke paths prove that the running router contains it.
- This rule does not permit starting or replacing the frontend dev server.

## Delivery Speed

- Follow [`docs/10-development-velocity.md`](docs/10-development-velocity.md) for implementation and validation scope.
- Before editing, reduce the request to a short acceptance list. Implement the smallest end-to-end slice that satisfies it; do not expand adjacent features during the same pass.
- Stop repository exploration once the target route, its direct dependencies, and the relevant business rule are identified. Batch independent reads and searches instead of issuing repeated single-file calls.
- Assign one owner per file. Use sub-agents only for independent implementation tracks or one final high-risk review, not overlapping investigations of the same code.
- Validate according to change risk. UI copy/style changes do not require backend or workspace-wide tests. Route-local changes use targeted tests first. Full workspace checks are reserved for migrations, shared foundations, release/commit gates, or an explicit user request.
- Run an unchanged expensive suite at most once after the final relevant edit. Do not stack targeted, package-wide, and workspace-wide versions of the same validation without a concrete reason.
- Integration fixtures must exercise documented branches, but reuse shared builders and the minimum rows needed. Do not grow a route test into a second reference implementation of the product.
- Update OpenAPI and narrative docs once the implementation contract is stable, not repeatedly during exploratory edits.
- For backend routes, runtime reload and direct/proxy smoke are part of the same delivery step; use the existing reload script instead of diagnosing a predictable stale-binary 404 afterward.

## Error Transparency

- Every user-visible failure must include a safe, concrete reason. Do not replace a known validation, permission, conflict, dependency, or transport failure with a generic failure sentence.
- Preserve the backend's public error message, stable error code, and request ID across frontend and Bot boundaries whenever they are available. The user should be able to understand what failed and provide a correlation ID for support.
- Unknown internal failures must still state the safe failure category and provide a request or interaction ID while the full diagnostic detail is recorded in server logs.
- Never expose stack traces, raw SQL or database errors, credentials, tokens, secrets, internal URLs, or unfiltered upstream response bodies in a user-facing error.
- Error-path tests must assert both the machine-readable reason and the user-visible explanation for important workflow failures.

## Requirement Discipline

- Decision priority is always:
  1. Direct instructions from the user in the current conversation.
  2. Project documents in this repository.
  3. Existing reference repositories such as `mookbot_v2` and `mookbot_v3`.
- Follow the user's requested structure and scope directly. Do not replace it with an unrelated interpretation, redesign, or broader product idea.
- Think from the business workflow first. Do not add UI, state, filters, metrics, helper components, or abstractions that do not directly serve the documented workflow.
- Write code from the project documents and named references. When implementing a route or workflow, read the relevant docs and reference implementation first instead of inventing behavior.
- If the business rule, target page, or expected interaction is unclear, ask the user before implementing. Do not fill gaps with guesses.
- Do not display duplicated information in multiple places just to fill the screen. Repeated counts, status labels, timestamps, or summary cards must have a clear workflow purpose.
- Prefer the smallest implementation that can be validated against the documented workflow. Avoid over-engineering and speculative component splits.
- When the user points to an existing reference such as `mookbot_v2`, inspect and preserve the useful interaction model before changing local components.
- Do not invent a new layout when the user asked to reuse or modernize an existing pattern. Modernization means adapting spacing, color, and component syntax while keeping the reference workflow recognizable.
- If a request says to place a feature in a component sample page, do not wire it into production routes unless explicitly asked.
- When the user asks to move or list completed components on a page, use the exact page the user names. Do not assume `/auction/a-118` or any other default destination.
- `/dev/components` is only for the component currently being developed or inspected. Keep completed components on the user-specified QA page unless the user explicitly asks to move or remove them.
- If the instruction is ambiguous, choose the smallest implementation that matches the literal request and existing local patterns.
- Avoid arguing through implementation. If the user rejects a direction, remove or replace that direction instead of layering more UI around it.
