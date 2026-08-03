import { useCallback, useEffect, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'

export const MIN_INSPECTOR_WIDTH = 280
export const MAX_INSPECTOR_WIDTH = 640
const DEFAULT_INSPECTOR_WIDTH = 360
const STORAGE_KEY = 'cortex-loom.inspector-width'

interface DragState {
  startX: number
  startWidth: number
}

const clampWidth = (width: number) =>
  Math.min(MAX_INSPECTOR_WIDTH, Math.max(MIN_INSPECTOR_WIDTH, width))

function initialWidth(): number {
  const stored = Number.parseInt(window.localStorage.getItem(STORAGE_KEY) ?? '', 10)
  return Number.isFinite(stored) ? clampWidth(stored) : DEFAULT_INSPECTOR_WIDTH
}

export function useResizableInspector() {
  const [width, setWidth] = useState(initialWidth)
  const dragRef = useRef<DragState | null>(null)

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, String(width))
  }, [width])

  useEffect(() => {
    const move = (event: PointerEvent) => {
      const drag = dragRef.current
      if (!drag) return
      setWidth(clampWidth(drag.startWidth + drag.startX - event.clientX))
    }
    const end = () => {
      dragRef.current = null
      document.body.classList.remove('resizing-inspector')
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', end)
    window.addEventListener('pointercancel', end)
    return () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', end)
      window.removeEventListener('pointercancel', end)
      document.body.classList.remove('resizing-inspector')
    }
  }, [])

  const startResize = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    dragRef.current = { startX: event.clientX, startWidth: width }
    document.body.classList.add('resizing-inspector')
  }, [width])

  const resizeBy = useCallback((delta: number) => {
    setWidth(current => clampWidth(current + delta))
  }, [])

  const reset = useCallback(() => setWidth(DEFAULT_INSPECTOR_WIDTH), [])

  return { width, startResize, resizeBy, reset }
}
