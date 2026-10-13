# syntax=docker/dockerfile:1.7
# Connecteurs mini8n (Oracle via node-oracledb en mode thin : open source, sans client Oracle).

FROM node:24-alpine AS deps
WORKDIR /app
COPY package.json package-lock.json ./
# --ignore-scripts : aucun script d'installation de dépendance n'est exécuté.
RUN --mount=type=cache,target=/root/.npm npm ci --omit=dev --ignore-scripts --no-audit --no-fund

# Exécution : Node seul. npm, npx, corepack et yarn (inutiles une fois les dépendances
# installées) sont retirés : moins d'outils, moins de vulnérabilités embarquées.
FROM node:24-alpine
RUN rm -rf /usr/local/lib/node_modules /usr/local/bin/npm /usr/local/bin/npx \
      /usr/local/bin/corepack /usr/local/bin/yarn /usr/local/bin/yarnpkg /opt/yarn-*
WORKDIR /app
ENV NODE_ENV=production
COPY --from=deps /app/node_modules ./node_modules
COPY package.json ./
COPY src ./src
USER node
EXPOSE 8090
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q --spider http://127.0.0.1:8090/healthz || exit 1
CMD ["node", "src/server.js"]
