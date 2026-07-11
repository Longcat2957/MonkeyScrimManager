# Codex Working Rules

## Dev Server

- Codex must not start the frontend dev server.
- Another shell is already responsible for keeping the dev environment running and visually checking changes.
- Do not run `pnpm dev`, `pnpm run dev`, `pnpm --filter @monkeybot/web dev`, or equivalent Vite/SvelteKit dev-server commands unless the user explicitly overrides this rule in the current conversation.
- For validation, use non-server commands such as `pnpm check`, `pnpm lint`, `pnpm test`, and `pnpm build`.
- If runtime confirmation is needed, ask the user to check the already-running dev server instead of starting a new one.

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
