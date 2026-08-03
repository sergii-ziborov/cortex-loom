import type { KeyboardEvent, PointerEvent } from 'react'
import { MAX_INSPECTOR_WIDTH, MIN_INSPECTOR_WIDTH } from '../hooks/useResizableInspector'

interface InspectorResizeHandleProps {
  width: number
  onPointerDown: (event: PointerEvent<HTMLElement>) => void
  onResizeBy: (delta: number) => void
  onReset: () => void
}

export function InspectorResizeHandle(props: InspectorResizeHandleProps) {
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const step = event.shiftKey ? 40 : 10
    if (event.key === 'ArrowLeft') {
      event.preventDefault()
      props.onResizeBy(step)
    } else if (event.key === 'ArrowRight') {
      event.preventDefault()
      props.onResizeBy(-step)
    } else if (event.key === 'Home') {
      event.preventDefault()
      props.onResizeBy(MAX_INSPECTOR_WIDTH)
    } else if (event.key === 'End') {
      event.preventDefault()
      props.onResizeBy(-MAX_INSPECTOR_WIDTH)
    }
  }

  return (
    <div
      className="inspector-resizer"
      role="separator"
      tabIndex={0}
      aria-label="Resize graph inspector"
      aria-orientation="vertical"
      aria-valuemin={MIN_INSPECTOR_WIDTH}
      aria-valuemax={MAX_INSPECTOR_WIDTH}
      aria-valuenow={props.width}
      title="Drag to resize. Double-click to reset."
      onPointerDown={props.onPointerDown}
      onDoubleClick={props.onReset}
      onKeyDown={onKeyDown}
    >
      <span aria-hidden="true" />
    </div>
  )
}
