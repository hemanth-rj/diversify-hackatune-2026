import { useState } from 'react'
import type { TrackResult } from '../api'

function matchColor(pct: number) {
  if (pct >= 75) return '#19d3a2'
  if (pct >= 50) return '#7c6cff'
  if (pct >= 30) return '#ffb454'
  return '#8b8b9e'
}

interface Props {
  tracks: TrackResult[]
  maxScore: number
  onOpenModal: (id: string) => void
}

const W = 600, H = 380
const PAD = { top: 28, right: 24, bottom: 52, left: 52 }
const IW = W - PAD.left - PAD.right
const IH = H - PAD.top - PAD.bottom

export default function FeatureMap({ tracks, maxScore, onOpenModal }: Props) {
  const [hoveredId, setHoveredId] = useState<string | null>(null)

  // Only plot tracks with va data; tracks without va cluster at centre
  const plotable = tracks.filter(t => t.valence != null && t.arousal != null)
  const noVa = tracks.filter(t => t.valence == null || t.arousal == null)

  const toX = (v: number) => PAD.left + v * IW
  const toY = (a: number) => PAD.top + (1 - a) * IH

  return (
    <div style={{ width: '100%' }}>
      <svg
        width="100%"
        viewBox={`0 0 ${W} ${H}`}
        style={{ display: 'block', borderRadius: 12, background: '#08080f' }}
      >
        {/* Quadrant background fills */}
        <rect x={PAD.left} y={PAD.top} width={IW / 2} height={IH / 2} fill="#ff555508" />
        <rect x={PAD.left + IW / 2} y={PAD.top} width={IW / 2} height={IH / 2} fill="#19d3a208" />
        <rect x={PAD.left} y={PAD.top + IH / 2} width={IW / 2} height={IH / 2} fill="#7c6cff08" />
        <rect x={PAD.left + IW / 2} y={PAD.top + IH / 2} width={IW / 2} height={IH / 2} fill="#ffb45408" />

        {/* Grid */}
        {[0.25, 0.5, 0.75].map(v => (
          <line key={`vg${v}`} x1={toX(v)} y1={PAD.top} x2={toX(v)} y2={PAD.top + IH}
            stroke="#1a1a2e" strokeWidth={1} />
        ))}
        {[0.25, 0.5, 0.75].map(a => (
          <line key={`ag${a}`} x1={PAD.left} y1={toY(a)} x2={PAD.left + IW} y2={toY(a)}
            stroke="#1a1a2e" strokeWidth={1} />
        ))}

        {/* Centre axes */}
        <line x1={PAD.left} y1={toY(0.5)} x2={PAD.left + IW} y2={toY(0.5)}
          stroke="#252538" strokeWidth={1.5} />
        <line x1={toX(0.5)} y1={PAD.top} x2={toX(0.5)} y2={PAD.top + IH}
          stroke="#252538" strokeWidth={1.5} />

        {/* Quadrant corner labels */}
        <text x={PAD.left + 6} y={PAD.top + 14} fill="#3a3a55" fontSize={9}>tense · dark</text>
        <text x={PAD.left + IW - 6} y={PAD.top + 14} textAnchor="end" fill="#3a3a55" fontSize={9}>excited · uplifting</text>
        <text x={PAD.left + 6} y={PAD.top + IH - 6} fill="#3a3a55" fontSize={9}>melancholic</text>
        <text x={PAD.left + IW - 6} y={PAD.top + IH - 6} textAnchor="end" fill="#3a3a55" fontSize={9}>serene · peaceful</text>

        {/* Axis labels */}
        <text x={PAD.left + IW / 2} y={H - 8} textAnchor="middle" fill="#555578" fontSize={10}>
          ← negative valence · positive →
        </text>
        <text
          x={14} y={PAD.top + IH / 2}
          textAnchor="middle" fill="#555578" fontSize={10}
          transform={`rotate(-90, 14, ${PAD.top + IH / 2})`}
        >
          ← calm · energetic →
        </text>

        {/* Track dots */}
        {plotable.map(t => {
          const pct = maxScore > 0 ? Math.round((t.finalScore / maxScore) * 100) : 50
          const color = matchColor(pct)
          const x = toX(t.valence!)
          const y = toY(t.arousal!)
          const isH = hoveredId === t.id

          // Tooltip position — keep within SVG bounds
          const tipW = 160, tipH = 44
          let tipX = x - tipW / 2
          let tipY = y - tipH - 10
          if (tipX < PAD.left) tipX = PAD.left
          if (tipX + tipW > W - PAD.right) tipX = W - PAD.right - tipW
          if (tipY < PAD.top) tipY = y + 14

          return (
            <g key={t.id} style={{ cursor: 'pointer' }}
              onClick={() => onOpenModal(t.id)}
              onMouseEnter={() => setHoveredId(t.id)}
              onMouseLeave={() => setHoveredId(null)}>
              {/* Glow ring */}
              {isH && <circle cx={x} cy={y} r={18} fill={color} opacity={0.12} />}
              <circle cx={x} cy={y} r={isH ? 9 : 6}
                fill={color} fillOpacity={isH ? 1 : 0.75}
                stroke="#08080f" strokeWidth={isH ? 2 : 1.5} />
              {/* Score ring */}
              {isH && (
                <circle cx={x} cy={y} r={12}
                  fill="none" stroke={color} strokeWidth={1} strokeOpacity={0.4} />
              )}
              {/* Tooltip */}
              {isH && (
                <g>
                  <rect x={tipX} y={tipY} width={tipW} height={tipH}
                    rx={6} fill="#13131f" stroke={color} strokeWidth={0.75} strokeOpacity={0.6} />
                  <text x={tipX + tipW / 2} y={tipY + 15} textAnchor="middle"
                    fill={color} fontSize={11} fontWeight={700} style={{ pointerEvents: 'none' }}>
                    {t.name.length > 24 ? t.name.slice(0, 24) + '…' : t.name}
                  </text>
                  <text x={tipX + tipW / 2} y={tipY + 30} textAnchor="middle"
                    fill="#6b6b85" fontSize={9.5} style={{ pointerEvents: 'none' }}>
                    {t.artist} · {pct}% match
                  </text>
                </g>
              )}
            </g>
          )
        })}
      </svg>

      {/* Footer row */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 8, padding: '0 4px' }}>
        {/* Legend */}
        <div style={{ display: 'flex', gap: 14 }}>
          {([['≥75%', '#19d3a2'], ['≥50%', '#7c6cff'], ['≥30%', '#ffb454'], ['lower', '#8b8b9e']] as const).map(([label, color]) => (
            <div key={label} style={{ display: 'flex', alignItems: 'center', gap: 5, fontSize: 10, color: '#555578' }}>
              <div style={{ width: 7, height: 7, borderRadius: '50%', background: color }} />
              {label}
            </div>
          ))}
        </div>
        {noVa.length > 0 && (
          <div style={{ fontSize: 10, color: '#3a3a55' }}>
            {noVa.length} track{noVa.length > 1 ? 's' : ''} without mood data not shown
          </div>
        )}
      </div>
    </div>
  )
}
