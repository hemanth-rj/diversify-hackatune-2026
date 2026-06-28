import { useEffect, useState, useRef } from 'react'
import { audioSrc, fetchTrackDetail, type TrackDetail } from '../api'
import TagExploreModal from './TagExploreModal'

// ─── constants ────────────────────────────────────────────────────────────────
const PITCH_NAMES = ['C','C#','D','D#','E','F','F#','G','G#','A','A#','B']

// Erkin's deep server serves the log-mel spectrogram PNG at /api/spectrogram/{cyanite_id}.
const ERKIN_BASE: string =
  (import.meta as any).env?.VITE_ERKIN_API || 'http://95.216.72.161:8000'

const MODEL_TO_CAT: Record<string, string> = {
  MoodSimpleV2:  'mood',
  MoodAdvancedV2:'mood',
  CharacterV2:   'character',
  MainGenreV2:   'genre',
  SubgenreV2:    'genre',
  FreeGenreV3:   'genre',
  InstrumentsV2: 'instrument',
  MovementV2:    'movement',
  VocalsV2:      'vocals',
  MusicForV1:    'music for',
}

const CAT_COLOR: Record<string, string> = {
  mood:        '#7c6cff',
  character:   '#ffb454',
  genre:       '#ff3d7f',
  instrument:  '#19d3a2',
  movement:    '#19d3a2',
  vocals:      '#7c6cff',
  'music for': '#5b9bd5',
}

const CAT_ORDER = ['mood','genre','instrument','character','movement','vocals','music for']

// ─── types ────────────────────────────────────────────────────────────────────
interface TagEntry { tag: string; score: number | null; model: string }

// ─── helpers ──────────────────────────────────────────────────────────────────
function buildTagGroups(raw: Record<string, { tag: string; score: number | null }[]>): Map<string, TagEntry[]> {
  const g = new Map<string, TagEntry[]>()
  for (const [model, entries] of Object.entries(raw)) {
    const cat = MODEL_TO_CAT[model] ?? model
    if (!g.has(cat)) g.set(cat, [])
    for (const { tag, score } of entries) g.get(cat)!.push({ tag, score, model })
  }
  for (const [cat, arr] of g) {
    arr.sort((a, b) => (b.score ?? 0) - (a.score ?? 0))
    const seen = new Set<string>()
    g.set(cat, arr.filter(e => { if (seen.has(e.tag)) return false; seen.add(e.tag); return true }))
  }
  return g
}

function dotColor(score: number | null): string | null {
  if (score == null || score < 0.25) return null
  if (score >= 0.7) return '#19d3a2'
  if (score >= 0.5) return '#ffb454'
  return '#ff5555'
}

function tagOpacity(score: number | null): number {
  if (score == null) return 0.5
  return Math.max(0.45, Math.min(0.95, 0.38 + score * 0.62))
}

function fmt(v: number | null | undefined): string {
  if (v == null) return '—'
  return Number.isInteger(v) ? String(v) : v.toFixed(3)
}

// ─── sub-components ───────────────────────────────────────────────────────────
function SLabel({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ fontSize: 10, color: '#555578', textTransform: 'uppercase', letterSpacing: '0.07em', marginBottom: 6 }}>
      {children}
    </div>
  )
}

function BarChart({ values, labels, color }: { values: number[]; labels?: string[]; color: string }) {
  const max = Math.max(...values.map(Math.abs), 1e-9)
  return (
    <div style={{ display: 'flex', alignItems: 'flex-end', gap: 2, height: 60 }}>
      {values.map((v, i) => (
        <div key={i} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', flex: 1 }}>
          <div style={{
            width: '100%',
            height: `${(Math.abs(v) / max) * 48}px`,
            minHeight: 1,
            background: `linear-gradient(to top, ${color}, ${color}55)`,
            borderRadius: '2px 2px 0 0',
          }} />
          {labels && (
            <div style={{ fontSize: 8, color: '#3a3a55', marginTop: 3, lineHeight: 1 }}>{labels[i]}</div>
          )}
        </div>
      ))}
    </div>
  )
}

