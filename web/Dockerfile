# syntax=docker/dockerfile:1.7
# Interface mini8n : build Vite puis service statique par nginx non-root (+ reverse proxy /api).
# Contexte de build : web/. Contexte additionnel « engine » : image produite par crates/engine/Dockerfile
# (fourni automatiquement par docker compose).

FROM node:24-alpine AS build
WORKDIR /app
COPY package.json package-lock.json ./
# --ignore-scripts : aucun script d'installation de dépendance n'est exécuté.
RUN --mount=type=cache,target=/root/.npm \
    npm ci --ignore-scripts --no-audit --no-fund
COPY . .
COPY --from=engine /pkg ./src/lib/engine/pkg
RUN npx svelte-check --tsconfig ./tsconfig.app.json --fail-on-warnings \
 && npx vite build

FROM nginxinc/nginx-unprivileged:1.30-alpine
USER root
RUN rm -f /etc/nginx/conf.d/default.conf /docker-entrypoint.d/10-listen-on-ipv6-by-default.sh
COPY nginx/nginx.conf /etc/nginx/nginx.conf
COPY nginx/templates /etc/nginx/templates
COPY --from=build /app/dist /usr/share/nginx/html
RUN chmod -R a-w /usr/share/nginx/html /etc/nginx
USER 101
# La configuration générée (CSP…) est écrite dans /tmp : le reste du système peut être en lecture seule.
ENV NGINX_ENVSUBST_OUTPUT_DIR=/tmp \
    CSP_CONNECT_SRC="https:"
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q --spider http://127.0.0.1:8080/healthz || exit 1
