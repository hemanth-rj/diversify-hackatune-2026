import { useState, useRef } from 'react'
import { chatSearch, type TrackResult } from '../api'
import TrackCard from '../components/TrackCard'
import TrackModal from '../components/TrackModal'
import FeatureMap from '../components/FeatureMap'

interface Turn { role: 'user' | 'assistant'; content: string; tracks?: TrackResult[]; summary?: string; view?: 'grid' | 'map' }

const EXAMPLES = [
  'dark cinematic piano, melancholic',
  'upbeat jazz for focusing',
  'bass-heavy electronic, energetic',
  'acoustic guitar, sunny morning',
  'epic orchestral, emotional',
  'indie folk, rainy day',
]

const G = { grid: { display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))', gap: 14 } as const }

function normalizedPct(track: TrackResult, maxScore: number) {
  return maxScore > 0 ? Math.round((track.finalScore / maxScore) * 100) : 50
}

export default function ChatPage() {
  const [turns, setTurns] = useState<Turn[]>([])
  const [input, setInput] = useState('')
  const [loading, setLoading] = useState(false)
  const [modalId, setModalId] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)

  async function send(text?: string) {
    const msg = (text ?? input).trim()
    if (!msg || loading) return
    setInput('')
    const newTurns = [...turns, { role: 'user' as const, content: msg }]
    setTurns(newTurns)
    setLoading(true)
    try {
      const messages = newTurns.map(t => ({ role: t.role, content: t.content }))
      const res = await chatSearch(messages, 10)
      setTurns(prev => [
        ...prev,
        { role: 'assistant', content: res.filterSummary, tracks: res.tracks, summary: res.filterSummary },
      ])
      setTimeout(() => bottomRef.current?.scrollIntoView({ behavior: 'smooth' }), 50)
    } catch {
      setTurns(prev => [...prev, { role: 'assistant', content: 'Search failed. Try again.' }])
    } finally {
      setLoading(false)
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }

  const isEmpty = turns.length === 0 && !loading

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 0, minHeight: 'calc(100vh - 58px - 64px)' }}>
      {modalId && <TrackModal trackId={modalId} onClose={() => setModalId(null)} />}

      {/* Empty state */}
      {isEmpty && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'flex-start', justifyContent: 'flex-start', gap: 20, padding: '0 0 32px' }}>
          <div>
            <h1 style={{ fontSize: 26, fontWeight: 800, letterSpacing: '-0.03em', margin: 0, color: '#eaeaf2' }}>
              What are you in the mood for?
            </h1>
            <p style={{ fontSize: 14, color: '#7a7a99', margin: '6px 0 0' }}>
              Describe a vibe, emotion, or genre — we'll find the perfect tracks.
            </p>
          </div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10 }}>
            {EXAMPLES.map(ex => (
              <button key={ex} onClick={() => send(ex)} style={{
                padding: '9px 18px', borderRadius: 999, cursor: 'pointer',
                fontSize: 13, fontWeight: 500,
                background: '#1a1a2e', border: '1px solid #2a2a42',
                color: '#aaaacc',
                transition: 'all 0.15s',
              }}
              onMouseEnter={e => {
                (e.currentTarget as HTMLButtonElement).style.borderColor = '#7c6cff66'
                ;(e.currentTarget as HTMLButtonElement).style.color = '#eaeaf2'
              }}
              onMouseLeave={e => {
                (e.currentTarget as HTMLButtonElement).style.borderColor = '#2a2a42'
                ;(e.currentTarget as HTMLButtonElement).style.color = '#aaaacc'
              }}
              >{ex}</button>
            ))}
          </div>
        </div>
      )}

      {/* Chat thread */}
      {!isEmpty && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 28, marginBottom: 24 }}>
          {turns.map((turn, i) => (
            <div key={i}>
              {turn.role === 'user' ? (
                <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
                  <div style={{
                    background: 'linear-gradient(135deg, #ff3d7f22, #7c6cff22)',
                    border: '1px solid #7c6cff40',
                    borderRadius: '14px 14px 4px 14px',
                    padding: '10px 16px', maxWidth: '65%',
                    fontSize: 14, color: '#eaeaf2',
                  }}>{turn.content}</div>
                </div>
              ) : (
                <div>
                  {turn.summary && (
                    <div style={{
                      fontSize: 12, color: '#7c6cff', marginBottom: 14,
                      display: 'flex', alignItems: 'center', gap: 8,
                    }}>
                      <span style={{ width: 6, height: 6, borderRadius: '50%', background: '#7c6cff', flexShrink: 0, display: 'inline-block' }} />
                      {turn.summary}
                    </div>
                  )}
                  {turn.tracks && turn.tracks.length === 0 && (
                    <div style={{
                      fontSize: 13, color: '#555578', padding: '14px 18px',
                      background: '#0f0f1a', border: '1px solid #1a1a2e',
                      borderRadius: 10, lineHeight: 1.6,
                    }}>
                      No tracks found for this vibe. Try describing it differently — more specific moods, instruments, or genres tend to work best.
                    </div>
                  )}
                  {turn.tracks && turn.tracks.length > 0 && (() => {
                    const max = Math.max(...turn.tracks.map(t => t.finalScore), 0.001)
                    const view = turn.view ?? 'grid'
                    return (
                      <div>
                        {/* View toggle */}
                        <div style={{ display: 'flex', gap: 4, marginBottom: 14 }}>
                          {(['grid', 'map'] as const).map(v => (
                            <button key={v} onClick={() => setTurns(prev => prev.map((t2, j2) => j2 === i ? { ...t2, view: v } : t2))}
                              style={{
                                fontSize: 11, padding: '4px 12px', borderRadius: 8, cursor: 'pointer',
                                background: view === v ? '#1c1c2e' : 'transparent',
                                border: `1px solid ${view === v ? '#2a2a42' : 'transparent'}`,
                                color: view === v ? '#eaeaf2' : '#555578',
                              }}>
                              {v === 'grid' ? '⊞ Grid' : '◎ Mood map'}
                            </button>
                          ))}
                        </div>
                        {view === 'map' ? (
                          <FeatureMap tracks={turn.tracks} maxScore={max} onOpenModal={setModalId} />
                        ) : (
                          <div style={G.grid}>
                            {turn.tracks.map(t => (
                              <TrackCard
                                key={t.id} track={t}
                                onOpenModal={setModalId}
                                displayPct={normalizedPct(t, max)}
                              />
                            ))}
                          </div>
                        )}
                      </div>
                    )
                  })()}
                </div>
              )}
            </div>
          ))}
          {loading && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, color: '#555578', fontSize: 13 }}>
              <span style={{ display: 'flex', gap: 4 }}>
                {[0,1,2].map(i => (
                  <span key={i} style={{
                    width: 6, height: 6, borderRadius: '50%', background: '#7c6cff',
                    opacity: 0.4,
                    animation: `pulse 1.2s ease-in-out ${i * 0.2}s infinite`,
                  }} />
                ))}
              </span>
              Searching…
            </div>
          )}
          <div ref={bottomRef} />
        </div>
      )}

      {/* Input — stays below content */}
      <div style={{
        display: 'flex', gap: 10, marginTop: isEmpty ? 0 : 'auto',
        padding: isEmpty ? 0 : '8px 0',
      }}>
        <input
          ref={inputRef}
          style={{
            flex: 1, background: '#13131f', border: '1px solid #1e1e30',
            color: '#eaeaf2', borderRadius: 12, padding: '13px 18px',
            fontSize: 14, outline: 'none',
          }}
          onFocus={e => { e.currentTarget.style.borderColor = '#7c6cff60' }}
          onBlur={e => { e.currentTarget.style.borderColor = '#1e1e30' }}
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && send()}
          placeholder="Describe a vibe… 'melancholic strings at 3am'"
        />
        <button
          onClick={() => send()}
          disabled={loading || !input.trim()}
          style={{
            background: 'linear-gradient(135deg, #ff3d7f, #7c6cff)',
            border: 'none', color: '#fff', borderRadius: 12,
            padding: '13px 22px', cursor: loading || !input.trim() ? 'default' : 'pointer',
            fontSize: 14, fontWeight: 600,
            opacity: loading || !input.trim() ? 0.4 : 1,
            transition: 'opacity 0.15s',
          }}
        >Send</button>
      </div>

      <style>{`
        @keyframes pulse { 0%,100% { opacity:0.3; transform:scale(0.8); } 50% { opacity:1; transform:scale(1.2); } }
      `}</style>
    </div>
  )
}
