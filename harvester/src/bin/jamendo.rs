//! `jamendo` — scrape every usable fact from the Jamendo API into Postgres.
//!
//! The Cyanite-only pipeline has no genre/mood/instrument tags; Jamendo does, for
//! free, straight from `/tracks/?include=musicinfo`. For each of our ~10,561
//! jamendo track ids (batched 200 per call) we store the track facts plus its
//! genre / instrument / vartag (mood/theme) tags — the metadata layer the
//! experiments were missing.

use anyhow::Result;
use serde_json::Value;
use std::env;

const BASE: &str = "https://api.jamendo.com/v3.0";

#[tokio::main]
async fn main() -> Result<()> {
    let url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://harvest:harvest@localhost:5432/harvest".into());
    let cid = env::var("JAMENDO_CLIENT_ID").expect("set JAMENDO_CLIENT_ID");
    let pool = harvester::db::connect(&url, 4).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS jamendo_track(
            jamendo_id text PRIMARY KEY, name text, artist_id text, artist_name text,
            album_id text, album_name text, releasedate text, duration int, position int,
            license_ccurl text, fetched_at timestamptz NOT NULL DEFAULT now())",
    ).execute(&pool).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS jamendo_tag(
            jamendo_id text, kind text, tag text, PRIMARY KEY(jamendo_id, kind, tag))",
    ).execute(&pool).await?;

    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT jamendo_id FROM tracks WHERE jamendo_id IS NOT NULL ORDER BY jamendo_id",
    ).fetch_all(&pool).await?;
    let http = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (hackatune research; +cyanite)")
        .build()?;
    let total = ids.len();
    let mut done = 0usize;
    println!("jamendo scrape: {total} track ids, batches of 200");

    for chunk in ids.chunks(50) {
        let joined = chunk.join("+");
        let u = format!("{BASE}/tracks/?client_id={cid}&format=json&limit=50&include=musicinfo&id={joined}");
        // Jamendo sits behind Cloudflare (error 1015 / HTTP 429 on bursts); pace +
        // retry with backoff.
        let mut body: Option<Value> = None;
        for attempt in 0..5 {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            match http.get(&u).send().await {
                Ok(r) if r.status().is_success() => match r.json::<Value>().await {
                    Ok(v) => { body = Some(v); break; }
                    Err(e) => eprintln!("json error: {e}"),
                },
                Ok(r) => {
                    eprintln!("http {} (attempt {attempt}); backing off", r.status());
                    tokio::time::sleep(std::time::Duration::from_secs(5 * (attempt + 1) as u64)).await;
                }
                Err(e) => eprintln!("request error: {e}"),
            }
        }
        let Some(body) = body else { eprintln!("giving up on batch"); continue };
        // surface Jamendo API-level errors (code != 0)
        if body["headers"]["code"].as_i64().unwrap_or(0) != 0 {
            eprintln!("jamendo api error: {}", body["headers"]["error_message"]);
        }
        let empty = vec![];
        let results = body["results"].as_array().unwrap_or(&empty);
        let mut tx = pool.begin().await?;
        for r in results {
            let jid = r["id"].as_str().map(str::to_string)
                .or_else(|| r["id"].as_i64().map(|n| n.to_string()));
            let Some(jid) = jid else { continue };
            sqlx::query(
                "INSERT INTO jamendo_track(jamendo_id,name,artist_id,artist_name,album_id,album_name,releasedate,duration,position,license_ccurl)
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
                 ON CONFLICT (jamendo_id) DO UPDATE SET name=EXCLUDED.name, artist_name=EXCLUDED.artist_name,
                   album_name=EXCLUDED.album_name, releasedate=EXCLUDED.releasedate, license_ccurl=EXCLUDED.license_ccurl",
            )
            .bind(&jid)
            .bind(r["name"].as_str())
            .bind(r["artist_id"].as_str())
            .bind(r["artist_name"].as_str())
            .bind(r["album_id"].as_str())
            .bind(r["album_name"].as_str())
            .bind(r["releasedate"].as_str())
            .bind(r["duration"].as_str().and_then(|s| s.parse::<i32>().ok()).or_else(|| r["duration"].as_i64().map(|n| n as i32)))
            .bind(r["position"].as_str().and_then(|s| s.parse::<i32>().ok()).or_else(|| r["position"].as_i64().map(|n| n as i32)))
            .bind(r["license_ccurl"].as_str())
            .execute(&mut *tx).await?;
            for (kind, key) in [("genre", "genres"), ("instrument", "instruments"), ("vartag", "vartags")] {
                if let Some(arr) = r["musicinfo"]["tags"][key].as_array() {
                    for t in arr {
                        if let Some(tag) = t.as_str() {
                            sqlx::query("INSERT INTO jamendo_tag(jamendo_id,kind,tag) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                                .bind(&jid).bind(kind).bind(tag).execute(&mut *tx).await?;
                        }
                    }
                }
            }
        }
        tx.commit().await?;
        done += results.len();
        println!("jamendo: {done}/{total} tracks");
    }
    let (nt, ng): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM jamendo_track), (SELECT count(*) FROM jamendo_tag)",
    ).fetch_one(&pool).await?;
    println!("jamendo scrape complete: {nt} tracks, {ng} tags");
    Ok(())
}
