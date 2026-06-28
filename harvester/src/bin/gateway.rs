//! `gateway` — a TYPED caching reverse-proxy that mirrors the Cyanite REST API.
//!
//! Friends point their Cyanite client base URL at this server instead of
//! `https://rest-api.cyanite.ai/v1`. The gateway is the canonical typed Cyanite
//! client (it supersedes the `sounds-like-you` crate):
//!   * every request is parsed into a typed model and **validated** — a malformed
//!     request is rejected locally (422) and NEVER spends a pooled-quota call;
//!   * valid requests are keyed and looked up in **Postgres** (single source of
//!     truth + shared cache); the real upstream is hit only on a MISS;
//!   * deterministic responses (2xx/3xx/4xx) are cached so a repeat — even a
//!     repeated *bad* request — never hits upstream twice; 429/5xx are transient
//!     (not cached, quota refunded);
//!   * one server-side `x-api-key` is injected; a client key is never trusted.
//!
//! Also serves the knowledge base: `/ontology` (tag vocabularies), `/library`
//! (docs), and `/stats`.

use anyhow::Result;
use axum::{
    body::{Body, Bytes},
    extract::{Path as AxPath, State},
    http::{header, Method, Response, StatusCode, Uri},
    response::Json,
    routing::{get, post},
    Router,
};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use harvester::cyanite::{self, SearchReq, SimilarMultiReq, SimilarReq, MODELS, UPSTREAM};
use nonzero_ext::nonzero;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Act {
    Search,
    Similar,
    Tag,
}
impl Act {
    fn name(self) -> &'static str {
        match self {
            Act::Search => "prompt_search",
            Act::Similar => "similarity",
            Act::Tag => "tagging",
        }
    }
    fn cap(self) -> i64 {
        match self {
            Act::Search | Act::Similar => 15_000,
            Act::Tag => 50_000,
        }
    }
}

struct App {
    pool: PgPool,
    http: reqwest::Client,
    key: String,
    rl_search: DefaultDirectRateLimiter,
    rl_similar: DefaultDirectRateLimiter,
    rl_tag: DefaultDirectRateLimiter,
}

impl App {
    fn limiter(&self, a: Act) -> &DefaultDirectRateLimiter {
        match a {
            Act::Search => &self.rl_search,
            Act::Similar => &self.rl_similar,
            Act::Tag => &self.rl_tag,
        }
    }

    async fn cache_get(&self, key: &str) -> Option<(i32, Option<String>, String)> {
        let row = sqlx::query_as::<_, (i32, Option<String>, String)>(
            "SELECT status, content_type, body FROM cyanite_cache WHERE cache_key=$1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        if row.is_some() {
            let _ = sqlx::query("UPDATE cyanite_cache SET hits=hits+1, last_hit=now() WHERE cache_key=$1")
                .bind(key).execute(&self.pool).await;
        }
        row
    }

    #[allow(clippy::too_many_arguments)]
    async fn cache_put(&self, key: &str, method: &str, path: &str, query: &str,
                       action: Option<Act>, status: i32, ctype: Option<&str>, body: &str) {
        let _ = sqlx::query(
            "INSERT INTO cyanite_cache(cache_key,method,path,query,action,status,content_type,body)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (cache_key) DO NOTHING",
        )
        .bind(key).bind(method).bind(path).bind(query)
        .bind(action.map(|a| a.name())).bind(status).bind(ctype).bind(body)
        .execute(&self.pool).await;
    }

    async fn quota_refund(&self, a: Act) {
        let _ = sqlx::query("UPDATE cyanite_quota SET used=GREATEST(used-1,0) WHERE action=$1")
            .bind(a.name()).execute(&self.pool).await;
    }

    /// Atomic, persistent pooled-quota debit. Returns false when exhausted.
    async fn quota_try(&self, a: Act) -> bool {
        let _ = sqlx::query("INSERT INTO cyanite_quota(action,used,cap) VALUES($1,0,$2) ON CONFLICT (action) DO NOTHING")
            .bind(a.name()).bind(a.cap()).execute(&self.pool).await;
        matches!(
            sqlx::query("UPDATE cyanite_quota SET used=used+1 WHERE action=$1 AND used+1<=cap")
                .bind(a.name()).execute(&self.pool).await,
            Ok(r) if r.rows_affected() == 1
        )
    }

