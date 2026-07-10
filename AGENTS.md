# Codex Working Rules

## Dev Server

- Codex must not start the frontend dev server.
- Another shell is already responsible for keeping the dev environment running and visually checking changes.
- Do not run `pnpm dev`, `pnpm run dev`, `pnpm --filter @monkeyscrim/web dev`, or equivalent Vite/SvelteKit dev-server commands unless the user explicitly overrides this rule in the current conversation.
- For validation, use non-server commands such as `pnpm check`, `pnpm lint`, `pnpm test`, and `pnpm build`.
- If runtime confirmation is needed, ask the user to check the already-running dev server instead of starting a new one.

## Requirement Discipline

- Follow the user's requested structure and scope directly. Do not replace it with an unrelated interpretation, redesign, or broader product idea.
- When the user points to an existing reference such as `mookbot_v2`, inspect and preserve the useful interaction model before changing local components.
- Do not invent a new layout when the user asked to reuse or modernize an existing pattern. Modernization means adapting spacing, color, and component syntax while keeping the reference workflow recognizable.
- If a request says to place a feature in a component sample page, do not wire it into production routes unless explicitly asked.
- When the user asks to move or list completed components on a page, use the exact page the user names. Do not assume `/auction/a-118` or any other default destination.
- `/dev/components` is only for the component currently being developed or inspected. Keep completed components on the user-specified QA page unless the user explicitly asks to move or remove them.
- If the instruction is ambiguous, choose the smallest implementation that matches the literal request and existing local patterns.
- Avoid arguing through implementation. If the user rejects a direction, remove or replace that direction instead of layering more UI around it.
