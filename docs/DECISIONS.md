# Decision log

ADR-style record of the choices behind the merged app and *why*.

### D1 — Hybrid: Cyanite recommends, the audio layer validates & extends
Cyanite is the recommendation engine for the product paths (Brief/Taste). The self-built audio
layer (Discovery + Deep) has two non-competing jobs: **acoustic similarity** for the ~40k tracks
Cyanite hasn't tagged (Similar by audio), and **validation** (audio↔tag grounding, spectrograms)
that audits Cyanite rather than replacing it.
*Why:* keeps the cardinal rule intact (Cyanite-central, no collaborative filtering shipped) while
still using the team's full audio engine. Confirmed acceptable with the organizers.

### D2 — One frontend, three backends behind it (not one rewritten backend)
The frontend routes each feature to the backend that owns it via build-time env vars.
*Why:* all three backends already work; unifying them into one codebase costs more than it buys
in a hackathon. *Trade-off:* the app needs all three reachable.

### D3 — Adopt the React/TS shell; rebuild the product tabs on it
Brief and Taste were ported into the TypeScript shell as new screens (`apiDiversify.ts`,
`BriefPage`, `DiversifyTastePage`); Chat/Similar/MoodBoard are the existing screens repointed at
their live backend. *Why:* one polished shell, minimal rewrite of working code.

### D4 — LLM (Gemini) is a translator, never the recommender
Gemini turns a brief, a conversation, or an image into a Cyanite query + filter. It never ranks
or invents tracks. *Why:* keeps retrieval Cyanite-driven and the system explainable.

### D5 — Route Cyanite through the caching gateway
The Diversify backend points at the team's typed caching gateway instead of Cyanite directly.
*Why:* cached repeats (fast + quota-safe), one shared server-side key, request validation.

### D6 — Resolve relative audio URLs to the backend origin
The discovery backend returns audio as a relative `/audio/...` path (works same-origin on the
server, not in local dev). `api.ts:audioSrc()` rewrites it to the backend origin.
*Why:* playback must work in dev and prod; this was the "tracks won't play" bug.

### D7 — Cover art via a backend proxy, not the browser
`art.ts` fetches covers from the Diversify backend's `/api/artwork/{jid}`, which calls the Jamendo
API server-side. *Why:* the browser can't call Jamendo's API directly (CORS); the proxy caches
and degrades to a gradient thumb on failure (artwork is cosmetic).

### D8 — Disk-cache search responses
Brief/Chat/Image responses are cached on the Diversify backend keyed on inputs; Taste streams too.
*Why:* repeated searches (the demo's reality) return instantly. *Trade-off:* delete the cache to
force fresh results after catalog changes.

### D9 — Hash-based routing for linkable, openable features
Each tab has a `#hash` URL; the app reads it on load. *Why:* enables "open in a new tab" per
feature, browser back/forward, and shareable links — without adding a router dependency.

### D10 — Drop "Insights" and "Similar by track name"
Insights (learned-weights viz) and the track-name search were cut. *Why:* five focused features
with a clear narrative beat a feature tour ("clarity of concept beats feature count").