    async fn forward(&self, method: &str, path: &str, query: &str, body: &[u8])
        -> Result<(u16, Option<String>, String)> {
        let mut url = format!("{UPSTREAM}/{path}");
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        let m = reqwest::Method::from_bytes(method.as_bytes())?;
        let mut rb = self.http.request(m, &url)
            .header("x-api-key", &self.key)
            .header(reqwest::header::ACCEPT, "application/json");
        if !body.is_empty() {
            rb = rb.header(reqwest::header::CONTENT_TYPE, "application/json").body(body.to_vec());
        }
        let resp = rb.send().await?;
        let status = resp.status().as_u16();
        let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()).map(str::to_string);
        Ok((status, ctype, resp.text().await?))
    }
}

fn cache_key(method: &str, path: &str, query: &str, body: &[u8]) -> String {
    let mut qs: Vec<&str> = query.split('&').filter(|s| !s.is_empty()).collect();
    qs.sort_unstable();
    let body_canon = serde_json::from_slice::<serde_json::Value>(body)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(body).into_owned());
    let raw = format!("{}\n{}\n{}\n{}", method.to_uppercase(), path, qs.join("&"), body_canon);
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

fn out(status: u16, ctype: &str, body: String, cache: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY))
        .header(header::CONTENT_TYPE, ctype)
        .header("x-cache", cache)
        .body(Body::from(body))
        .unwrap()
}

/// Locally-generated validation rejection — never touches upstream or quota.
fn reject(status: u16, detail: &str) -> Response<Body> {
    let b = json!({
        "type": "https://gateway/problems/validation",
        "title": "Validation problem (rejected by gateway — no upstream call)",
        "status": status, "detail": detail,
    }).to_string();
    out(status, "application/problem+json", b, "REJECTED")
}

fn parse_limit(query: &str) -> Result<Option<i64>, String> {
    for kv in query.split('&') {
        if let Some(v) = kv.strip_prefix("limit=") {
            return v.parse::<i64>().map(Some).map_err(|_| format!("limit '{v}' is not an integer"));
        }
    }
    Ok(None)
}

/// Shared cache-or-forward for an already-VALIDATED request.
async fn forward_cached(app: &App, method: &str, path: &str, query: &str, body: &[u8]) -> Response<Body> {
    let key = cache_key(method, path, query, body);
    if let Some((status, ctype, cbody)) = app.cache_get(&key).await {
        return out(status as u16, ctype.as_deref().unwrap_or("application/json"), cbody, "HIT");
    }
    let action = match () {
        _ if path.contains("/similar") => Some(Act::Similar),
        _ if path.contains("/search") => Some(Act::Search),
        _ if path.contains("/models") => Some(Act::Tag),
        _ => None,
    };
    if let Some(a) = action {
        app.limiter(a).until_ready().await;
        if !app.quota_try(a).await {
            return out(429, "application/json",
                json!({"error":"pooled event quota exhausted","action":a.name()}).to_string(), "QUOTA");
        }
    }
    match app.forward(method, path, query, body).await {
        Ok((status, ctype, text)) => {
            if status == 429 || status >= 500 {
                if let Some(a) = action {
                    app.quota_refund(a).await;
                }
            } else {
                app.cache_put(&key, method, path, query, action, status as i32, ctype.as_deref(), &text).await;
            }
            out(status, ctype.as_deref().unwrap_or("application/json"), text, "MISS")
        }
        Err(e) => {
            if let Some(a) = action {
                app.quota_refund(a).await;
            }
            out(502, "application/json", json!({"gateway_error": e.to_string()}).to_string(), "ERROR")
        }
    }
}

// ---------- typed endpoint handlers (validate → never let bad input reach upstream) ----------

async fn h_search(State(app): State<Arc<App>>, uri: Uri, body: Bytes) -> Response<Body> {
    let req: SearchReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return reject(422, &format!("invalid search body: {e}")),
    };
    if let Err(e) = req.validate() {
        return reject(422, &e);
    }
    let q = uri.query().unwrap_or("");
    match parse_limit(q).and_then(|l| cyanite::valid_limit(l).map_err(|e| e)) {
        Ok(()) => {}
        Err(e) => return reject(422, &e),
    }
    forward_cached(&app, "POST", "private-alpha/library-tracks/search", q, &body).await
}

async fn h_similar_one(State(app): State<Arc<App>>, AxPath(id): AxPath<String>, uri: Uri, body: Bytes) -> Response<Body> {
    if !cyanite::valid_id(&id) {
        return reject(422, "invalid library-track id in path (must match ^libtr_)");
    }
    let parsed: SimilarReq = if body.is_empty() {
        SimilarReq { metadata_filter: None }
    } else {
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => return reject(422, &format!("invalid similar body: {e}")),
        }
    };
    if let Err(e) = parsed.validate() {
        return reject(422, &e);
    }
    let q = uri.query().unwrap_or("");
    if let Err(e) = parse_limit(q).and_then(cyanite::valid_limit) {
        return reject(422, &e);
    }
    let fwd: Bytes = if body.is_empty() { Bytes::from_static(b"{}") } else { body };
    forward_cached(&app, "POST", &format!("private-alpha/library-tracks/{id}/similar"), q, &fwd).await
}

