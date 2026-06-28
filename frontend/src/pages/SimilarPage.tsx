import { useState, useRef, useEffect } from 'react'
import { searchTracksByFile, type TrackResult, type PredictedTag } from '../api'
import TrackCard from '../components/TrackCard'
import TrackModal from '../components/TrackModal'

function normalizedPct(track: TrackResult, maxScore: number) {
  return maxScore > 0 ? Math.round((track.finalScore / maxScore) * 100) : 50
}

export default function SimilarPage() {
  const [results, setResults] = useState<TrackResult[]>([])
  const [loading, setLoading] = useState(false)
  const [dragging, setDragging] = useState(false)
  const [uploadedFile, setUploadedFile] = useState<File | null>(null)
  const [uploadedUrl, setUploadedUrl] = useState<string | null>(null)
  const [predictedTags, setPredictedTags] = useState<Record<string, PredictedTag[]> | null>(null)
  const [modalId, setModalId] = useState<string | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!uploadedFile) { setUploadedUrl(null); return }
    const url = URL.createObjectURL(uploadedFile)
    setUploadedUrl(url)
    return () => URL.revokeObjectURL(url)
  }, [uploadedFile])

  async function searchByFile(file: File) {
    setUploadedFile(file)
    setLoading(true)
    setResults([])
    setPredictedTags(null)
    try {
      const r = await searchTracksByFile(file, 20)
      setResults(r.tracks)
      setPredictedTags(r.predicted_tags ?? null)
    } finally { setLoading(false) }
  }

  const maxScore = results.length ? Math.max(...results.map(t => t.finalScore), 0.001) : 1

  return (
    <div style={{ display: 'grid', gridTemplateColumns: '320px 1fr', gap: 28 }}>
      {modalId && <TrackModal trackId={modalId} onClose={() => setModalId(null)} />}

      {/* Left panel — upload a reference track */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        <div style={{ fontSize: 13, fontWeight: 600, color: '#eaeaf2' }}>Upload a reference audio file</div>
        <input ref={fileRef} type="file" accept="audio/*,.mp3,.wav,.flac,.ogg,.m4a"
          style={{ display: 'none' }} onChange={e => { const f = e.target.files?.[0]; if (f) searchByFile(f) }} />
        <div
          onClick={() => fileRef.current?.click()}
          onDragOver={e => { e.preventDefault(); setDragging(true) }}
          onDragLeave={() => setDragging(false)}
          onDrop={e => { e.preventDefault(); setDragging(false); const f = e.dataTransfer.files[0]; if (f) searchByFile(f) }}
          style={{
            border: `2px dashed ${dragging ? '#19d3a2' : uploadedFile ? '#19d3a240' : '#1e1e30'}`,
            borderRadius: 14, padding: uploadedFile ? '16px 20px' : '40px 20px', textAlign: 'center',
            cursor: 'pointer',
            background: dragging ? '#0c1f1c' : '#0f0f1a',
            color: dragging ? '#19d3a2' : '#555578',
            fontSize: 13, transition: 'all 0.15s',
          }}
        >
          {loading && !uploadedFile
            ? 'Analyzing audio…'
            : uploadedFile
              ? (
                <div onClick={e => e.stopPropagation()}>
                  <div style={{ fontSize: 12, color: '#19d3a2', marginBottom: 8, textAlign: 'left', display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
                    <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', minWidth: 0 }}>
                      ✓ {uploadedFile.name}
                    </span>
                    <span style={{ color: '#3a3a55', fontSize: 10, flexShrink: 0 }}>click to change</span>
                  </div>
                  {uploadedUrl && (
                    <audio controls src={uploadedUrl} style={{ width: '100%', height: 28 }} />
                  )}
                </div>
              )
              : 'Drop an audio file or click to browse'}
        </div>
        <div style={{ fontSize: 11, color: '#3a3a55', lineHeight: 1.5 }}>
          MP3, WAV, FLAC, OGG · matched against 50k tracks by raw audio features
        </div>
      </div>

      {/* Right panel — results */}
      <div>
        {loading && !results.length && (
          <div style={{ color: '#555578', fontSize: 14, padding: '60px 0', textAlign: 'center' }}>
            Finding similar tracks…
          </div>
        )}

        {/* Predicted audio profile */}
        {predictedTags && Object.keys(predictedTags).length > 0 && (
          <div style={{
            background: 'linear-gradient(135deg, #0d1a20, #0f0f1a)',
            border: '1px solid #19d3a230', borderRadius: 12,
            padding: '14px 18px', marginBottom: 20,
          }}>
            <div style={{ fontSize: 11, color: '#19d3a2', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 12 }}>
              Predicted audio profile
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {(['MoodSimpleV2', 'MainGenreV2', 'InstrumentsV2', 'VocalsV2', 'CharacterV2'] as const).map(model => {
                const chips = predictedTags[model]
                if (!chips?.length) return null
                const label = model.replace('V2', '').replace('V1', '')
                  .replace('MoodSimple', 'Mood').replace('MainGenre', 'Genre')
                return (
                  <div key={model} style={{ display: 'flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap' }}>
                    <span style={{ fontSize: 10, color: '#555578', width: 72, flexShrink: 0, textTransform: 'uppercase', letterSpacing: '0.06em' }}>{label}</span>
                    {chips.map(c => (
                      <span key={c.tag} style={{
                        fontSize: 11, padding: '2px 8px', borderRadius: 20,
                        background: '#19d3a215', border: '1px solid #19d3a230',
                        color: c.prob >= 0.5 ? '#19d3a2' : '#6b8c80',
                        fontWeight: c.prob >= 0.5 ? 600 : 400,
                      }}>
                        {c.tag}
                        <span style={{ opacity: 0.55, marginLeft: 4, fontSize: 9 }}>{Math.round(c.prob * 100)}%</span>
                      </span>
                    ))}
                  </div>
                )
              })}
            </div>
          </div>
        )}

        {results.length > 0 && (
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))', gap: 14 }}>
            {results.map(t => (
              <TrackCard key={t.id} track={t} onOpenModal={setModalId} displayPct={normalizedPct(t, maxScore)} />
            ))}
          </div>
        )}

        {!results.length && !loading && (
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60%', flexDirection: 'column', gap: 12 }}>
            <div style={{ fontSize: 40, opacity: 0.15 }}>◎</div>
            <div style={{ fontSize: 14, color: '#3a3a55', textAlign: 'center' }}>
              Upload a reference audio file to find acoustically similar music
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
