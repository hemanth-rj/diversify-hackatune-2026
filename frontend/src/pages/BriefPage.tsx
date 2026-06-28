import { useEffect, useState, type CSSProperties } from 'react'
import {
  audioUrl,
  type BriefRow,
  type BriefView,
  getConfig,
  type PersonaInfo,
  postBrief,
} from '../apiDiversify'

const ACCENT = 'linear-gradient(135deg, #ff3d7f, #7c6cff)'
const EXAMPLES = [
  'tense cinematic chase, no vocals, 120-130 bpm',
  'warm nostalgic lo-fi for a coffee ad',
  'epic heroic trailer, orchestral, building',
]

function hashStr(s: string): number {
  let h = 0
  for (const c of String(s)) h = (h * 31 + c.charCodeAt(0)) >>> 0
  return h
}
function gradient(seed: string): string {
  const h = hashStr(seed)
  return `linear-gradient(135deg, hsl(${h % 360},55%,55%), hsl(${(h * 7) % 360},55%,40%))`
}

function SyncBadge({ sync }: { sync?: string | null }) {
  if (sync === 'sync-cleared')
    return <span style={{ ...badge, background: '#10331f', color: '#4ade80' }}>cleared</span>
  if (typeof sync === 'string' && sync.startsWith('restricted'))
    return <span style={{ ...badge, background: '#3a1620', color: '#f87171' }}>restricted</span>
  return null
}

function Row({ r, onPlay, playing }: { r: BriefRow; onPlay: () => void; playing: boolean }) {
  const t = r.tags || {}
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 12, padding: '10px 12px',
        borderRadius: 12, background: playing ? 'rgba(124,108,255,0.12)' : '#16161f',
        border: playing ? '1px solid rgba(124,108,255,0.4)' : '1px solid #1a1a2e',
      }}
    >
      <button
        onClick={onPlay}
        style={{
          width: 44, height: 44, borderRadius: 10, flexShrink: 0, border: 'none',
          cursor: 'pointer', color: '#fff', fontSize: 14, background: gradient(r.cyanite_id),
        }}
      >
        {playing ? '❚❚' : '▶'}
      </button>
      <div style={{ minWidth: 0, flex: 1 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 11, color: '#555578' }}>#{r.rank}</span>
          <span style={{ fontWeight: 600, fontSize: 14, color: '#eaeaf2', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
            {r.title || 'Jamendo ' + r.jamendo_id}
          </span>
        </div>
        <div style={{ fontSize: 12, color: '#7a7a99' }}>{r.artist || 'CC · Jamendo'}</div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 4 }}>
          {(t.genre || []).slice(0, 2).map((g) => (
            <span key={g} style={{ ...pill, color: '#b9aaff', background: '#221d3a' }}>{g}</span>
          ))}
          {(t.mood || []).slice(0, 2).map((m) => (
            <span key={m} style={pill}>{m}</span>
          ))}
        </div>
        {r.reason && (
          <div style={{ fontSize: 11, color: '#6b6b88', marginTop: 4 }}>➤ {r.reason}</div>
        )}
      </div>
      <div style={{ textAlign: 'right', fontSize: 11, color: '#7a7a99', flexShrink: 0, width: 76 }}>
        {t.bpm ? <div>{t.bpm} BPM</div> : null}
        <SyncBadge sync={r.sync} />
      </div>
    </div>
  )
}

