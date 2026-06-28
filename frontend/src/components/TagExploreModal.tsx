import { useEffect, useState } from 'react'
import { fetchTagTracks, type TagTrack } from '../api'

interface Props {
  model: string
  tag: string
  color: string
  onClose: () => void
  onOpenTrack: (id: string) => void
}

const CAT_LABEL: Record<string, string> = {
  MoodSimpleV2: 'mood', MoodAdvancedV2: 'mood', CharacterV2: 'character',
  MainGenreV2: 'genre', SubgenreV2: 'genre', FreeGenreV3: 'genre',
  InstrumentsV2: 'instrument', MovementV2: 'movement',
  VocalsV2: 'vocals', MusicForV1: 'music for',
}

const BINARY_MODELS = new Set(['InstrumentsV2', 'FreeGenreV3'])

function MiniTrackCard({
  track, color, onOpen, isBinary,
}: { track: TagTrack; color: string; onOpen: () => void; isBinary: boolean }) {
  const [hovered, setHovered] = useState(false)
  const [audioFailed, setAudioFailed] = useState(false)
  const pct = Math.round(track.tagScore * 100)

  return (
    <div
      onClick={onOpen}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        background: '#13131f',
        border: `1px solid ${hovered ? color + '55' : '#1a1a2e'}`,
        borderRadius: 10, overflow: 'hidden',
        cursor: 'pointer',
        boxShadow: hovered ? `0 4px 20px rgba(0,0,0,0.5), 0 0 0 1px ${color}22` : '0 2px 8px rgba(0,0,0,0.3)',
        transform: hovered ? 'translateY(-1px)' : 'none',
        transition: 'all 0.15s ease',
      }}
    >
      {/* Score bar */}
      <div style={{ height: 3, background: '#1a1a2e' }}>
        <div style={{ height: '100%', width: isBinary ? '100%' : `${pct}%`, background: color, opacity: isBinary ? 0.4 : 0.8 }} />
      </div>
      <div style={{ padding: '10px 12px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 8, marginBottom: 6 }}>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: 13, fontWeight: 700, color: '#eaeaf2', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', letterSpacing: '-0.01em' }}>
              {track.name}
            </div>
            <div style={{ fontSize: 11, color: '#6b6b85', marginTop: 1 }}>{track.artist}</div>
          </div>
          <div style={{
            flexShrink: 0, fontSize: 11, fontWeight: 700, color,
            background: color + '1a', border: `1px solid ${color}35`,
            borderRadius: 20, padding: '2px 8px',
          }}>{isBinary ? '✓' : `${pct}%`}</div>
        </div>
        {track.audioUrl && !audioFailed && (
          <audio
            controls
            style={{ width: '100%', height: 26 }}
            src={track.audioUrl}
            onError={() => setAudioFailed(true)}
            onClick={e => e.stopPropagation()}
          />
        )}
      </div>
    </div>
  )
}

export default function TagExploreModal({ model, tag, color, onClose, onOpenTrack }: Props) {
  const [tracks, setTracks] = useState<TagTrack[]>([])
  const [loading, setLoading] = useState(true)
  const catLabel = CAT_LABEL[model] ?? model
  const isBinary = BINARY_MODELS.has(model)

  useEffect(() => {
    fetchTagTracks(model, tag, 15)
      .then(r => setTracks(r.tracks))
      .finally(() => setLoading(false))
  }, [model, tag])

  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [onClose])

  return (
    <div
      style={{
        position: 'fixed', inset: 0,
        background: 'rgba(2,2,12,0.82)',
        zIndex: 1100, display: 'flex',
        alignItems: 'flex-start', justifyContent: 'center',
        padding: '28px 16px', overflowY: 'auto',
        backdropFilter: 'blur(12px)',
      }}
      onClick={e => { if (e.target === e.currentTarget) onClose() }}
    >
      <div style={{
        background: '#0e0e1b',
        borderRadius: 14, width: '100%', maxWidth: 680,
        flexShrink: 0, overflow: 'hidden',
        border: '1px solid #1c1c2e',
        boxShadow: '0 32px 80px rgba(0,0,0,0.8)',
      }}>
        {/* Accent bar in tag color */}
        <div style={{ height: 3, background: `linear-gradient(90deg, ${color}, ${color}55)` }} />

        <div style={{ padding: '20px 22px' }}>
          {/* Header */}
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 16 }}>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                <span style={{
                  fontSize: 10, color, background: color + '18',
                  border: `1px solid ${color}33`, borderRadius: 20,
                  padding: '2px 8px', textTransform: 'uppercase', letterSpacing: '0.07em',
                }}>{catLabel}</span>
                <span style={{ fontSize: 10, color: '#3a3a55' }}>{model}</span>
              </div>
              <div style={{ fontSize: 22, fontWeight: 800, color, letterSpacing: '-0.03em' }}>{tag}</div>
              <div style={{ fontSize: 12, color: '#555578', marginTop: 4 }}>
                {loading ? '…' : tracks.length} tracks {isBinary ? 'with this feature detected' : 'by Cyanite confidence'} · click any to view full details
              </div>
            </div>
            <button onClick={onClose} style={{
              background: '#181828', border: '1px solid #2a2a3e',
              color: '#555578', cursor: 'pointer',
              borderRadius: 8, width: 30, height: 30,
              fontSize: 16, display: 'flex', alignItems: 'center', justifyContent: 'center',
              flexShrink: 0,
            }}>✕</button>
          </div>

          {loading ? (
            <div style={{ color: '#555578', fontSize: 13, padding: '40px 0', textAlign: 'center' }}>Loading…</div>
          ) : tracks.length === 0 ? (
            <div style={{ color: '#555578', fontSize: 13, padding: '40px 0', textAlign: 'center' }}>No tracks found.</div>
          ) : (
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
              {tracks.map(t => (
                <MiniTrackCard key={t.id} track={t} color={color} isBinary={isBinary} onOpen={() => onOpenTrack(t.id)} />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
