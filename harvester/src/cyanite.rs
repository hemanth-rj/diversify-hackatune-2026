//! Typed Cyanite REST layer — the canonical client (supersedes the `sounds-like-you`
//! crate). Request types are **strictly validated** so malformed requests are
//! rejected at the gateway and NEVER spend a pooled-quota upstream call. Response
//! types are typed but use `#[serde(flatten)]` to preserve unknown fields losslessly.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const UPSTREAM: &str = "https://rest-api.cyanite.ai/v1";

/// The 23 AI model types accepted by `GET /library-tracks/{id}/models?model=...`.
pub const MODELS: &[&str] = &[
    "AiMusicDetectionV1", "AudioFileInfoV1", "AugmentedKeywordsV3", "AutoDescriptionV2", "BpmV2",
    "CharacterV2", "FreeGenreV3", "InstrumentsV2", "KeyV2", "MainGenreV2", "MoodAdvancedV2",
    "MoodSimpleV2", "MovementV2", "MusicalEraV2", "MusicForV1", "RepresentativeSegmentV2",
    "SubgenreV2", "TempoV1", "TimeSignatureV2", "ValenceArousalV2", "VocalStyleV1", "VocalsV2",
    "VoiceoverV2",
];

/// A Cyanite library-track id must match `^libtr_[A-Za-z0-9]+`.
pub fn valid_id(id: &str) -> bool {
    id.len() > 6 && id.starts_with("libtr_") && id[6..].chars().all(|c| c.is_ascii_alphanumeric())
}

/// `limit` query param: absent, or 1..=50. Cyanite's REAL search rejects >50 (422)
/// and ignores `offset`, so we reject >50 LOCALLY (never spend pooled quota on a
/// request Cyanite is guaranteed to refuse). The "up to 100 + paginate" capability
/// is served instead by the catalog-backed fake search (/api/cyanite_search).
pub fn valid_limit(limit: Option<i64>) -> Result<(), String> {
    match limit {
        None => Ok(()),
        Some(l) if (1..=50).contains(&l) => Ok(()),
        Some(l) => Err(format!("limit must be an integer in 1..=50 (got {l}); use the catalog-backed fake search for up to 100 + pagination")),
    }
}

/// A `metadataFilter`, if present, must be a JSON object whose dot-notation keys
/// `<Model>.<field>` reference a known model (or be a `$and`/`$or` combinator).
pub fn validate_filter(f: &Option<Value>) -> Result<(), String> {
    let Some(v) = f else { return Ok(()) };
    let obj = v.as_object().ok_or("metadataFilter must be a JSON object")?;
    for (k, val) in obj.iter() {
        if k.starts_with('$') {
            // $and/$or: array of sub-filters; $not: a single sub-filter object.
            // Recurse so a malformed combinator is rejected locally, never forwarded
            // upstream (which would spend pooled quota and cache a bad response).
            match val {
                Value::Array(arr) => {
                    for elem in arr {
                        validate_filter(&Some(elem.clone()))?;
                    }
                }
                Value::Object(_) => validate_filter(&Some(val.clone()))?,
                _ => return Err(format!("combinator '{k}' must be an array or object")),
            }
            continue;
        }
        let model = k.split('.').next().unwrap_or("");
        if !MODELS.contains(&model) {
            return Err(format!("metadataFilter key '{k}' references unknown model '{model}'"));
        }
    }
    Ok(())
}

// ---------- request types (strict) ----------

#[derive(Debug, Deserialize)]
pub struct SearchReq {
    pub query: String,
    #[serde(default, rename = "metadataFilter")]
    pub metadata_filter: Option<Value>,
}
impl SearchReq {
    pub fn validate(&self) -> Result<(), String> {
        if self.query.trim().is_empty() {
            return Err("`query` must be a non-empty string".into());
        }
        validate_filter(&self.metadata_filter)
    }
}

#[derive(Debug, Deserialize)]
pub struct SimilarReq {
    #[serde(default, rename = "metadataFilter")]
    pub metadata_filter: Option<Value>,
}
impl SimilarReq {
    pub fn validate(&self) -> Result<(), String> {
        validate_filter(&self.metadata_filter)
    }
}

