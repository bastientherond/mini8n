# syntax=docker/dockerfile:1.7
# Connecteurs mini8n (Oracle via node-oracledb en mode thin : open source, sans client Oracle).
FROM node:24-alpine
WORKDIR /app
ENV NODE_ENV=production
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --omit=dev --ignore-scripts --no-audit --no-fund
COPY src ./src
USER node
EXPOSE 8090
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q --spider http://127.0.0.1:8090/healthz || exit 1
CMD ["node", "src/server.js"]