function AudioPane({ data, title, icon }: {
  data: Record<string, number | null> | null
  title: string
  icon: string
}) {
  if (!data) return null
  const entries = Object.entries(data).filter(([, v]) => v != null)
  if (!entries.length) return null
  return (
    <div style={{
      background: '#0b0b17', border: '1px solid #1c1c2e',
      borderRadius: 10, padding: '12px 14px', flex: 1,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 10 }}>
        <span style={{ fontSize: 14, opacity: 0.6 }}>{icon}</span>
        <SLabel>{title}</SLabel>
      </div>
      {entries.map(([k, v]) => (
        <div key={k} style={{
          display: 'flex', justifyContent: 'space-between',
          fontSize: 11.5, padding: '4px 0',
          borderBottom: '1px solid #151525',
        }}>
          <span style={{ color: '#555578' }}>{k}</span>
          <span style={{ color: '#c0c0d8', fontVariantNumeric: 'tabular-nums' }}>{fmt(v)}</span>
        </div>
      ))}
    </div>
  )
}

// ─── main ─────────────────────────────────────────────────────────────────────
interface Props { trackId: string; onClose: () => void }

export default function TrackModal({ trackId, onClose }: Props) {
  const [detail, setDetail]     = useState<TrackDetail | null>(null)
  const [loading, setLoading]   = useState(true)
  const [exploreTag, setExploreTag] = useState<{ model: string; tag: string; color: string } | null>(null)
  const [innerTrackId, setInnerTrackId] = useState<string | null>(null)
  const [specError, setSpecError] = useState(false)
  const audioRef = useRef<HTMLAudioElement>(null)

  useEffect(() => {
    setLoading(true); setDetail(null); setExploreTag(null); setInnerTrackId(null); setSpecError(false)
    fetchTrackDetail(trackId).then(setDetail).finally(() => setLoading(false))
  }, [trackId])

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (innerTrackId) { setInnerTrackId(null); return }
        if (exploreTag) { setExploreTag(null); return }
        onClose()
      }
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [onClose, exploreTag, innerTrackId])

  const d = detail

  const chroma12 = d?.chromaVec
    ? Array.from({ length: 12 }, (_, i) => {
        const chunk = d.chromaVec!.slice(i * 4, i * 4 + 4)
        return chunk.reduce((a, b) => a + b, 0) / 4
      })
    : null

  const tagGroups = d ? buildTagGroups(d.tags) : null
  const topTagNames = tagGroups
    ? CAT_ORDER.flatMap(cat => (tagGroups.get(cat) ?? []).slice(0, 2).map(t => t.tag)).slice(0, 6)
    : []

  // Merged dynamics data
  const dynamicsData: Record<string, number | null> | null =
    (d?.rms || d?.zcr) ? {
      ...Object.fromEntries(Object.entries(d?.rms ?? {}).map(([k, v]) => [`rms_${k}`, v])),
      ...Object.fromEntries(Object.entries(d?.zcr ?? {}).map(([k, v]) => [`zcr_${k}`, v])),
    } : null

  return (
    <>
      <div
        style={{
          position: 'fixed', inset: 0,
          background: 'rgba(4,4,14,0.88)',
          zIndex: 1000, display: 'flex',
          alignItems: 'flex-start', justifyContent: 'center',
          padding: '28px 16px', overflowY: 'auto',
          backdropFilter: 'blur(8px)',
        }}
        onClick={e => { if (e.target === e.currentTarget) onClose() }}
      >
        <div style={{
          background: '#0e0e1b',
          borderRadius: 14, width: '100%', maxWidth: 760,
          flexShrink: 0, overflow: 'hidden',
          border: '1px solid #1c1c2e',
          boxShadow: '0 32px 100px rgba(0,0,0,0.8)',
        }}>
          {/* Gradient accent bar */}
          <div style={{ height: 3, background: 'linear-gradient(90deg, #ff3d7f, #7c6cff 50%, #19d3a2)' }} />

          <div style={{ padding: '22px 26px 28px', maxHeight: '88vh', overflowY: 'auto' }}>

            {/* Title row + close */}
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 14 }}>
              <div style={{ flex: 1, paddingRight: 16, minWidth: 0 }}>
                {loading ? (
                  <div style={{ color: '#555578', fontSize: 13, padding: '32px 0' }}>Loading…</div>
                ) : d ? (
                  <>
                    <div style={{ fontSize: 20, fontWeight: 800, color: '#eaeaf2', letterSpacing: '-0.03em', lineHeight: 1.2 }}>{d.name}</div>
                    <div style={{ fontSize: 13, color: '#6b6b85', marginTop: 4 }}>{d.artist}</div>
                  </>
                ) : (
                  <div style={{ color: '#ff3d7f', fontSize: 13 }}>Failed to load.</div>
                )}
              </div>
              <button onClick={onClose} style={{
                flexShrink: 0,
                background: '#181828', border: '1px solid #2a2a3e',
                color: '#555578', cursor: 'pointer',
                borderRadius: 8, width: 30, height: 30,
                fontSize: 16, display: 'flex', alignItems: 'center', justifyContent: 'center',
              }}>✕</button>
            </div>

            {d && (
              <>
                {/* Meta chips */}
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginBottom: 14 }}>
                  {([
                    d.duration > 0 && `${Math.floor(d.duration / 60)}:${String(d.duration % 60).padStart(2, '0')}`,
                    d.key      && `key ${d.key}`,
                    d.bpm      && `${d.bpm} BPM`,
                    d.timeSignature,
                    d.energyLevel  && `energy ${d.energyLevel}`,
                    d.vocalPresence && `${d.vocalPresence}${d.vocalGender ? ` (${d.vocalGender})` : ''}`,
                    d.valence != null && `valence ${d.valence > 0 ? '+' : ''}${d.valence}`,
                    d.arousal != null && `arousal ${d.arousal > 0 ? '+' : ''}${d.arousal}`,
                  ] as (string | false)[]).filter(Boolean).map((m, i) => (
                    <span key={i} style={{
                      fontSize: 11, color: '#8888a8',
                      background: '#13131f', border: '1px solid #1c1c2e',
                      borderRadius: 20, padding: '3px 10px',
                    }}>{m as string}</span>
                  ))}
                  {d.license && (
                    <a href={d.license} target="_blank" rel="noopener noreferrer" style={{
                      fontSize: 11, color: '#7c6cff',
                      background: '#7c6cff1a', border: '1px solid #7c6cff33',
                      borderRadius: 20, padding: '3px 10px', textDecoration: 'none',
                    }}>CC license ↗</a>
                  )}
                </div>

                {/* Audio */}
                {d.audioUrl && (
                  <audio ref={audioRef} controls style={{ width: '100%', marginBottom: 14 }} src={audioSrc(d.audioUrl)} />
                )}

                {/* Spectrogram (log-mel) — from Erkin's audio analysis */}
                {!specError && (
                  <div style={{ background: '#0b0b17', border: '1px solid #1c1c2e', borderRadius: 10, padding: '12px 14px', marginBottom: 14 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
                      <span style={{ fontSize: 14, opacity: 0.6 }}>🌈</span>
                      <SLabel>Spectrogram · log-mel</SLabel>
                    </div>
                    <img
                      src={`${ERKIN_BASE}/api/spectrogram/${trackId}`}
                      alt="log-mel spectrogram"
                      onError={() => setSpecError(true)}
                      style={{ width: '100%', borderRadius: 6, display: 'block' }}
                    />
                  </div>
                )}

                {/* Description */}
                {d.description && (
                  <div style={{
                    fontSize: 12.5, color: '#8888aa', fontStyle: 'italic',
                    lineHeight: 1.65, marginBottom: 20,
                    padding: '10px 14px',
                    background: '#0c0c18',
                    borderLeft: '2px solid #2a2a42',
                    borderRadius: '0 6px 6px 0',
                  }}>{d.description}</div>
                )}

                {/* ════ TAG PANEL ════ */}
                {tagGroups && tagGroups.size > 0 && (
                  <div style={{
                    borderTop: '1px solid #1c1c2e',
                    borderRight: '1px solid #1c1c2e',
                    borderBottom: '1px solid #1c1c2e',
                    borderLeft: '3px solid #ffb454',
                    background: '#0b0b17',
                    borderRadius: '0 8px 8px 0',
                    padding: '12px 14px',
                    marginBottom: 24,
                  }}>
                    {/* Panel header */}
                    <div style={{ fontSize: 12, marginBottom: 10 }}>
                      <b style={{ color: '#c0c0d8', fontWeight: 600 }}>Predicted tags</b>
                      {topTagNames.length > 0 && (
                        <span style={{ color: '#3a3a55', marginLeft: 8 }}>{topTagNames.join(', ')}…</span>
                      )}
                    </div>

                    {CAT_ORDER.map(cat => {
                      const entries = tagGroups.get(cat)
                      if (!entries?.length) return null
                      const color = CAT_COLOR[cat] ?? '#8b8b9e'
                      return (
                        <div key={cat} style={{ marginTop: 9 }}>
                          <div style={{ fontSize: 10, color: '#555578', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 4 }}>
                            {cat}
                          </div>
                          <div>
                            {entries.map(({ tag, score, model }) => {
                              const dc  = dotColor(score)
                              const op  = tagOpacity(score)
                              const pct = score != null ? Math.round(score * 100) : null
                              return (
                                <button
                                  key={`${model}:${tag}`}
                                  title={`${model} · ${pct != null ? pct + '% confidence' : '?'} · click to explore`}
                                  onClick={() => setExploreTag({ model, tag, color })}
                                  style={{
                                    display: 'inline-flex', alignItems: 'center', gap: 4,
                                    margin: '2px 3px', padding: '2px 8px',
                                    borderRadius: 10,
                                    border: '1px solid #1e1e2e',
                                    background: '#13131f',
                                    fontSize: 11.5, opacity: op,
                                    cursor: 'pointer',
                                    fontFamily: 'inherit',
                                    transition: 'all 0.12s',
                                  }}
                                  onMouseEnter={e => {
                                    const el = e.currentTarget
                                    el.style.borderColor = color + '66'
                                    el.style.opacity = '1'
                                    el.style.background = color + '14'
                                  }}
                                  onMouseLeave={e => {
                                    const el = e.currentTarget
                                    el.style.borderColor = '#1e1e2e'
                                    el.style.opacity = String(op)
                                    el.style.background = '#13131f'
                                  }}
                                >
                                  {dc && <span style={{ color: dc, fontSize: 9, lineHeight: 1 }}>●</span>}
                                  <span style={{ color }}>{tag}</span>
                                  {pct != null && <span style={{ color: '#555578', fontSize: 10 }}>{pct}%</span>}
                                </button>
                              )
                            })}
                          </div>
                        </div>
                      )
                    })}

                    <div style={{ fontSize: 10, color: '#3a3a55', marginTop: 10, lineHeight: 1.5 }}>
                      sorted by Cyanite confidence ·{' '}
                      ● <span style={{ color: '#19d3a2' }}>green</span> ≥70% ·{' '}
                      <span style={{ color: '#ffb454' }}>amber</span> ≥50% ·{' '}
                      <span style={{ color: '#ff5555' }}>red</span> lower · opacity encodes strength ·{' '}
                      <span style={{ color: '#555578' }}>click any tag to explore top tracks</span>
                    </div>
                  </div>
                )}

                {/* ════ AUDIO VISUALIZATIONS ════ */}
                <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>

                  {/* Chroma + MFCC */}
                  {(chroma12 || d.mfccVec) && (
                    <div style={{ display: 'grid', gridTemplateColumns: chroma12 && d.mfccVec ? '1fr 1fr' : '1fr', gap: 14 }}>
                      {chroma12 && (
                        <div style={{ background: '#0b0b17', border: '1px solid #1c1c2e', borderRadius: 10, padding: '12px 14px' }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
                            <span style={{ fontSize: 14, opacity: 0.6 }}>🎹</span>
                            <SLabel>Chroma / pitch</SLabel>
                          </div>
                          <BarChart values={chroma12} labels={PITCH_NAMES} color="#7c6cff" />
                        </div>
                      )}
                      {d.mfccVec && d.mfccVec.length > 0 && (
                        <div style={{ background: '#0b0b17', border: '1px solid #1c1c2e', borderRadius: 10, padding: '12px 14px' }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
                            <span style={{ fontSize: 14, opacity: 0.6 }}>〰</span>
                            <SLabel>MFCC timbre · {d.mfccVec.length} coeff</SLabel>
                          </div>
                          <BarChart values={d.mfccVec.slice(0, 20)} color="#19d3a2" />
                        </div>
                      )}
                    </div>
                  )}

                  {/* Rhythm / Pitch / Dynamics — 3 panes */}
                  {(d.rhythm || d.pitch || dynamicsData) && (
                    <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap' }}>
                      <AudioPane data={d.rhythm} title="Rhythm" icon="♩" />
                      <AudioPane data={d.pitch} title="Pitch & tuning" icon="𝄞" />
                      <AudioPane data={dynamicsData} title="Dynamics" icon="〜" />
                    </div>
                  )}
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      {/* Tag explore modal — stacks above */}
      {exploreTag && (
        <TagExploreModal
          model={exploreTag.model}
          tag={exploreTag.tag}
          color={exploreTag.color}
          onClose={() => setExploreTag(null)}
          onOpenTrack={(id) => { setExploreTag(null); setInnerTrackId(id) }}
        />
      )}

      {/* Inner track modal — opened from TagExploreModal */}
      {innerTrackId && (
        <TrackModal trackId={innerTrackId} onClose={() => setInnerTrackId(null)} />
      )}
    </>
  )
}
