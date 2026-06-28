import { useState, useEffect } from 'react'
import { listUsers, tasteRecs, type TrackResult, type SeedTrack } from '../api'
import TrackCard from '../components/TrackCard'
import TrackModal from '../components/TrackModal'
import ScatterPlot from '../components/ScatterPlot'

type TasteResult = {
  tracks: TrackResult[]
  profileSummary: string
  profileFingerprint: { dimension: string; weight: number }[]
  seedTracks: SeedTrack[]
}

function normalizedPct(track: TrackResult, maxScore: number) {
  return maxScore > 0 ? Math.round((track.finalScore / maxScore) * 100) : 50
}

export default function TastePage() {
  const [userList, setUserList] = useState<{ id: string; n: number }[]>([])
  const [userId, setUserId] = useState('')
  const [result, setResult] = useState<TasteResult | null>(null)
  const [excludeMoods, setExcludeMoods] = useState<string[]>([])
  const [loading, setLoading] = useState(false)
  const [scatter, setScatter] = useState(false)
  const [showSeeds, setShowSeeds] = useState(false)
  const [modalId, setModalId] = useState<string | null>(null)

  useEffect(() => {
    listUsers().then(users => {
      setUserList(users)
      setUserId(users[0]?.id ?? '')
    })
  }, [])

  async function load() {
    if (!userId) return
    setLoading(true)
    setResult(null)
    try {
      const r = await tasteRecs(userId, 20, { pinIds: [], excludeMoods })
      setResult(r as TasteResult)
    } finally { setLoading(false) }
  }

  function lessOfThat(mood: string) {
    const next = [...new Set([...excludeMoods, mood])]
    setExcludeMoods(next)
    setLoading(true)
    tasteRecs(userId, 20, { pinIds: [], excludeMoods: next })
      .then(r => setResult(r as TasteResult))
      .finally(() => setLoading(false))
  }

  const maxScore = result ? Math.max(...result.tracks.map(t => t.finalScore), 0.001) : 1
  const topDims = result?.profileFingerprint?.slice(0, 6) ?? []

  return (
    <div>
      {modalId && <TrackModal trackId={modalId} onClose={() => setModalId(null)} />}

      {/* Controls */}
      <div style={{ display: 'flex', gap: 12, alignItems: 'center', marginBottom: 24, flexWrap: 'wrap' }}>
        <select
          value={userId}
          onChange={e => setUserId(e.target.value)}
          style={{
            background: '#13131f', border: '1px solid #1e1e30', color: '#eaeaf2',
            borderRadius: 10, padding: '10px 14px', fontSize: 13, minWidth: 220,
            cursor: 'pointer',
          }}
        >
          {userList.map(u => (
            <option key={u.id} value={u.id}>User {u.id} · {u.n} liked tracks</option>
          ))}
        </select>
        <button
          onClick={load}
          disabled={loading}
          style={{
            background: 'linear-gradient(135deg, #7c6cff, #ff3d7f)',
            border: 'none', color: '#fff', borderRadius: 10,
            padding: '10px 24px', cursor: loading ? 'default' : 'pointer',
            fontSize: 14, fontWeight: 700, letterSpacing: '-0.01em',
            opacity: loading ? 0.6 : 1,
          }}
        >{loading ? 'Loading…' : 'Discover'}</button>

        {excludeMoods.length > 0 && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12 }}>
            <span style={{ color: '#6b6b85' }}>Excluding:</span>
            {excludeMoods.map(m => (
              <span key={m} style={{
                background: '#ff3d7f1a', border: '1px solid #ff3d7f40',
                color: '#ff3d7f', borderRadius: 20, padding: '3px 10px',
                display: 'flex', alignItems: 'center', gap: 5,
              }}>
                {m}
                <button
                  onClick={() => setExcludeMoods(ex => ex.filter(x => x !== m))}
                  style={{ background: 'none', border: 'none', color: '#ff3d7f', cursor: 'pointer', padding: 0, fontSize: 13, lineHeight: 1 }}
                >×</button>
              </span>
            ))}
          </div>
        )}
      </div>

      {loading && !result && (
        <div style={{ color: '#555578', fontSize: 14, padding: '60px 0', textAlign: 'center' }}>
          Building your taste profile…
        </div>
      )}

      {result && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
          {/* Profile summary card */}
          <div style={{
            background: 'linear-gradient(135deg, #1a1230, #111a28)',
            border: '1px solid #2a2a42',
            borderRadius: 14, padding: '18px 22px',
            display: 'flex', gap: 32, flexWrap: 'wrap',
          }}>
            <div style={{ flex: 1, minWidth: 200 }}>
              <div style={{ fontSize: 11, color: '#555578', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 8 }}>Your Taste Profile</div>
              <div style={{ fontSize: 14, color: '#c0c0e0', lineHeight: 1.6 }}>{result.profileSummary}</div>
            </div>
            {topDims.length > 0 && (
              <div style={{ minWidth: 200, flex: 1 }}>
                <div style={{ fontSize: 11, color: '#555578', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 10 }}>Top Dimensions</div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
                  {topDims.map(({ dimension, weight }) => {
                    const label = dimension.split('.').pop() ?? dimension
                    const maxW = topDims[0]?.weight ?? 1
                    const w = Math.round((weight / maxW) * 100)
                    return (
                      <div key={dimension} style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                        <div style={{ fontSize: 11, color: '#8b8b9e', width: 100, flexShrink: 0, textOverflow: 'ellipsis', overflow: 'hidden', whiteSpace: 'nowrap' }}>{label}</div>
                        <div style={{ flex: 1, height: 4, background: '#1e1e30', borderRadius: 2 }}>
                          <div style={{ height: '100%', width: `${w}%`, background: 'linear-gradient(90deg, #7c6cff, #ff3d7f)', borderRadius: 2 }} />
                        </div>
                      </div>
                    )
                  })}
                </div>
              </div>
            )}
          </div>

          {/* Seeds (collapsed by default) */}
          {result.seedTracks.length > 0 && (
            <div>
              <button
                onClick={() => setShowSeeds(o => !o)}
                style={{
                  background: 'none', border: 'none', cursor: 'pointer',
                  fontSize: 12, color: '#555578', padding: '4px 0',
                  display: 'flex', alignItems: 'center', gap: 6,
                }}
              >
                <span style={{ fontSize: 10 }}>{showSeeds ? '▼' : '▶'}</span>
                {result.seedTracks.length} seed tracks from your library
              </button>
              {showSeeds && (
                <div style={{
                  display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))',
                  gap: 8, marginTop: 10,
                }}>
                  {result.seedTracks.map(t => (
                    <div key={t.id} style={{
                      background: '#0f0f1a', border: '1px solid #1a1a2e',
                      borderRadius: 8, padding: '10px 12px',
                    }}>
                      <div style={{ fontSize: 12, fontWeight: 600, color: '#c0c0d8', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{t.name}</div>
                      <div style={{ fontSize: 11, color: '#555578', marginTop: 1 }}>{t.artist}</div>
                      {t.audioUrl && (
                        <audio controls style={{ width: '100%', height: 24, marginTop: 6 }} src={t.audioUrl} />
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* View toggle + results */}
          <div>
            <div style={{ display: 'flex', gap: 6, marginBottom: 16 }}>
              {(['Grid', 'Scatter'] as const).map(v => (
                <button key={v} onClick={() => setScatter(v === 'Scatter')} style={{
                  fontSize: 12, padding: '5px 14px', borderRadius: 8, cursor: 'pointer',
                  background: (scatter ? 'Scatter' : 'Grid') === v ? '#1c1c2e' : 'transparent',
                  border: `1px solid ${(scatter ? 'Scatter' : 'Grid') === v ? '#2a2a42' : 'transparent'}`,
                  color: (scatter ? 'Scatter' : 'Grid') === v ? '#eaeaf2' : '#555578',
                }}>{v}</button>
              ))}
            </div>
            {scatter
              ? <ScatterPlot tracks={result.tracks} />
              : (
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))', gap: 14 }}>
                  {result.tracks.map(t => (
                    <TrackCard
                      key={t.id} track={t}
                      showSteer onExcludeMood={lessOfThat}
                      onOpenModal={setModalId}
                      displayPct={normalizedPct(t, maxScore)}
                    />
                  ))}
                </div>
              )
            }
          </div>
        </div>
      )}
    </div>
  )
}
