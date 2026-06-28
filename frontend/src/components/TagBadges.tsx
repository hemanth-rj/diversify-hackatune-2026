import type { MatchDim } from '../api'

const DIM_COLOR: Record<string, string> = {
  MainGenreV2:   '#ff3d7f',
  SubgenreV2:    '#ff3d7f',
  MoodSimpleV2:  '#7c6cff',
  MoodAdvancedV2:'#7c6cff',
  InstrumentsV2: '#19d3a2',
  CharacterV2:   '#ffb454',
  MovementV2:    '#19d3a2',
  VocalsV2:      '#7c6cff',
  MusicForV1:    '#5b9bd5',
}

function dimColor(dim: string): string {
  const model = dim.split('.')[0]
  return DIM_COLOR[model] ?? '#6b6b9e'
}

function dotColor(score: number): string | null {
  if (score < 0.25) return null
  if (score >= 0.7) return '#19d3a2'
  if (score >= 0.5) return '#ffb454'
  return '#ff5555'
}

export default function TagBadges({ breakdown }: { breakdown: MatchDim[] }) {
  if (!breakdown?.length) return null
  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3 }}>
      {breakdown.slice(0, 6).map(d => {
        const label = d.dimension.split('.').pop() ?? d.dimension
        const color  = dimColor(d.dimension)
        const score  = Math.min(1, Math.max(0, d.resultScore))
        const pct    = Math.round(score * 100)
        const op     = Math.max(0.45, Math.min(0.95, 0.38 + score * 0.62))
        const dc     = dotColor(score)
        return (
          <span key={d.dimension} style={{
            display: 'inline-flex', alignItems: 'center', gap: 4,
            background: '#13131f',
            border: '1px solid #1e1e2e',
            borderRadius: 10,
            padding: '2px 8px',
            fontSize: 11,
            opacity: op,
          }}>
            {dc && <span style={{ color: dc, fontSize: 9, lineHeight: 1 }}>●</span>}
            <span style={{ color }}>{label}</span>
            <span style={{ color: '#555578', fontSize: 10 }}>{pct}%</span>
          </span>
        )
      })}
    </div>
  )
}
