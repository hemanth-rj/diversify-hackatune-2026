import { useState, useRef } from 'react'
import { multimodalSearch, type TrackResult } from '../api'
import TrackCard from '../components/TrackCard'
import TrackModal from '../components/TrackModal'
import ScatterPlot from '../components/ScatterPlot'

function normalizedPct(track: TrackResult, maxScore: number) {
  return maxScore > 0 ? Math.round((track.finalScore / maxScore) * 100) : 50
}

export default function MoodBoardPage() {
  const [imageDataUrl, setImageDataUrl] = useState<string | null>(null)
  const [imageB64, setImageB64] = useState<string | null>(null)
  const [imageMime, setImageMime] = useState('image/jpeg')
  const [result, setResult] = useState<{ tracks: TrackResult[]; inferred: { filterSummary: string } } | null>(null)
  const [loading, setLoading] = useState(false)
  const [scatter, setScatter] = useState(false)
  const [dragging, setDragging] = useState(false)
  const [modalId, setModalId] = useState<string | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  function loadFile(file: File) {
    setImageMime(file.type || 'image/jpeg')
    const reader = new FileReader()
    reader.onload = ev => {
      const full = ev.target?.result as string
      setImageDataUrl(full)
      setImageB64(full.split(',')[1])
    }
    reader.readAsDataURL(file)
  }

  function onFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    if (file) loadFile(file)
  }

  function onDrop(e: React.DragEvent) {
    e.preventDefault()
    setDragging(false)
    const file = e.dataTransfer.files[0]
    if (file && file.type.startsWith('image/')) loadFile(file)
  }

  async function submit() {
    if (!imageB64) return
    setLoading(true)
    try {
      const r = await multimodalSearch({ image: imageB64, mimeType: imageMime, limit: 10 })
      setResult(r)
    } finally { setLoading(false) }
  }

  const maxScore = result ? Math.max(...result.tracks.map(t => t.finalScore), 0.001) : 1

  return (
    <div>
      {modalId && <TrackModal trackId={modalId} onClose={() => setModalId(null)} />}

      {/* Upload + preview layout */}
      <div style={{ display: 'flex', gap: 24, marginBottom: 28, alignItems: 'flex-start', flexWrap: 'wrap' }}>
        {/* Upload zone */}
        <div style={{ flex: '0 0 280px' }}>
          <input ref={fileRef} type="file" accept="image/*" style={{ display: 'none' }} onChange={onFile} />
          <div
            onClick={() => fileRef.current?.click()}
            onDragOver={e => { e.preventDefault(); setDragging(true) }}
            onDragLeave={() => setDragging(false)}
            onDrop={onDrop}
            style={{
              border: `2px dashed ${dragging ? '#7c6cff' : imageDataUrl ? '#2a2a42' : '#1e1e30'}`,
              borderRadius: 14, overflow: 'hidden',
              cursor: 'pointer', position: 'relative',
              background: dragging ? '#1a1230' : '#0f0f1a',
              transition: 'all 0.2s',
              minHeight: imageDataUrl ? 0 : 180,
            }}
          >
            {imageDataUrl ? (
              <img
                src={imageDataUrl}
                alt="mood"
                style={{ width: '100%', display: 'block', objectFit: 'cover', maxHeight: 280 }}
              />
            ) : (
              <div style={{
                display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
                gap: 10, padding: 32, textAlign: 'center',
              }}>
                <div style={{ fontSize: 32, opacity: 0.4 }}>🖼</div>
                <div style={{ fontSize: 13, color: '#555578', fontWeight: 500 }}>Drop an image or click to upload</div>
                <div style={{ fontSize: 11, color: '#3a3a55' }}>JPG, PNG, WebP — any mood board</div>
              </div>
            )}
            {imageDataUrl && (
              <div style={{
                position: 'absolute', inset: 0,
                background: 'linear-gradient(to top, rgba(13,13,24,0.7) 0%, transparent 50%)',
                display: 'flex', alignItems: 'flex-end', padding: 12,
              }}>
                <span style={{ fontSize: 11, color: '#aaaacc' }}>Click to change image</span>
              </div>
            )}
          </div>

          <button
            onClick={submit}
            disabled={loading || !imageB64}
            style={{
              width: '100%', marginTop: 12,
              background: imageB64 && !loading
                ? 'linear-gradient(135deg, #ff3d7f, #7c6cff)'
                : '#1a1a2e',
              border: 'none', color: imageB64 && !loading ? '#fff' : '#3a3a55',
              borderRadius: 10, padding: '12px', cursor: imageB64 && !loading ? 'pointer' : 'default',
              fontSize: 14, fontWeight: 700, letterSpacing: '-0.01em',
              transition: 'all 0.2s',
            }}
          >{loading ? 'Analyzing image…' : 'Find Music for This'}</button>
        </div>

        {/* Result header / empty prompt */}
        <div style={{ flex: 1, minWidth: 260 }}>
          {!imageDataUrl && !result && (
            <div style={{ padding: '40px 20px', textAlign: 'center', color: '#3a3a55' }}>
              <div style={{ fontSize: 40, marginBottom: 12 }}>♪</div>
              <div style={{ fontSize: 14, fontWeight: 600, color: '#555578' }}>Upload a mood board, photo, or artwork</div>
              <div style={{ fontSize: 12, marginTop: 6 }}>AI reads the visual vibe and finds matching music from 50k tracks</div>
            </div>
          )}

          {result && (
            <div>
              {/* AI interpretation */}
              <div style={{
                background: '#1a1230', border: '1px solid #2a2a42',
                borderRadius: 10, padding: '12px 16px', marginBottom: 20,
                fontSize: 13, color: '#c0c0e0', lineHeight: 1.5,
              }}>
                <span style={{ fontSize: 11, color: '#7c6cff', textTransform: 'uppercase', letterSpacing: '0.07em', marginRight: 8 }}>AI read:</span>
                {result.inferred.filterSummary}
              </div>

              {/* View toggle */}
              <div style={{ display: 'flex', gap: 6, marginBottom: 14 }}>
                {(['Grid', 'Scatter'] as const).map(v => (
                  <button key={v} onClick={() => setScatter(v === 'Scatter')} style={{
                    fontSize: 12, padding: '5px 14px', borderRadius: 8, cursor: 'pointer',
                    background: (scatter ? 'Scatter' : 'Grid') === v ? '#1c1c2e' : 'transparent',
                    border: `1px solid ${(scatter ? 'Scatter' : 'Grid') === v ? '#2a2a42' : 'transparent'}`,
                    color: (scatter ? 'Scatter' : 'Grid') === v ? '#eaeaf2' : '#555578',
                  }}>{v}</button>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Results grid — full width below */}
      {result && !scatter && (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))', gap: 14 }}>
          {result.tracks.map(t => (
            <TrackCard key={t.id} track={t} onOpenModal={setModalId} displayPct={normalizedPct(t, maxScore)} />
          ))}
        </div>
      )}
      {result && scatter && <ScatterPlot tracks={result.tracks} />}

      {loading && (
        <div style={{ color: '#555578', fontSize: 14, padding: '40px 0', textAlign: 'center' }}>
          Reading the image vibe…
        </div>
      )}
    </div>
  )
}