async fn h_similar_multi(State(app): State<Arc<App>>, uri: Uri, body: Bytes) -> Response<Body> {
    let req: SimilarMultiReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return reject(422, &format!("invalid multi-similar body: {e}")),
    };
    if let Err(e) = req.validate() {
        return reject(422, &e);
    }
    let q = uri.query().unwrap_or("");
    if let Err(e) = parse_limit(q).and_then(cyanite::valid_limit) {
        return reject(422, &e);
    }
    forward_cached(&app, "POST", "private-alpha/library-tracks/similar", q, &body).await
}

async fn h_models(State(app): State<Arc<App>>, AxPath(id): AxPath<String>, uri: Uri) -> Response<Body> {
    if !cyanite::valid_id(&id) {
        return reject(422, "invalid library-track id in path (must match ^libtr_)");
    }
    let q = uri.query().unwrap_or("");
    let models: Vec<&str> = q.split('&')
        .filter_map(|kv| kv.strip_prefix("model="))
        .collect();
    if models.is_empty() {
        return reject(422, "at least one ?model= is required");
    }
    for m in &models {
        if !MODELS.contains(m) {
            return reject(422, &format!("unknown model '{m}' (not one of the {} Cyanite models)", MODELS.len()));
        }
    }
    forward_cached(&app, "GET", &format!("library-tracks/{id}/models"), q, &[]).await
}

/// Any other path/method is NOT a real Cyanite endpoint — reject locally.
async fn h_fallback(method: Method, uri: Uri) -> Response<Body> {
    reject(404, &format!("{} {} is not a Cyanite endpoint", method, uri.path()))
}

// ---------- knowledge-base + management endpoints ----------

async fn stats(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let by_action = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT COALESCE(action,'other'), COUNT(*), COALESCE(SUM(hits),0)::bigint FROM cyanite_cache GROUP BY 1 ORDER BY 1",
    ).fetch_all(&app.pool).await.unwrap_or_default();
    let quota = sqlx::query_as::<_, (String, i64, i64)>("SELECT action, used, cap FROM cyanite_quota ORDER BY action")
        .fetch_all(&app.pool).await.unwrap_or_default();
    let (n, h): (i64, i64) = sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(hits),0)::bigint FROM cyanite_cache")
        .fetch_one(&app.pool).await.unwrap_or((0, 0));
    let ont: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ontology_term").fetch_one(&app.pool).await.unwrap_or(0);
    let docs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM doc").fetch_one(&app.pool).await.unwrap_or(0);
    Json(json!({
        "cached_responses": n, "cache_hits_served": h, "upstream_calls_saved": h,
        "by_action": by_action.iter().map(|(a,c,hh)| json!({"action":a,"upstream_calls":c,"cache_hits":hh})).collect::<Vec<_>>(),
        "quota": quota.iter().map(|(a,u,c)| json!({"action":a,"used":u,"cap":c,"remaining":c-u})).collect::<Vec<_>>(),
        "ontology_terms": ont, "documents": docs, "upstream": UPSTREAM, "key_configured": !app.key.is_empty(),
    }))
}

async fn ontology(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let rows = sqlx::query_as::<_, (String, i64)>("SELECT vocabulary, COUNT(*) FROM ontology_term GROUP BY 1 ORDER BY 1")
        .fetch_all(&app.pool).await.unwrap_or_default();
    Json(json!({"vocabularies": rows.iter().map(|(v,n)| json!({"vocabulary":v,"count":n})).collect::<Vec<_>>()}))
}

async fn ontology_vocab(State(app): State<Arc<App>>, AxPath(vocab): AxPath<String>) -> Json<serde_json::Value> {
    let vals = sqlx::query_scalar::<_, String>("SELECT value FROM ontology_term WHERE vocabulary=$1 ORDER BY idx")
        .bind(&vocab).fetch_all(&app.pool).await.unwrap_or_default();
    Json(json!({"vocabulary": vocab, "count": vals.len(), "values": vals}))
}

async fn library(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let rows = sqlx::query_as::<_, (String, Option<String>, Option<i32>)>("SELECT name, kind, bytes FROM doc ORDER BY name")
        .fetch_all(&app.pool).await.unwrap_or_default();
    Json(json!({"documents": rows.iter().map(|(n,k,b)| json!({"name":n,"kind":k,"bytes":b})).collect::<Vec<_>>()}))
}

