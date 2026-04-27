FROM node:22-alpine AS builder

# Optional npm registry override (homelab CI). Empty default = npmjs.org.
#   NPM_REGISTRY=https://npm.girok.dev/ → Verdaccio pull-through cache
ARG NPM_REGISTRY=""
ENV NPM_CONFIG_REGISTRY=${NPM_REGISTRY:-https://registry.npmjs.org/}

WORKDIR /app
COPY web/package.json web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci
COPY web/ .
# Copy crates bindings referenced by ts-rs generated types
COPY crates/veronex/bindings/ /crates/veronex/bindings/

ARG NEXT_PUBLIC_VERONEX_API_URL=http://localhost:3001
ARG NEXT_PUBLIC_VERONEX_ADMIN_KEY=
ENV NEXT_PUBLIC_VERONEX_API_URL=$NEXT_PUBLIC_VERONEX_API_URL
ENV NEXT_PUBLIC_VERONEX_ADMIN_KEY=$NEXT_PUBLIC_VERONEX_ADMIN_KEY

RUN --mount=type=cache,target=/app/.next/cache \
    npm run build

FROM node:22-alpine
WORKDIR /app
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
COPY --from=builder /app/public ./public
EXPOSE 3000
ENV NODE_OPTIONS="--max-http-header-size=32768"
CMD ["node", "server.js"]
