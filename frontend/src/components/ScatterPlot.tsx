import { ScatterChart, Scatter, XAxis, YAxis, Tooltip, ResponsiveContainer, Cell } from 'recharts'
import type { TrackResult } from '../api'

const GENRE_COLORS: Record<string, string> = {
  electronic: '#7c6cff', ambient: '#19d3a2', rock: '#ff3d7f',
  classical: '#ffb454', jazz: '#ff9f43', pop: '#54a0ff',
  soundtrack: '#5f27cd', metal: '#ee5a24',
}

function genreColor(track: TrackResult) {
  const genre = track.matchBreakdown
    .find(d => d.dimension.startsWith('MainGenreV2'))
    ?.dimension.split('.').pop() ?? ''
  return GENRE_COLORS[genre] ?? '#8b8b9e'
}

interface Props {
  tracks: TrackResult[]
  onSelect?: (track: TrackResult) => void
}

export default function ScatterPlot({ tracks, onSelect }: Props) {
  const plotable = tracks.filter(t => t.valence != null && t.arousal != null)
  const excluded = tracks.length - plotable.length
  const data = plotable.map(t => ({
    x: t.valence,
    y: t.arousal,
    r: Math.round(6 + t.finalScore * 8),
    track: t,
  }))

  return (
    <div style={{ width: '100%' }}>
      <div style={{ width: '100%', height: 340 }}>
        <ResponsiveContainer>
          <ScatterChart margin={{ top: 10, right: 10, bottom: 30, left: 30 }}>
            <XAxis dataKey="x" name="Valence" type="number" domain={[0, 1]}
                   label={{ value: '← negative · valence · positive →', position: 'bottom', fontSize: 11, fill: '#8b8b9e' }} tick={false} />
            <YAxis dataKey="y" name="Arousal" type="number" domain={[0, 1]}
                   label={{ value: 'arousal', angle: -90, position: 'insideLeft', fontSize: 11, fill: '#8b8b9e' }} tick={false} />
            <Tooltip
              content={({ payload }) => {
                const d = payload?.[0]?.payload
                if (!d) return null
                return (
                  <div style={{ background: '#1b1b27', border: '1px solid #272736', borderRadius: 8, padding: '8px 12px', fontSize: 12 }}>
                    <div style={{ fontWeight: 600 }}>{d.track.name}</div>
                    <div style={{ color: '#8b8b9e', marginTop: 2, maxWidth: 220 }}>{d.track.explanation}</div>
                  </div>
                )
              }}
            />
            <Scatter data={data} onClick={(d: { track: TrackResult }) => onSelect?.(d.track)}>
              {data.map((entry, i) => (
                <Cell key={i} fill={genreColor(entry.track)} r={entry.r} style={{ cursor: 'pointer', opacity: 0.8 }} />
              ))}
            </Scatter>
          </ScatterChart>
        </ResponsiveContainer>
      </div>
      {excluded > 0 && (
        <div style={{ fontSize: 10, color: '#3a3a55', textAlign: 'right', marginTop: 4 }}>
          {excluded} track{excluded > 1 ? 's' : ''} without mood data not shown
        </div>
      )}
    </div>
  )
}
