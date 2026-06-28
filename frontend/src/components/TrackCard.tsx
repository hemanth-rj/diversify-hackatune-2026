import { useEffect, useRef, useState } from 'react'
import { audioSrc, type TrackResult } from '../api'
import { getArt } from '../art'
import TagBadges from './TagBadges'

function scoreColor(pct: number) {
  if (pct >= 75) return '#19d3a2'
  if (pct >= 50) return '#7c6cff'
  if (pct >= 30) return '#ffb454'
  return '#8b8b9e'
}

interface Props {
  track: TrackResult
  onPin?: (id: string) => void
  onExcludeMood?: (mood: string) => void
  showSteer?: boolean
  onOpenModal?: (id: string) => void
  /** Pre-normalized 0-100 display score (if caller knows the max). Falls back to clamped finalScore. */
  displayPct?: number
}

export default function TrackCard({ track, onPin, onExcludeMood, showSteer, onOpenModal, displayPct }: Props) {
  const audioRef = useRef<HTMLAudioElement>(null)
  const [audioFailed, setAudioFailed] = useState(false)
  const [hovered, setHovered] = useState(false)
  const [art, setArt] = useState<string | null>(null)
  useEffect(() => {
    let on = true
    getArt(track.jamendoId).then((a) => { if (on) setArt(a) })
    return () => { on = false }
  }, [track.jamendoId])

  const pct = displayPct ?? Math.min(100, Math.max(0, Math.round(track.finalScore * 100)))
  const color = scoreColor(pct)
  const topMood = track.matchBreakdown.find(d => d.dimension.startsWith('MoodSimpleV2'))
  const moodLabel = topMood?.dimension.split('.').pop() ?? ''

  return (
    <div
      onClick={() => onOpenModal?.(track.id)}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        background: '#13131f',
        border: `1px solid ${hovered && onOpenModal ? '#2e2e50' : '#1a1a2e'}`,
        borderRadius: 12,
        overflow: 'hidden',
        display: 'flex', flexDirection: 'column',
        cursor: onOpenModal ? 'pointer' : 'default',
        boxShadow: hovered && onOpenModal
          ? `0 0 0 1px ${color}28, 0 8px 32px rgba(0,0,0,0.5)`
          : '0 2px 12px rgba(0,0,0,0.3)',
        transform: hovered && onOpenModal ? 'translateY(-2px)' : 'none',
        transition: 'transform 0.18s ease, box-shadow 0.18s ease, border-color 0.18s ease',
      }}
    >
      <div style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 10, flex: 1 }}>
        {/* Name + score badge */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 10 }}>
          {art && (
            <img src={art} alt="" style={{ width: 40, height: 40, borderRadius: 6, objectFit: 'cover', flexShrink: 0 }} />
          )}
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{
              fontSize: 14, fontWeight: 700, color: '#eaeaf2',
              whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
              letterSpacing: '-0.01em',
            }}>{track.name}</div>
            <div style={{ fontSize: 12, color: '#6b6b85', marginTop: 2 }}>{track.artist}</div>
          </div>
          <div style={{
            flexShrink: 0,
            fontSize: 11, fontWeight: 700,
            color,
            background: color + '1a',
            border: `1px solid ${color}35`,
            borderRadius: 20,
            padding: '3px 9px',
            letterSpacing: '0.01em',
          }}>{pct}%</div>
        </div>

        {/* Audio player */}
        {track.audioUrl && !audioFailed && (
          <audio
            ref={audioRef}
            controls
            style={{ width: '100%', height: 28, borderRadius: 4 }}
            src={audioSrc(track.audioUrl)}
            onError={() => setAudioFailed(true)}
            onClick={e => e.stopPropagation()}
            onLoadedMetadata={() => {
              if (audioRef.current && track.representativeSegmentStart > 0) {
                audioRef.current.currentTime = track.representativeSegmentStart
              }
            }}
          />
        )}
        {audioFailed && (
          <div style={{
            fontSize: 11, color: '#3a3a55', textAlign: 'center',
            padding: '5px 0', borderRadius: 4, background: '#0d0d1a',
            border: '1px solid #1a1a2e',
          }}>preview unavailable</div>
        )}

        {/* Tags */}
        <TagBadges breakdown={track.matchBreakdown} />

        {/* Steer controls */}
        {showSteer && (onPin || onExcludeMood) && (
          <div style={{ display: 'flex', gap: 6, marginTop: 2 }} onClick={e => e.stopPropagation()}>
            {onPin && (
              <button onClick={() => onPin(track.id)} style={{
                fontSize: 11, padding: '4px 10px', borderRadius: 20, cursor: 'pointer',
                background: '#7c6cff1a', border: '1px solid #7c6cff40', color: '#7c6cff',
                fontWeight: 500,
              }}>+ More like this</button>
            )}
            {onExcludeMood && moodLabel && (
              <button onClick={() => onExcludeMood(moodLabel)} style={{
                fontSize: 11, padding: '4px 10px', borderRadius: 20, cursor: 'pointer',
                background: '#ff3d7f1a', border: '1px solid #ff3d7f40', color: '#ff3d7f',
                fontWeight: 500,
              }}>− Less {moodLabel}</button>
            )}
          </div>
        )}

        {onOpenModal && hovered && (
          <div style={{ fontSize: 10, color: '#3a3a55', textAlign: 'center' }}>
            tap for full track details
          </div>
        )}
      </div>
    </div>
  )
}
