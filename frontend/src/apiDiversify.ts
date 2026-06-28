// Calls to the MERGED product backend (Hemanth's FastAPI), separate from Orkun's same-origin
// /api. Override the host with VITE_DIVERSIFY_API at build/dev time if needed.
const BASE: string =
  (import.meta as any).env?.VITE_DIVERSIFY_API || 'http://localhost:8000/api'

export interface PersonaQuestion {
  key: string
  label: string
  options: [string, string | null][] // [display, value] pairs; value null = "Any"
}
export interface PersonaInfo {
  name: string
  blurb: string
  emphasis: string | string[]
  questions: PersonaQuestion[]
}

export interface DiversifyConfig {
  personas: PersonaInfo[]
  listeners: string[]
}

export interface BriefRow {
  rank: number
  cyanite_id: string
  jamendo_id: string
  title?: string
  artist?: string
  reason?: string
  sync?: string | null
  tags?: { genre?: string[]; mood?: string[]; bpm?: number }
}

export interface BriefView {
  brief: string
  prompt: string
  intent?: string
  persona: string
  results: BriefRow[]
}

// --- grounding (Trust panel) ---
export interface GroundingTag { tag: string; model: string; r2: number; support: number }
export interface Grounding {
  n_tracks: number
  n_tags: number
  explain: string
  per_tag: GroundingTag[]
}

const getJSON = <T>(path: string): Promise<T> =>
  fetch(BASE + path).then((r) => {
    if (!r.ok) throw new Error(`${path} -> ${r.status}`)
    return r.json()
  })

const postJSON = <T>(path: string, body: unknown): Promise<T> =>
  fetch(BASE + path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  }).then((r) => {
    if (!r.ok) throw new Error(`${path} -> ${r.status}`)
    return r.json()
  })

export const getConfig = () => getJSON<DiversifyConfig>('/config')

export const postBrief = (
  brief: string,
  persona: string,
  answers: Record<string, string | null> = {},
) => postJSON<BriefView>('/brief', { brief, persona, answers })

// Trust panel reads grounding via the deep proxy (/api/deep/* -> Erkin's server).
export const getGrounding = () => getJSON<Grounding>('/deep/grounding')
export const getDeepHealth = () =>
  getJSON<{ available: boolean; reason?: string }>('/deep/health')

// --- Taste (named listeners) ---
export interface TasteViz {
  valence: number
  arousal: number
  mood: Record<string, number>
  instruments: [string, number][]
}
export interface Sample {
  cyanite_id: string
  artist: string
  title: string
  jamendo: string
}
export interface ListenerProfile {
  persona: string
  viz: TasteViz
  samples: Sample[]
}

export const getListener = (name: string) =>
  getJSON<ListenerProfile>('/listener/' + encodeURIComponent(name))

export const postStream = (
  name: string,
  steers: Record<string, string | null>,
  persona_override: string,
) => postJSON<BriefView>('/listener/' + encodeURIComponent(name) + '/stream', { steers, persona_override })

export const audioUrl = (jamendoId: string) => `${BASE}/audio/${jamendoId}`
