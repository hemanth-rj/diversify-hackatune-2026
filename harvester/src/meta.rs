//! Load tracks.csv into [`TrackMeta`], assigning each track an **album block**.
//!
//! Jamendo ids are sequential within an album upload, so a run of near-
//! consecutive ids by the same artist is an album. We label each track with the
//! minimum jamendo id of its run — a stable album key usable for de-duplication
//! and diversity in downstream recommendation.

use crate::db::TrackMeta;
use anyhow::Result;
use std::collections::HashMap;

const ALBUM_GAP: i64 = 4;

/// Unescape the HTML entities present in the Jamendo export.
pub fn clean(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .trim()
        .to_string()
}

#[derive(serde::Deserialize)]
struct Row {
    track_id: String,
    cyanite_id: String,
    name: String,
    artist_name: String,
    duration: Option<f64>,
    #[serde(default)]
    license_ccurl: String,
}

pub fn load_tracks(path: &str) -> Result<Vec<TrackMeta>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let rows: Vec<Row> = rdr.deserialize().collect::<Result<_, _>>()?;

    // group jamendo ids per cleaned artist to find album runs
    let mut by_artist: HashMap<String, Vec<i64>> = HashMap::new();
    for r in &rows {
        if let Ok(jid) = r.track_id.parse::<i64>() {
            by_artist.entry(clean(&r.artist_name)).or_default().push(jid);
        }
    }
    // jamendo_id -> album-block key (min id of its consecutive run)
    let mut block_of: HashMap<i64, i64> = HashMap::new();
    for ids in by_artist.values_mut() {
        ids.sort_unstable();
        let mut run_min = ids[0];
        block_of.insert(ids[0], run_min);
        for w in ids.windows(2) {
            if w[1] - w[0] > ALBUM_GAP {
                run_min = w[1]; // gap -> new album run starts
            }
            block_of.insert(w[1], run_min);
        }
    }

    Ok(rows
        .into_iter()
        .map(|r| {
            let jid = r.track_id.parse::<i64>().unwrap_or(0);
            let block = block_of.get(&jid).copied().unwrap_or(jid);
            TrackMeta {
                cyanite_id: r.cyanite_id,
                jamendo_id: r.track_id,
                name: clean(&r.name),
                artist: clean(&r.artist_name),
                album_block: block.to_string(),
                duration: r.duration,
                license: r.license_ccurl,
            }
        })
        .collect())
}
