FROM node:22-alpine AS builder
WORKDIR /app
COPY package.json ./
RUN npm install
COPY . .

# These must be available at build time so Next.js bakes them into the client bundle
ARG NEXT_PUBLIC_VERONEX_API_URL=http://localhost:3001
ARG NEXT_PUBLIC_VERONEX_ADMIN_KEY=
ENV NEXT_PUBLIC_VERONEX_API_URL=$NEXT_PUBLIC_VERONEX_API_URL
ENV NEXT_PUBLIC_VERONEX_ADMIN_KEY=$NEXT_PUBLIC_VERONEX_ADMIN_KEY

RUN npm run build

FROM node:22-alpine
WORKDIR /app
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
COPY --from=builder /app/public ./public
EXPOSE 3002
CMD ["node", "server.js"]
