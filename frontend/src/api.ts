// Orkun/Erkin's deployed backend (Chat, Similar, MoodBoard, Insights). Cross-origin is fine —
// that backend sends CORS allow-origin *. Override with VITE_ORKUN_API if the host changes.
const BASE: string =
  (import.meta as any).env?.VITE_ORKUN_API || 'http://95.216.72.161:8001/api'

// The backend returns audio as a relative path (e.g. "/audio/965307.mp3"), which only works
// when the frontend is served same-origin. In dev (localhost:5173) we must resolve it against
// the backend origin, or nothing plays.
const ORIGIN = BASE.replace(/\/api\/?$/, '')
export function audioSrc(url: string | null | undefined): string {
  if (!url) return ''
  if (/^https?:\/\//.test(url)) return url
  return ORIGIN + (url.startsWith('/') ? url : '/' + url)
}

export interface MatchDim {
  dimension: string
  seedScore: number
  resultScore: number
  delta: number
}

export interface TrackResult {
  id: string
  jamendoId: string
  name: string
  artist: string
  duration: number
  audioUrl: string
  cyaniteScore: number
  tagSim: number
  finalScore: number
  explanation: string
  matchBreakdown: MatchDim[]
  autoDescription: string
  representativeSegmentStart: number
  valence: number
  arousal: number
}

export interface ChatResponse {
  tracks: TrackResult[]
  inferredQuery: string
  inferredFilters: Record<string, unknown>
  filterSummary: string
}

export interface SeedTrack {
  id: string
  jamendoId: string
  name: string
  artist: string
  audioUrl: string
}

export interface TasteResponse {
  tracks: TrackResult[]
  profileSummary: string
  profileFingerprint: { dimension: string; weight: number }[]
  seedTracks: SeedTrack[]
}

export interface SimilarResponse {
  seed: TrackResult
  tracks: TrackResult[]
}

export interface MultimodalResponse {
  tracks: TrackResult[]
  inferred: { query: string; filterSummary: string }
}

export async function chatSearch(
  messages: { role: string; content: string }[],
  limit = 10
): Promise<ChatResponse> {
  const r = await fetch(`${BASE}/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ messages, limit }),
  })
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

export async function tasteRecs(
  userId: string,
  limit = 20,
  steer?: { pinIds?: string[]; excludeMoods?: string[] }
): Promise<TasteResponse> {
  const r = await fetch(`${BASE}/taste`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ userId, limit, steer }),
  })
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

export async function similarTracks(seedId: string, limit = 20): Promise<SimilarResponse> {
  const r = await fetch(`${BASE}/similar`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ seedId, limit }),
  })
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

export async function multimodalSearch(
  params: { brief?: string; image?: string; mimeType?: string; limit?: number }
): Promise<MultimodalResponse> {
  const r = await fetch(`${BASE}/multimodal`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(params),
  })
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

export interface PredictedTag { tag: string; prob: number; n: number }

export async function searchTracksByFile(
  file: File, limit = 20
): Promise<{ tracks: TrackResult[]; predicted_tags: Record<string, PredictedTag[]> }> {
  const form = new FormData()
  form.append('file', file)
  const r = await fetch(`${BASE}/similar-file?limit=${limit}`, { method: 'POST', body: form })
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

export async function searchTracksByName(
  q: string, limit = 15
): Promise<{ id: string; name: string; artist: string; jamendoId: string }[]> {
  const r = await fetch(`${BASE}/tracks/search?q=${encodeURIComponent(q)}&limit=${limit}`)
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

export interface TrackDetail {
  id: string; jamendoId: string; name: string; artist: string
  duration: number; audioUrl: string; license: string | null
  key: string | null; bpm: number | null; timeSignature: string | null
  valence: number | null; arousal: number | null
  energyLevel: string | null; vocalPresence: string | null; vocalGender: string | null
  description: string | null
  tags: Record<string, { tag: string; score: number | null }[]>
  rhythm: Record<string, number | null> | null
  pitch: Record<string, number | null> | null
  spectral: Record<string, number | null> | null
  spectralAubio: Record<string, number | null> | null
  rms: Record<string, number | null> | null
  zcr: Record<string, number | null> | null
  notes: { note_count: number; note_mean_dur: number | null } | null
  chromaVec: number[] | null
  mfccVec: number[] | null
}

export async function fetchTrackDetail(cid: string): Promise<TrackDetail> {
  const r = await fetch(`${BASE}/track/${encodeURIComponent(cid)}`)
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

export interface TagTrack {
  id: string; jamendoId: string; name: string; artist: string
  duration: number; audioUrl: string; tagScore: number
}

export async function fetchTagTracks(
  model: string, tag: string, limit = 15
): Promise<{ model: string; tag: string; tracks: TagTrack[] }> {
  const r = await fetch(`${BASE}/tag/${encodeURIComponent(model)}/${encodeURIComponent(tag)}?limit=${limit}`)
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

export async function listUsers(): Promise<{ id: string; n: number }[]> {
  const r = await fetch(`${BASE}/users`)
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}
