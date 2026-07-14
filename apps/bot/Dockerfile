# syntax=docker/dockerfile:1.7
# Build context: repository root.
FROM node:22-alpine AS dependencies
WORKDIR /app
RUN corepack enable

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/bot/package.json apps/bot/package.json
RUN --mount=type=cache,id=monkeybot-pnpm-store,target=/pnpm-store \
    pnpm config set store-dir /pnpm-store \
 && pnpm install --frozen-lockfile

FROM dependencies AS build
COPY apps/bot apps/bot
RUN pnpm --filter @monkeybot/bot build

FROM node:22-alpine AS runtime
WORKDIR /app
RUN corepack enable \
 && apk add --no-cache font-noto-cjk

ENV NODE_ENV=production
ENV MONKEYBOT_BOT_HEALTH_ADDR=0.0.0.0:8081

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/bot/package.json apps/bot/package.json
RUN --mount=type=cache,id=monkeybot-pnpm-store,target=/pnpm-store \
    pnpm config set store-dir /pnpm-store \
 && pnpm install --frozen-lockfile --prod --filter @monkeybot/bot...

COPY --from=build --chown=node:node /app/apps/bot/dist apps/bot/dist

WORKDIR /app/apps/bot
USER node
EXPOSE 8081
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=3 \
  CMD wget -qO- http://127.0.0.1:8081/healthz >/dev/null 2>&1 || exit 1
CMD ["node", "dist/index.js"]
