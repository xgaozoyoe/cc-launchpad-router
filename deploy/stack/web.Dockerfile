FROM node:24-bookworm-slim AS build
WORKDIR /app
COPY apps/launchpad/package*.json ./
RUN npm ci
COPY apps/launchpad ./
RUN npm run build
FROM node:24-bookworm-slim
ENV NODE_ENV=production BIND_HOST=0.0.0.0 PORT=4173 STATIC_DIR=/app/dist
WORKDIR /app
COPY --from=build /app/package*.json ./
RUN npm ci --omit=dev && chown -R 10001:10001 /app
COPY --from=build /app/dist ./dist
COPY --from=build /app/.server-check/server ./server
USER 10001
ENTRYPOINT ["node","server/standalone.js"]