#[derive(Debug, Deserialize)]
pub struct TrackRef {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct SimilarMultiReq {
    pub tracks: Vec<TrackRef>,
    #[serde(default, rename = "metadataFilter")]
    pub metadata_filter: Option<Value>,
}
impl SimilarMultiReq {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=10).contains(&self.tracks.len()) {
            return Err(format!("`tracks` must contain 1..=10 ids (got {})", self.tracks.len()));
        }
        for t in &self.tracks {
            if !valid_id(&t.id) {
                return Err(format!("invalid track id '{}' (must match ^libtr_)", t.id));
            }
        }
        validate_filter(&self.metadata_filter)
    }
}

// ---------- response types (typed, lossless) ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub items: Vec<SearchItem>,
    #[serde(default, rename = "pageInfo")]
    pub page_info: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchItem {
    pub track: Track,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    /// Everything else the API returns (externalId, createdAt, customTags, …) — kept.
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

impl Track {
    /// Best-effort Jamendo id from the `<jamendo_id>.mp3` title convention.
    pub fn jamendo_id(&self) -> Option<String> {
        self.title.as_ref().map(|t| t.split(".mp3").next().unwrap_or(t).to_string())
    }
}

// ---------- model outputs (ported from sounds-like-you/models.rs) ----------

pub type Scores = BTreeMap<String, f32>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEnvelope {
    pub items: Vec<ModelOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version", rename_all_fields = "camelCase")]
pub enum ModelOutput {
    MainGenreV2 { scores: Scores, tags: Vec<String>, #[serde(default)] segments: Option<Value> },
    MoodSimpleV2 { scores: Scores, tags: Vec<String>, #[serde(default)] segments: Option<Value> },
    MoodAdvancedV2 { scores: Scores, tags: Vec<String>, #[serde(default)] segments: Option<Value> },
    CharacterV2 { scores: Scores, tags: Vec<String>, #[serde(default)] segments: Option<Value> },
    MovementV2 { scores: Scores, tags: Vec<String>, #[serde(default)] segments: Option<Value> },
    VocalStyleV1 { scores: Scores, tags: Vec<String>, #[serde(default)] segments: Option<Value> },
    SubgenreV2 {
        #[serde(default)] scores: Option<Scores>,
        #[serde(default)] tags: Option<Vec<String>>,
        #[serde(default)] segments: Option<Value>,
    },
    FreeGenreV3 { tags: Vec<String> },
    MusicForV1 { tags: Vec<String> },
    AugmentedKeywordsV3 { scores: Scores },
    AutoDescriptionV2 { description: String },
    VocalsV2 {
        scores: Scores, tags: Vec<String>,
        #[serde(default)] vocal_presence: Option<String>,
        #[serde(default)] predominant_vocal_gender: Option<String>,
        #[serde(default)] segments: Option<Value>,
    },
    BpmV2 { tag: i32, confidence: Confidence, #[serde(default)] segments: Option<Value> },
    KeyV2 { tag: String, confidence: Confidence, #[serde(default)] segments: Option<Value> },
    TimeSignatureV2 { tag: String, confidence: Confidence, #[serde(default)] segments: Option<Value> },
    TempoV1 { tag: String, score: f32, #[serde(default)] segments: Option<Value> },
    ValenceArousalV2 {
        scores: Scores, energy_level: String, energy_changes: String,
        emotion_profile: String, emotion_changes: String, #[serde(default)] segments: Option<Value>,
    },
    MusicalEraV2 { estimated_production_year: i32, tag: String },
    InstrumentsV2 {
        #[serde(default)] presence: Option<BTreeMap<String, String>>,
        tags: Vec<String>, #[serde(default)] segments: Option<Value>,
    },
    RepresentativeSegmentV2 { start_seconds: f32, end_seconds: f32 },
    AiMusicDetectionV1 { is_ai_music: bool, score: f32, #[serde(default)] suspected_model: Option<String> },
    VoiceoverV2 { voiceover_degree: f32, is_voiceover_dominant: bool, #[serde(default)] segments: Option<Value> },
    AudioFileInfoV1 { duration: f32, file_size_b: i64, bitrate: i64, samplerate: i64 },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Confidence {
    pub model_certainty: f32,
    pub prediction_stability: f32,
    pub confidence: f32,
}