async fn library_doc(State(app): State<Arc<App>>, AxPath(name): AxPath<String>) -> Response<Body> {
    match sqlx::query_scalar::<_, String>("SELECT content FROM doc WHERE name=$1")
        .bind(&name).fetch_optional(&app.pool).await.ok().flatten() {
        Some(c) => out(200, "text/plain; charset=utf-8", c, "DOC"),
        None => out(404, "text/plain", "not found".into(), "NA"),
    }
}

async fn kb(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let rows = sqlx::query_as::<_, (String, String, String, serde_json::Value, String, String)>(
        "SELECT name, kind, vocabulary, fields, value_range, description FROM cyanite_model ORDER BY name",
    ).fetch_all(&app.pool).await.unwrap_or_default();
    let by_kind = sqlx::query_as::<_, (String, i64)>(
        "SELECT kind, count(*) FROM cyanite_model GROUP BY 1 ORDER BY 1",
    ).fetch_all(&app.pool).await.unwrap_or_default();
    let spec: (i64,) = sqlx::query_as("SELECT count(*) FROM spectrogram")
        .fetch_one(&app.pool).await.unwrap_or((0,));
    Json(json!({
        "models": rows.iter().map(|(n, k, v, f, vr, d)| json!({
            "name": n, "kind": k, "vocabulary": v, "fields": f, "value_range": vr, "description": d
        })).collect::<Vec<_>>(),
        "by_kind": by_kind.iter().map(|(k, c)| json!({"kind": k, "count": c})).collect::<Vec<_>>(),
        "spectrograms_stored": spec.0,
    }))
}

async fn kb_model(State(app): State<Arc<App>>, AxPath(name): AxPath<String>) -> Json<serde_json::Value> {
    let r = sqlx::query_as::<_, (String, String, String, serde_json::Value, String, String, String)>(
        "SELECT name, kind, vocabulary, fields, value_range, segment_shape, example FROM cyanite_model WHERE name=$1",
    ).bind(&name).fetch_optional(&app.pool).await.ok().flatten();
    let vocab = sqlx::query_scalar::<_, String>("SELECT value FROM ontology_term WHERE vocabulary=$1 ORDER BY idx")
        .bind(format!("{name}Tags")).fetch_all(&app.pool).await.unwrap_or_default();
    match r {
        Some((n, k, v, f, vr, ss, ex)) => Json(json!({
            "name": n, "kind": k, "vocabulary": v, "fields": f, "value_range": vr,
            "segment_shape": ss, "example": ex, "vocabulary_values": vocab,
        })),
        None => Json(json!({"error": format!("unknown model '{name}'")})),
    }
}

