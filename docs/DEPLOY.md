# Deploy — everything on the team server (95.216.72.161)

The discovery backend (`:8001`), deep server (`:8000`), and Cyanite gateway (`:8080`) already run
there. Two things remain: **(1) run the Diversify backend** (Brief/Taste) on the server, and
**(2) build the frontend and serve the static files**.

Target layout:

```
95.216.72.161:8000  Deep server (Erkin)        — already running
95.216.72.161:8001  Discovery backend (Orkun)  — already running
95.216.72.161:8002  Diversify backend (NEW — deploy this)
95.216.72.161:8080  Cyanite caching gateway    — already running
95.216.72.161:80     nginx → serves the built frontend
```

## 1. Diversify backend on :8002

Copy the `mml-hackatune-26` repo to the server, then:

```bash
cd ~/mml-hackatune-26
python3 -m venv .venv && . .venv/bin/activate
pip install -r api/requirements.txt

# .env (never commit): real keys + route Cyanite through the gateway
cat > .env <<'EOF'
CYANITE_API_KEY=cyk__...
CYANITE_ACCOUNT=acc_...
GEMINI_API_KEY=AIza...
JAMENDO_CLIENT_ID=...
CYANITE_BASE_URL=http://127.0.0.1:8080/v1
EOF

python api/warm_cache.py          # pre-bake the 5 listeners (instant Taste)
```

Run it under systemd so it survives reboots — `/etc/systemd/system/diversify-api.service`:

```ini
[Unit]
Description=Diversify backend (Brief/Taste)
After=network.target

[Service]
User=ekin
WorkingDirectory=/home/ekin/mml-hackatune-26
ExecStart=/home/ekin/mml-hackatune-26/.venv/bin/uvicorn api.server:app --host 0.0.0.0 --port 8002
Restart=always

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now diversify-api
curl http://localhost:8002/api/health   # {"ok": true}
```

CORS is already `*` on this backend, so the browser can reach it cross-origin.

## 2. Build the frontend

`frontend/.env.production` already points the build at the server (public URLs, no secrets):

```
VITE_DIVERSIFY_API=http://95.216.72.161:8002/api
VITE_ORKUN_API=http://95.216.72.161:8001/api
VITE_ERKIN_API=http://95.216.72.161:8000
```

```bash
cd frontend
npm install
npm run build        # outputs frontend/dist/
```

## 3. Serve the static frontend (nginx)

```nginx
server {
    listen 80;
    server_name 95.216.72.161;
    root /home/ekin/diversify/frontend/dist;
    index index.html;
    location / { try_files $uri /index.html; }   # SPA fallback
}
```

```bash
sudo nginx -t && sudo systemctl reload nginx
```

The app is now live at **http://95.216.72.161/**.

## Smoke test (after deploy)

- `http://95.216.72.161/` loads, sidebar shows five tabs.
- **Brief** → search returns a pitch list (hits `:8002`).
- **Taste** → pick a listener → persona + card + stream (hits `:8002`).
- **Chat / Similar / MoodBoard** → results (hit `:8001`).
- Click a track → detail modal shows the spectrogram (hits `:8000`).

## Notes

- All traffic is plain HTTP. If you add a domain + TLS later, every backend URL in
  `.env.production` must become `https://` too (no mixed content) — put nginx in front of each
  backend or terminate TLS centrally.
- To rebuild after a frontend change: `npm run build` and reload nginx (static files only).
- To force fresh search results after a catalog change: delete `mml-hackatune-26/app/.cache/`.
