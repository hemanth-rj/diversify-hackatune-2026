// Jamendo cover art, fetched lazily per track and cached in-memory. The browser can't call
// Jamendo's API directly (CORS), so we go through the product backend's /api/artwork proxy,
// which fetches it server-side. On any failure getArt resolves to null and the UI falls back
// to its gradient thumb — artwork is purely cosmetic.
const DIVERSIFY_BASE: string =
  (import.meta as any).env?.VITE_DIVERSIFY_API || 'http://localhost:8000/api'

const cache = new Map<string, string | null>()
const inflight = new Map<string, Promise<string | null>>()

export async function getArt(jamendoId: string | null | undefined): Promise<string | null> {
  if (!jamendoId) return null
  const id = String(jamendoId)
  if (cache.has(id)) return cache.get(id) ?? null
  const existing = inflight.get(id)
  if (existing) return existing

  const p = (async (): Promise<string | null> => {
    try {
      const r = await fetch(`${DIVERSIFY_BASE}/artwork/${encodeURIComponent(id)}`)
      const data = await r.json()
      const img: string | null = data?.image || null
      cache.set(id, img)
      return img
    } catch {
      cache.set(id, null)
      return null
    } finally {
      inflight.delete(id)
    }
  })()
  inflight.set(id, p)
  return p
}
