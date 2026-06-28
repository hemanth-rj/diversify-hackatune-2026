import { useEffect, useState } from 'react'
import {
  audioUrl,
  type BriefView,
  getConfig,
  getListener,
  type ListenerProfile,
  postStream,
} from '../apiDiversify'
import DvTasteCard from '../components/DvTasteCard'
import DvTrackRow from '../components/DvTrackRow'

const ACCENT = 'linear-gradient(135deg, #ff3d7f, #7c6cff)'

type Steers = Record<string, string | null>
const KNOBS: [string, string, [string, string | null][]][] = [
  ['Energy', 'energy', [['Any', null], ['Low', 'low'], ['Med', 'medium'], ['High', 'high']]],
  ['Tempo', 'tempo', [['Any', null], ['Slower', 'slower'], ['Faster', 'faster']]],
  ['Vocals', 'vocals', [['Any', null], ['Instrumental', 'instrumental']]],
  ['Mood', 'mood', [['None', null], ['Brighter', 'brighter'], ['Darker', 'darker'], ['Dreamy', 'dreamy'], ['Intense', 'intense']]],
  ['Add instrument', 'instrument', [['None', null], ['Piano', 'piano'], ['Guitar', 'guitar'], ['Synth', 'synth'], ['Strings', 'strings']]],
]

function gradient(seed: string): string {
  let h = 0
  for (const c of String(seed)) h = (h * 31 + c.charCodeAt(0)) >>> 0
  return `linear-gradient(135deg, hsl(${h % 360},60%,55%), hsl(${(h * 7) % 360},60%,40%))`
}

export default function DiversifyTastePage() {
  const [listeners, setListeners] = useState<string[]>([])
  const [name, setName] = useState<string>('')
  const [prof, setProf] = useState<ListenerProfile | null>(null)
  const [persona, setPersona] = useState<string>('')
  const [steers, setSteers] = useState<Steers>({})
  const [showSamples, setShowSamples] = useState(false)
  const [view, setView] = useState<BriefView | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string>('')
  const [playJid, setPlayJid] = useState<string>('')

  useEffect(() => {
    getConfig().then((c) => setListeners(c.listeners)).catch((e) => setError(String(e)))
  }, [])

  const pick = (n: string) => {
    setName(n)
    setProf(null)
    setView(null)
    setLoading(true)
    setError('')
    getListener(n)
      .then((p) => { setProf(p); setPersona(p.persona || ''); setSteers({}) })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false))
  }

  const gen = () => {
    if (!name) return
    setLoading(true)
    setError('')
    postStream(name, steers, persona)
      .then((v) => setView(v))
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false))
  }

  return (
    <div style={{ display: 'grid', gap: 20 }}>
      <div>
        <h1 style={{ fontSize: 26, fontWeight: 800, letterSpacing: '-0.03em', margin: 0, color: '#eaeaf2' }}>
          Explore by taste
        </h1>
        <p style={{ color: '#7a7a99', margin: '6px 0 0', fontSize: 14 }}>
          Pick a listener — their taste, learned from the sound of what they like.
        </p>
      </div>

      {/* listener tiles */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(5, 1fr)', gap: 12 }}>
        {listeners.map((n) => (
          <button
            key={n}
            onClick={() => pick(n)}
            style={{
              borderRadius: 12, overflow: 'hidden', cursor: 'pointer', padding: 0, textAlign: 'left',
              border: name === n ? '1px solid #ff3d7f' : '1px solid #2a2a3e', background: '#16161f',
            }}
          >
            <div style={{ height: 64, background: gradient(n) }} />
            <div style={{ padding: '8px 10px' }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: '#eaeaf2' }}>{n}</div>
              <div style={{ fontSize: 11, color: '#555578' }}>listener</div>
            </div>
          </button>
        ))}
      </div>

      {error && <div style={{ color: '#f87171', fontSize: 13 }}>{error}</div>}
      {loading && !prof && <div style={{ color: '#7a7a99', fontSize: 13 }}>Loading…</div>}

      {prof && (
        <div style={{ display: 'grid', gap: 16 }}>
          <DvTasteCard viz={prof.viz} name={name} />

          <div>
            <div style={{ fontSize: 12, color: '#7a7a99', marginBottom: 4 }}>
              📝 Taste in words (editable — steers the stream)
            </div>
            <textarea
              value={persona}
              onChange={(e) => setPersona(e.target.value)}
              rows={3}
              style={{
                width: '100%', resize: 'vertical', padding: 12, borderRadius: 12, fontSize: 14,
                background: '#16161f', color: '#eaeaf2', border: '1px solid #2a2a3e', outline: 'none',
                fontFamily: 'inherit', boxSizing: 'border-box',
              }}
            />
          </div>

          <div>
            <button
              onClick={() => setShowSamples((s) => !s)}
              style={{ background: 'none', border: 'none', color: '#b9aaff', cursor: 'pointer', fontSize: 13, padding: 0 }}
            >
              🎧 Hear their taste — 5 of their likes
            </button>
            {showSamples && (
              <div style={{ marginTop: 8, display: 'grid', gap: 8, background: '#16161f', borderRadius: 12, padding: 12 }}>
                {(prof.samples || []).slice(0, 5).map((s) => (
                  <div key={s.cyanite_id}>
                    <div style={{ fontSize: 12, color: '#c7c7d9' }}>{s.artist} — {s.title}</div>
                    <audio controls src={audioUrl(s.jamendo)} style={{ width: '100%', height: 32, marginTop: 4 }} />
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* steering knobs */}
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 12 }}>
            {KNOBS.map(([label, key, opts]) => (
              <div key={key}>
                <div style={{ fontSize: 12, color: '#7a7a99', marginBottom: 4 }}>{label}</div>
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
                  {opts.map(([disp, val]) => {
                    const active = steers[key] === val || (!steers[key] && val === null)
                    return (
                      <button
                        key={disp}
                        onClick={() => setSteers((s) => ({ ...s, [key]: val }))}
                        style={{
                          padding: '4px 10px', borderRadius: 999, fontSize: 12, cursor: 'pointer',
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

          <button
            onClick={gen}
            disabled={loading}
            style={{
              justifySelf: 'start', padding: '9px 22px', borderRadius: 10, border: 'none', cursor: 'pointer',
              fontSize: 14, fontWeight: 600, color: '#fff', background: ACCENT, opacity: loading ? 0.5 : 1,
            }}
          >
            {loading ? 'Generating…' : `▶ Generate ${name}'s stream`}
          </button>

          {view && (
            <div style={{ display: 'grid', gap: 8 }}>
              {view.results.map((r) => (
                <DvTrackRow
                  key={r.cyanite_id}
                  r={r}
                  playing={playJid === r.jamendo_id}
                  onPlay={() => setPlayJid(playJid === r.jamendo_id ? '' : r.jamendo_id)}
                />
              ))}
            </div>
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
