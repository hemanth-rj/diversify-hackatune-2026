import {
  RadarChart, PolarGrid, PolarAngleAxis, Radar,
  ScatterChart, Scatter, XAxis, YAxis, ReferenceLine,
  BarChart, Bar, Cell, ResponsiveContainer,
} from 'recharts'
import type { TasteViz } from '../apiDiversify'

const MOODS = ['aggressive', 'calm', 'chill', 'dark', 'energetic', 'epic', 'happy',
  'romantic', 'sad', 'scary', 'sexy', 'ethereal', 'uplifting']

export default function DvTasteCard({ viz, name }: { viz: TasteViz; name: string }) {
  const moodData = MOODS.map((m) => ({ mood: m, v: viz.mood?.[m] || 0 }))
  const emo = [{ x: viz.valence || 0, y: viz.arousal || 0 }]
  const inst = (viz.instruments || []).map(([n, c]) => ({ name: n, c }))

  return (
    <div style={{ background: '#16161f', borderRadius: 16, padding: 16, border: '1px solid #1a1a2e' }}>
      <div style={{ fontWeight: 600, marginBottom: 8, color: '#eaeaf2' }}>{name}'s taste</div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 8 }}>
        <div style={{ height: 176 }}>
          <div style={{ fontSize: 12, color: '#7a7a99', textAlign: 'center', marginBottom: 4 }}>Emotion</div>
          <ResponsiveContainer width="100%" height="85%">
            <ScatterChart margin={{ top: 8, right: 10, bottom: 8, left: 0 }}>
              <XAxis type="number" dataKey="x" domain={[-1, 1]} hide />
              <YAxis type="number" dataKey="y" domain={[-1, 1]} hide />
              <ReferenceLine x={0} stroke="#2a3142" />
              <ReferenceLine y={0} stroke="#2a3142" />
              <Scatter data={emo} fill="#ff3d7f" />
            </ScatterChart>
          </ResponsiveContainer>
        </div>

        <div style={{ height: 176 }}>
          <div style={{ fontSize: 12, color: '#7a7a99', textAlign: 'center', marginBottom: 4 }}>Mood</div>
          <ResponsiveContainer width="100%" height="85%">
            <RadarChart data={moodData} outerRadius="72%">
              <PolarGrid stroke="#222a38" />
              <PolarAngleAxis dataKey="mood" tick={{ fontSize: 7, fill: '#7a8190' }} />
              <Radar dataKey="v" stroke="#7c6cff" fill="#7c6cff" fillOpacity={0.35} />
            </RadarChart>
          </ResponsiveContainer>
        </div>

        <div style={{ height: 176 }}>
          <div style={{ fontSize: 12, color: '#7a7a99', textAlign: 'center', marginBottom: 4 }}>Top instruments</div>
          <ResponsiveContainer width="100%" height="85%">
            <BarChart data={inst} layout="vertical" margin={{ left: 6, right: 6 }}>
              <XAxis type="number" hide />
              <YAxis type="category" dataKey="name" width={72} tick={{ fontSize: 9, fill: '#9aa0ab' }} />
              <Bar dataKey="c" radius={4}>
                {inst.map((_, i) => <Cell key={i} fill="#2ecc71" />)}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        </div>
      </div>
    </div>
  )
}
