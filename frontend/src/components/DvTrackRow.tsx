import { useEffect, useState, type CSSProperties } from 'react'
import type { BriefRow } from '../apiDiversify'
import { getArt } from '../art'

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

export default function DvTrackRow({
  r, onPlay, playing,
}: { r: BriefRow; onPlay: () => void; playing: boolean }) {
  const t = r.tags || {}
  const [art, setArt] = useState<string | null>(null)
  useEffect(() => {
    let on = true
    getArt(r.jamendo_id).then((a) => { if (on) setArt(a) })
    return () => { on = false }
  }, [r.jamendo_id])
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
          cursor: 'pointer', color: '#fff', fontSize: 14,
          background: art ? '#000' : gradient(r.cyanite_id),
          backgroundImage: art ? `url(${art})` : undefined,
          backgroundSize: 'cover', backgroundPosition: 'center',
          textShadow: '0 1px 4px rgba(0,0,0,0.85)',
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
        {r.reason && <div style={{ fontSize: 11, color: '#6b6b88', marginTop: 4 }}>➤ {r.reason}</div>}
      </div>
      <div style={{ textAlign: 'right', fontSize: 11, color: '#7a7a99', flexShrink: 0, width: 76 }}>
        {t.bpm ? <div>{t.bpm} BPM</div> : null}
        <SyncBadge sync={r.sync} />
      </div>
    </div>
  )
}

const pill: CSSProperties = {
  fontSize: 11, padding: '2px 8px', borderRadius: 999, background: '#1f1f2e', color: '#9a9ab8',
}
const badge: CSSProperties = {
  fontSize: 10, padding: '2px 7px', borderRadius: 999, fontWeight: 600, display: 'inline-block',
}
