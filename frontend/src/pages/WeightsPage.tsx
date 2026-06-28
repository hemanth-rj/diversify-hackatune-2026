import { useState, useEffect } from 'react'
import {
  BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer, Cell,
  PieChart, Pie,
} from 'recharts'

interface WeightDim {
  label: string
  category: string
  shortLabel: string
  weight: number
  pct: number
}

interface CategoryTotal {
  name: string
  pct: number
  total: number
}

interface WeightsData {
  n_folds: number
  n_users: number
  dims: WeightDim[]
  category_totals: CategoryTotal[]
}

const CAT_COLORS: Record<string, string> = {
  Instruments:    '#7c6cff',
  ValenceArousal: '#19d3a2',
  Mood:           '#ff9f43',
  Genre:          '#ff3d7f',
  BPM:            '#54a0ff',
}

const CAT_LABELS: Record<string, string> = {
  Instruments:    'Instruments',
  ValenceArousal: 'Valence / Arousal',
  Mood:           'Mood',
  Genre:          'Genre',
  BPM:            'BPM',
}


export default function WeightsPage() {
  const [data, setData] = useState<WeightsData | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    fetch('/api/weights')
      .then(r => { if (!r.ok) throw new Error(`HTTP ${r.status}`); return r.json() })
      .then(d => { setData(d); setLoading(false) })
      .catch(e => { setError(e.message); setLoading(false) })
  }, [])

  if (loading) {
    return (
      <div style={{ color: '#555578', padding: '80px 0', textAlign: 'center', fontSize: 14 }}>
        Loading model weights…
      </div>
    )
  }
  if (error || !data) {
    return (
      <div style={{ color: '#ff6b6b', padding: '80px 0', textAlign: 'center', fontSize: 14 }}>
        Failed to load weights: {error}
      </div>
    )
  }

  const top25 = data.dims.slice(0, 25)
  const avgLikes = Math.round(data.n_folds / data.n_users)

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>

      {/* Stats row */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 12 }}>
        {[
          {
            value: data.n_users.toLocaleString(),
            label: 'Users',
            sub: 'with 2+ liked tracks',
          },
          {
            value: data.n_folds.toLocaleString(),
            label: 'Training examples',
            sub: `${avgLikes} avg liked tracks per user`,
          },
          {
            value: '86',
            label: 'Acoustic dimensions',
            sub: 'mood · genre · instruments · energy',
          },
        ].map(s => (
          <div key={s.label} style={{
            background: '#0f0f1a', border: '1px solid #1a1a2e',
            borderRadius: 12, padding: '16px 20px',
          }}>
            <div style={{ fontSize: 26, fontWeight: 700, color: '#eaeaf2', letterSpacing: '-0.03em' }}>
              {s.value}
            </div>
            <div style={{ fontSize: 12, color: '#8b8b9e', marginTop: 3, fontWeight: 500 }}>
              {s.label}
            </div>
            <div style={{ fontSize: 11, color: '#3a3a55', marginTop: 2 }}>
              {s.sub}
            </div>
          </div>
        ))}
      </div>

      {/* Charts row */}
      <div style={{ display: 'grid', gridTemplateColumns: '1.8fr 1fr', gap: 16 }}>

        {/* Horizontal bar chart — top 25 dims */}
        <div style={{ background: '#0f0f1a', border: '1px solid #1a1a2e', borderRadius: 14, padding: '20px 24px' }}>
          <div style={{ fontSize: 13, fontWeight: 600, color: '#eaeaf2', marginBottom: 2 }}>
            Top 25 dimensions by learned weight
          </div>
          <div style={{ fontSize: 11, color: '#555578', marginBottom: 18 }}>
            Larger weight → more influence on taste predictions
          </div>
          <ResponsiveContainer width="100%" height={490}>
            <BarChart data={top25} layout="vertical" margin={{ top: 0, right: 24, bottom: 0, left: 8 }}>
              <XAxis
                type="number"
                tick={{ fontSize: 10, fill: '#555578' }}
                tickFormatter={v => `${(v * 100).toFixed(0)}%`}
                axisLine={{ stroke: '#1a1a2e' }}
                tickLine={false}
              />
              <YAxis
                type="category"
                dataKey="shortLabel"
                tick={{ fontSize: 11, fill: '#8b8b9e' }}
                width={110}
                axisLine={false}
                tickLine={false}
              />
              <Tooltip
                cursor={{ fill: '#ffffff06' }}
                content={({ payload }: { payload?: { payload?: WeightDim }[] }) => {
                  const d = payload?.[0]?.payload
                  if (!d) return null
                  return (
                    <div style={{ background: '#1b1b27', border: '1px solid #272736', borderRadius: 8, padding: '8px 12px', fontSize: 12 }}>
                      <div style={{ fontWeight: 600, color: '#eaeaf2', marginBottom: 3 }}>{d.shortLabel}</div>
                      <div style={{ color: '#8b8b9e', marginBottom: 4 }}>{CAT_LABELS[d.category] ?? d.category}</div>
                      <div style={{ color: '#19d3a2', fontWeight: 600 }}>{d.pct.toFixed(2)}% of total weight</div>
                    </div>
                  )
                }}
              />
              <Bar
                dataKey="weight"
                radius={[0, 3, 3, 0]}
                isAnimationActive={false}
              >
                {top25.map((d, i) => (
                  <Cell key={i} fill={CAT_COLORS[d.category] ?? '#8b8b9e'} />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        </div>

        {/* Right column */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>

          {/* Category donut */}
          <div style={{
            background: '#0f0f1a', border: '1px solid #1a1a2e',
            borderRadius: 14, padding: '20px 24px', flex: 1,
            display: 'flex', flexDirection: 'column',
          }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: '#eaeaf2', marginBottom: 2 }}>
              Weight by signal type
            </div>
            <div style={{ fontSize: 11, color: '#555578', marginBottom: 14 }}>
              Which feature families matter most
            </div>

            <ResponsiveContainer width="100%" height={180}>
              <PieChart>
                <Pie
                  data={data.category_totals}
                  dataKey="pct"
                  nameKey="name"
                  cx="50%" cy="50%"
                  outerRadius={80} innerRadius={44}
                  strokeWidth={0}
                  isAnimationActive={false}
                >
                  {data.category_totals.map((d, i) => (
                    <Cell key={i} fill={CAT_COLORS[d.name] ?? '#8b8b9e'} />
                  ))}
                </Pie>
                <Tooltip
                  content={({ payload }: { payload?: { name?: string; value?: number }[] }) => {
                    const d = payload?.[0]
                    if (!d?.name) return null
                    return (
                      <div style={{ background: '#1b1b27', border: '1px solid #272736', borderRadius: 8, padding: '8px 12px', fontSize: 12 }}>
                        <div style={{ fontWeight: 600, color: '#eaeaf2', marginBottom: 4 }}>{CAT_LABELS[d.name] ?? d.name}</div>
                        <div style={{ color: '#19d3a2', fontWeight: 600 }}>{(d.value ?? 0).toFixed(1)}% of total weight</div>
                      </div>
                    )
                  }}
                />
              </PieChart>
            </ResponsiveContainer>

            <div style={{ display: 'flex', flexDirection: 'column', gap: 9, marginTop: 8 }}>
              {data.category_totals.map(c => (
                <div key={c.name} style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <div style={{
                    width: 9, height: 9, borderRadius: 2, flexShrink: 0,
                    background: CAT_COLORS[c.name] ?? '#8b8b9e',
                  }} />
                  <span style={{ fontSize: 12, color: '#8b8b9e', flex: 1 }}>
                    {CAT_LABELS[c.name] ?? c.name}
                  </span>
                  <div style={{ width: 70, height: 3, background: '#1a1a2e', borderRadius: 2, overflow: 'hidden' }}>
                    <div style={{
                      height: '100%', borderRadius: 2,
                      background: CAT_COLORS[c.name] ?? '#8b8b9e',
                      width: `${c.pct}%`,
                    }} />
                  </div>
                  <span style={{ fontSize: 12, color: '#eaeaf2', fontWeight: 600, width: 36, textAlign: 'right' }}>
                    {c.pct.toFixed(1)}%
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* How it works */}
          <div style={{
            background: '#0f0f1a', border: '1px solid #1a1a2e',
            borderRadius: 14, padding: '18px 20px',
          }}>
            <div style={{ fontSize: 12, fontWeight: 600, color: '#eaeaf2', marginBottom: 10 }}>
              How the weights were learned
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 9 }}>
              {[
                { n: '1', text: 'For each user, take one liked track and hide it' },
                { n: '2', text: 'Compute the average acoustic profile of their remaining liked tracks' },
                { n: '3', text: 'Measure how much each dimension overlaps between the profile and the hidden track' },
                { n: '4', text: 'Repeat for all users and all their likes — average overlap × IDF → final weights' },
              ].map(s => (
                <div key={s.n} style={{ display: 'flex', gap: 10, alignItems: 'flex-start' }}>
                  <div style={{
                    width: 18, height: 18, borderRadius: 5,
                    background: '#1a1a2e', color: '#555578',
                    fontSize: 10, fontWeight: 700, flexShrink: 0,
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                  }}>{s.n}</div>
                  <span style={{ fontSize: 11, color: '#6b6b85', lineHeight: 1.6 }}>{s.text}</span>
                </div>
              ))}
            </div>
            <div style={{
              marginTop: 12, paddingTop: 12, borderTop: '1px solid #13131f',
              fontSize: 11, color: '#3a3a55', lineHeight: 1.6,
            }}>
              This measures feature agreement, not retrieval rank — a dimension with high weight consistently appears in both a user's existing taste and their next liked track.
            </div>
          </div>

        </div>
      </div>
    </div>
  )
}
