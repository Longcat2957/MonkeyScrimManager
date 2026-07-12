# syntax=docker/dockerfile:1.7
# Build context: repository root.
FROM node:22-alpine AS dependencies
WORKDIR /app
RUN corepack enable

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN --mount=type=cache,id=monkeybot-pnpm-store,target=/pnpm-store \
    pnpm config set store-dir /pnpm-store \
 && pnpm install --frozen-lockfile

FROM dependencies AS build
COPY apps/web apps/web
RUN pnpm --filter @monkeybot/web build

FROM node:22-alpine AS runtime
WORKDIR /app
RUN corepack enable

ENV NODE_ENV=production
ENV HOST=0.0.0.0
ENV PORT=3002

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN --mount=type=cache,id=monkeybot-pnpm-store,target=/pnpm-store \
    pnpm config set store-dir /pnpm-store \
 && pnpm install --frozen-lockfile --prod --filter @monkeybot/web...

COPY --from=build /app/apps/web/build apps/web/build

WORKDIR /app/apps/web
EXPOSE 3002
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD wget -qO- http://127.0.0.1:3002/healthz >/dev/null 2>&1 || wget -qO- http://127.0.0.1:3002/ >/dev/null 2>&1 || exit 1
CMD ["node", "build"]

