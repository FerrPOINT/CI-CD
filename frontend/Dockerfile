FROM node:22-bookworm-slim AS build
WORKDIR /app
COPY ../openapi/openapi.yaml /workspace/openapi/openapi.yaml
COPY package.json pnpm-lock.yaml* pnpm-workspace.yaml ./
RUN corepack enable && SPEC=/workspace/openapi/openapi.yaml pnpm install --frozen-lockfile
COPY . .
RUN pnpm build

FROM nginx:1.31.4-alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