export default function BriefPage() {
  const [personas, setPersonas] = useState<PersonaInfo[]>([])
  const [persona, setPersona] = useState<string>('')
  const [answers, setAnswers] = useState<Record<string, string | null>>({})
  const [showPrefs, setShowPrefs] = useState(false)
  const [brief, setBrief] = useState<string>('')
  const [view, setView] = useState<BriefView | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string>('')
  const [playJid, setPlayJid] = useState<string>('')

  useEffect(() => {
    getConfig()
      .then((c) => {
        setPersonas(c.personas)
        if (c.personas[0]) setPersona(c.personas[0].name)
      })
      .catch((e) => setError(String(e)))
  }, [])

  const current = personas.find((p) => p.name === persona)

  const run = (text: string) => {
    const q = text.trim()
    if (!q || !persona) return
    setLoading(true)
    setError('')
    postBrief(q, persona, answers)
      .then((v) => setView(v))
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false))
  }

  return (
    <div style={{ display: 'grid', gap: 20 }}>
      <div>
        <h1 style={{ fontSize: 26, fontWeight: 800, letterSpacing: '-0.03em', margin: 0, color: '#eaeaf2' }}>
          Brief → Pitch List
        </h1>
        <p style={{ color: '#7a7a99', margin: '6px 0 0', fontSize: 14 }}>
          Describe the scene. A persona-aware search returns sync-ready tracks, each with a reason.
        </p>
      </div>

      {/* persona picker */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
        {personas.map((p) => {
          const active = p.name === persona
          return (
            <button
              key={p.name}
              onClick={() => { setPersona(p.name); setAnswers({}) }}
              title={p.blurb}
              style={{
                padding: '7px 14px', borderRadius: 999, cursor: 'pointer', fontSize: 13,
                border: active ? '1px solid transparent' : '1px solid #2a2a3e',
                color: active ? '#fff' : '#9a9ab8',
                background: active ? ACCENT : 'transparent',
                fontWeight: active ? 600 : 400,
              }}
            >
              {p.name}
            </button>
          )
        })}
      </div>

      {/* persona blurb + workflow preferences */}
      {current && (
        <div style={{ display: 'grid', gap: 10, marginTop: -6 }}>
          <div style={{ fontSize: 12, color: '#7a7a99' }}>
            {current.blurb}
            {current.emphasis ? (
              <span style={{ color: '#9a9ab8' }}>
                {' · '}
                {Array.isArray(current.emphasis) ? current.emphasis.join(', ') : current.emphasis}
              </span>
            ) : null}
          </div>
          <div>
            <button
              onClick={() => setShowPrefs((s) => !s)}
              style={{ background: 'none', border: 'none', color: '#b9aaff', cursor: 'pointer', fontSize: 13, padding: 0 }}
            >
              ⚙️ {showPrefs ? 'Hide' : 'Your'} preferences (optional)
            </button>
            {showPrefs && current.questions.length > 0 && (
              <div style={{ marginTop: 8, display: 'grid', gap: 12, background: '#16161f', borderRadius: 12, padding: 14, border: '1px solid #1a1a2e' }}>
                {current.questions.map((q) => (
                  <div key={q.key}>
                    <div style={{ fontSize: 12, color: '#7a7a99', marginBottom: 4 }}>{q.label}</div>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
                      {q.options.map(([disp, val]) => {
                        const active = answers[q.key] === val || (!answers[q.key] && val === null)
                        return (
                          <button
                            key={disp}
                            onClick={() => setAnswers((a) => ({ ...a, [q.key]: val }))}
                            style={{
                              padding: '4px 12px', borderRadius: 999, fontSize: 12, cursor: 'pointer',
                              border: active ? '1px solid rgba(124,108,255,0.4)' : '1px solid #2a2a3e',
                              background: active ? 'rgba(124,108,255,0.18)' : 'transparent',
                              color: active ? '#b9aaff' : '#7a7a99',
                            }}
                          >
                            {disp}
                          </button>
                        )
                      })}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {/* brief input */}
      <div style={{ display: 'grid', gap: 10 }}>
        <textarea
          value={brief}
          onChange={(e) => setBrief(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) run(brief)
          }}
          placeholder="e.g. tense cinematic chase, no vocals, 120-130 bpm"
          rows={3}
          style={{
            width: '100%', resize: 'vertical', padding: 14, borderRadius: 12, fontSize: 14,
            background: '#16161f', color: '#eaeaf2', border: '1px solid #2a2a3e', outline: 'none',
            fontFamily: 'inherit', boxSizing: 'border-box',
          }}
        />
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <button
            onClick={() => run(brief)}
            disabled={loading || !brief.trim()}
            style={{
              padding: '9px 20px', borderRadius: 10, border: 'none', cursor: 'pointer',
              fontSize: 14, fontWeight: 600, color: '#fff', background: ACCENT,
              opacity: loading || !brief.trim() ? 0.5 : 1,
            }}
          >
            {loading ? 'Searching…' : 'Search'}
          </button>
          <span style={{ fontSize: 12, color: '#555578' }}>Try:</span>
          {EXAMPLES.map((ex) => (
            <button
              key={ex}
              onClick={() => { setBrief(ex); run(ex) }}
              style={{
                padding: '5px 10px', borderRadius: 999, cursor: 'pointer', fontSize: 12,
                border: '1px solid #2a2a3e', background: 'transparent', color: '#8a8aa8',
              }}
            >
              {ex}
            </button>
          ))}
        </div>
      </div>

      {error && <div style={{ color: '#f87171', fontSize: 13 }}>{error}</div>}

      {/* results */}
      {view && (
        <div style={{ display: 'grid', gap: 8 }}>
          {view.prompt && (
            <div style={{ fontSize: 12, color: '#7a7a99' }}>
              Searched: <span style={{ color: '#b9aaff' }}>{view.prompt}</span>
              {view.intent ? ` · ${view.intent}` : ''}
            </div>
          )}
          {view.results.map((r) => (
            <Row
              key={r.cyanite_id}
              r={r}
              playing={playJid === r.jamendo_id}
              onPlay={() => setPlayJid(playJid === r.jamendo_id ? '' : r.jamendo_id)}
            />
          ))}
          {view.results.length === 0 && (
            <div style={{ color: '#7a7a99', fontSize: 13 }}>No tracks — try a looser brief.</div>
          )}
        </div>
      )}

      {playJid && (
        <audio
          autoPlay
          controls
          src={audioUrl(playJid)}
          style={{ position: 'sticky', bottom: 12, width: '100%' }}
          onEnded={() => setPlayJid('')}
        />
      )}
    </div>
  )
}

const pill: CSSProperties = {
  fontSize: 11, padding: '2px 8px', borderRadius: 999, background: '#1f1f2e', color: '#9a9ab8',
}
const badge: CSSProperties = {
  fontSize: 10, padding: '2px 7px', borderRadius: 999, fontWeight: 600, display: 'inline-block',
}
