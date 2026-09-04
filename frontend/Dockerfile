FROM node:22-bookworm-slim AS build
WORKDIR /app
COPY openapi/openapi.yaml /workspace/openapi/openapi.yaml
COPY frontend/package.json frontend/pnpm-lock.yaml* frontend/pnpm-workspace.yaml ./
RUN corepack enable && SPEC=/workspace/openapi/openapi.yaml pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM nginx:1.31.4-alpine
COPY frontend/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