async fn index(State(app): State<Arc<App>>) -> Response<Body> {
    let Json(s) = stats(State(app)).await;
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>Cyanite gateway</title>\
<style>body{{background:#0c0c12;color:#eaeaf2;font:14px/1.6 ui-sans-serif,system-ui;max-width:780px;margin:40px auto;padding:0 18px}}\
code{{background:#1b1b27;padding:2px 6px;border-radius:6px;color:#19d3a2}}h1{{color:#19d3a2}}a{{color:#7c6cff}}li{{margin:3px 0}}</style>\
<h1>Cyanite caching gateway <small style=color:#8b8b9e>(typed)</small></h1>\
<p>Point your Cyanite client base URL here — invalid requests are rejected locally and never spend quota; the \
<code>x-api-key</code> is injected server-side:</p>\
<p><code>https://rest-api.cyanite.ai/v1</code> &rarr; <code>http://THIS-HOST/v1</code></p>\
<p>Typed endpoints: <ul>\
<li>POST <code>/v1/private-alpha/library-tracks/search</code></li>\
<li>POST <code>/v1/private-alpha/library-tracks/{{id}}/similar</code></li>\
<li>POST <code>/v1/private-alpha/library-tracks/similar</code></li>\
<li>GET <code>/v1/library-tracks/{{id}}/models?model=...</code></li></ul></p>\
<p><b>{}</b> responses cached &middot; <b>{}</b> cache hits (upstream calls saved) &middot; key {}</p>\
<p>KB: <a href=/stats>/stats</a> &middot; <a href=/ontology>/ontology</a> &middot; <a href=/library>/library</a></p>",
        s["cached_responses"], s["cache_hits_served"],
        if s["key_configured"].as_bool().unwrap_or(false) { "&#10003;" } else { "&#10007; not set" },
    );
    out(200, "text/html; charset=utf-8", html, "NA")
}

async fn ensure_schema(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cyanite_cache(
            cache_key text PRIMARY KEY, method text, path text, query text, action text,
            status int NOT NULL, content_type text, body text NOT NULL,
            hits bigint NOT NULL DEFAULT 0, created_at timestamptz NOT NULL DEFAULT now(), last_hit timestamptz)",
    ).execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS cyanite_quota(action text PRIMARY KEY, used bigint NOT NULL DEFAULT 0, cap bigint NOT NULL)")
        .execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS ontology_term(vocabulary text NOT NULL, value text NOT NULL, idx int, PRIMARY KEY(vocabulary,value))")
        .execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS doc(name text PRIMARY KEY, kind text, path text, bytes int, content text)")
        .execute(pool).await?;
    Ok(())
}

async fn seed(pool: &PgPool, dir: &str) {
    if let Ok(text) = tokio::fs::read_to_string(format!("{dir}/guides/tag_vocabularies.md")).await {
        let mut vocab = String::new();
        let mut buf = String::new();
        let mut flush = Vec::new();
        for line in text.lines().chain(std::iter::once("## __END__")) {
            if let Some(rest) = line.strip_prefix("## ") {
                if !vocab.is_empty() {
                    for (i, v) in buf.split(',').map(str::trim).filter(|s| !s.is_empty()).enumerate() {
                        flush.push((vocab.clone(), v.to_string(), i as i32));
                    }
                }
                vocab = rest.split_whitespace().next().unwrap_or("").to_string();
                buf.clear();
            } else {
                buf.push(' ');
                buf.push_str(line);
            }
        }
        for (voc, val, idx) in flush {
            let _ = sqlx::query("INSERT INTO ontology_term(vocabulary,value,idx) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                .bind(voc).bind(val).bind(idx).execute(pool).await;
        }
    }
    let files = [
        ("README.md", "guide"), ("CHALLENGE.md", "guide"), ("CHALLENGE_AGREEMENT.md", "agreement"),
        ("DATA_LICENSE.md", "license"), ("guides/model_outputs.md", "api"),
        ("guides/tag_vocabularies.md", "ontology"), ("guides/cyanite_api_spec.json", "api"),
    ];
    for (rel, kind) in files {
        if let Ok(c) = tokio::fs::read_to_string(format!("{dir}/{rel}")).await {
            let name = rel.rsplit('/').next().unwrap_or(rel);
            let _ = sqlx::query("INSERT INTO doc(name,kind,path,bytes,content) VALUES($1,$2,$3,$4,$5) ON CONFLICT (name) DO UPDATE SET content=EXCLUDED.content, bytes=EXCLUDED.bytes")
                .bind(name).bind(kind).bind(rel).bind(c.len() as i32).bind(c).execute(pool).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://harvest:harvest@localhost:5432/harvest".into());
    let key = std::env::var("CYANITE_API_KEY").unwrap_or_default();
    let port: u16 = std::env::var("GATEWAY_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let docs_dir = std::env::var("GATEWAY_DOCS").unwrap_or_else(|_| ".".into());
    if key.is_empty() {
        eprintln!("WARNING: CYANITE_API_KEY not set — cache HITs work, MISSes will fail upstream auth.");
    }

    let pool = harvester::db::connect(&url, 8).await?;
    ensure_schema(&pool).await?;
    seed(&pool, &docs_dir).await;

    let app = Arc::new(App {
        pool,
        http: reqwest::Client::builder().build()?,
        key,
        rl_search: RateLimiter::direct(Quota::per_minute(nonzero!(100u32))),
        rl_similar: RateLimiter::direct(Quota::per_minute(nonzero!(100u32))),
        rl_tag: RateLimiter::direct(Quota::per_minute(nonzero!(180u32))),
    });

    let router = Router::new()
        .route("/v1/private-alpha/library-tracks/search", post(h_search))
        .route("/v1/private-alpha/library-tracks/similar", post(h_similar_multi))
        .route("/v1/private-alpha/library-tracks/:id/similar", post(h_similar_one))
        .route("/v1/library-tracks/:id/models", get(h_models))
        .route("/stats", get(stats))
        .route("/ontology", get(ontology))
        .route("/ontology/:vocab", get(ontology_vocab))
        .route("/library", get(library))
        .route("/library/:name", get(library_doc))
        .route("/kb", get(kb))
        .route("/kb/:model", get(kb_model))
        .route("/", get(index))
        .fallback(h_fallback)
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("typed cyanite gateway on :{port} (upstream {UPSTREAM}, db {url})");
    axum::serve(listener, router).await?;
    Ok(())
}
