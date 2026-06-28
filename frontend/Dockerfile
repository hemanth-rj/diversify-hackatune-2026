# Build the Vite app, then serve the static output with nginx.
FROM node:20-alpine AS build
WORKDIR /app

# Backend URLs are baked in at build time (Vite). Defaults work for local docker-compose
# (browser reaches the backend on localhost:8000) + the hosted discovery/deep services.
ARG VITE_DIVERSIFY_API=http://localhost:8000/api
ARG VITE_ORKUN_API=http://95.216.72.161:8001/api
ARG VITE_ERKIN_API=http://95.216.72.161:8000
ENV VITE_DIVERSIFY_API=$VITE_DIVERSIFY_API \
    VITE_ORKUN_API=$VITE_ORKUN_API \
    VITE_ERKIN_API=$VITE_ERKIN_API

COPY package*.json ./
RUN npm install
COPY . .
RUN npm run build

FROM nginx:alpine
COPY --from=build /app/dist /usr/share/nginx/html
# SPA fallback so deep links (#hash) work
RUN printf 'server { listen 80; root /usr/share/nginx/html; location / { try_files $uri /index.html; } }' \
    > /etc/nginx/conf.d/default.conf
EXPOSE 80
