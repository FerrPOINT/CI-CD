FROM node:22-bookworm-slim AS build
WORKDIR /app
COPY CI-CD/openapi/openapi.yaml /workspace/openapi/openapi.yaml
COPY CI-CD/frontend/package.json CI-CD/frontend/pnpm-lock.yaml* CI-CD/frontend/pnpm-workspace.yaml ./
RUN corepack enable && SPEC=/workspace/openapi/openapi.yaml pnpm install --frozen-lockfile
COPY CI-CD/frontend/ ./
RUN pnpm build

FROM nginx:1.31.4-alpine
COPY CI-CD/frontend/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
