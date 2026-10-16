# Diktiax frontend (Next.js 16). NEXT_PUBLIC_API_BASE is inlined into the browser bundle at
# build time, so it must be the backend URL as the BROWSER reaches it, passed as a build arg.
# Dependencies are installed with bun (bun.lock is the lockfile); build and serve run on Node,
# because `next build` crashed under the Bun runtime in the Docker build.
FROM oven/bun:1.3.14-slim AS deps
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

FROM node:22-slim AS builder
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
ARG NEXT_PUBLIC_API_BASE=http://localhost:3001
ENV NEXT_PUBLIC_API_BASE=$NEXT_PUBLIC_API_BASE NEXT_TELEMETRY_DISABLED=1
RUN node node_modules/next/dist/bin/next build

FROM node:22-slim
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000 HOSTNAME=0.0.0.0
COPY --from=builder --chown=node:node /app/package.json /app/next.config.ts ./
COPY --from=builder --chown=node:node /app/node_modules ./node_modules
COPY --from=builder --chown=node:node /app/.next ./.next
COPY --from=builder --chown=node:node /app/public ./public
USER node
EXPOSE 3000
CMD ["node", "node_modules/next/dist/bin/next", "start"]
