import { useEffect, useState } from 'react'
import BriefPage from './pages/BriefPage'
import ChatPage from './pages/ChatPage'
import DiversifyTastePage from './pages/DiversifyTastePage'
import SimilarPage from './pages/SimilarPage'
import MoodBoardPage from './pages/MoodBoardPage'

const TABS = [
  { id: 'Brief',     label: 'Brief',     tip: 'Type a query and get song recommendations that have context' },
  { id: 'Chat',      label: 'Chat',      tip: 'Conversational recommendations' },
  { id: 'Taste',     label: 'Taste',     tip: "Recommendations based on a user's likes" },
  { id: 'Explain',   label: 'Similar',   tip: 'Upload a reference song and get similar songs' },
  { id: 'MoodBoard', label: 'MoodBoard', tip: 'Get relevant songs for an uploaded image' },
] as const
type TabId = typeof TABS[number]['id']

function initialTab(): TabId {
  const h = window.location.hash.replace('#', '')
  return (TABS.some(t => t.id === h) ? h : 'Brief') as TabId
}

export default function App() {
  const [tab, setTab] = useState<TabId>(initialTab)

  useEffect(() => {
    const onHash = () => {
      const h = window.location.hash.replace('#', '')
      if (TABS.some(t => t.id === h)) setTab(h as TabId)
    }
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])

  const go = (id: TabId) => { setTab(id); window.location.hash = id }

  return (
    <div style={{ display: 'flex', minHeight: '100vh', background: '#0d0d18', color: '#eaeaf2' }}>
      {/* Sidebar */}
      <aside style={{
        width: 224, flexShrink: 0, borderRight: '1px solid #1a1a2e',
        display: 'flex', flexDirection: 'column',
        padding: '22px 16px', position: 'sticky', top: 0, height: '100vh',
        background: 'rgba(13,13,24,0.6)',
      }}>
        {/* Brand */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={{
            width: 30, height: 30, borderRadius: 9,
            background: 'linear-gradient(135deg, #ff3d7f, #7c6cff)',
            display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 15, flexShrink: 0,
          }}>♪</div>
          <div>
            <div style={{
              fontSize: 17, fontWeight: 800, letterSpacing: '-0.03em',
              background: 'linear-gradient(135deg, #ff3d7f 20%, #7c6cff)',
              WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent', backgroundClip: 'text',
            }}>Diversify</div>
            <div style={{ fontSize: 9, color: '#555578', letterSpacing: '0.08em', textTransform: 'uppercase', marginTop: -2 }}>
              Contextual Discovery
            </div>
          </div>
        </div>

        {/* Nav */}
        <nav style={{ display: 'flex', flexDirection: 'column', gap: 2, marginTop: 30 }}>
          {TABS.map(t => {
            const active = tab === t.id
            return (
              <div key={t.id} title={t.tip} style={{ display: 'flex', alignItems: 'center', gap: 2 }}>
                <button onClick={() => go(t.id)} style={{
                  flex: 1, textAlign: 'left', padding: '9px 12px', borderRadius: 9, cursor: 'pointer',
                  fontSize: 14, fontWeight: active ? 600 : 400, border: 'none',
                  background: active ? '#1c1c2e' : 'transparent',
                  color: active ? '#eaeaf2' : '#7a7a99',
                }}>{t.label}</button>
                <a
                  href={`#${t.id}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  title={`Open ${t.label} in a new tab`}
                  style={{ color: '#3a3a55', textDecoration: 'none', fontSize: 13, padding: '4px 8px', flexShrink: 0 }}
                >↗</a>
              </div>
            )
          })}
        </nav>

        {/* Footer */}
        <div style={{ marginTop: 'auto', fontSize: 10, color: '#555578', whiteSpace: 'nowrap' }}>
          Made by Diversify, Hackatune 2026
        </div>
      </aside>

      {/* Main content */}
      <main style={{ flex: 1, minWidth: 0, padding: '32px 36px', maxWidth: 1320, margin: '0 auto' }}>
        {tab === 'Brief'     && <BriefPage />}
        {tab === 'Chat'      && <ChatPage />}
        {tab === 'Taste'     && <DiversifyTastePage />}
        {tab === 'Explain'   && <SimilarPage />}
        {tab === 'MoodBoard' && <MoodBoardPage />}
      </main>
    </div>
  )
}
