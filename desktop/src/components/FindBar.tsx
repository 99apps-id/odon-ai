import { ChevronDown, ChevronUp, Search, X } from 'lucide-react'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'

import {
  clearMatches,
  collectMatchRanges,
  findBarKeyAction,
  formatMatchLabel,
  isOpenFindCombo,
  paintMatches,
  revealMatch,
  stepMatchIndex,
  supportsFindHighlight,
} from '../lib/findInPage'

interface FindBarProps {
  /** Scroll container whose content is searched. */
  containerRef: React.RefObject<HTMLElement | null>
  /** Changing this value reopens from scratch (one chat per scope). */
  scopeKey?: string
}

/**
 * Find-in-transcript (`mod+F`), following the platform convention shared by
 * Chrome, VS Code and Hermes' Electron bar: `mod+F` opens, `Enter` /
 * `Shift+Enter` step from the input, `mod+G` / `mod+Shift+G` step from
 * anywhere, `Escape` closes and clears.
 *
 * Matches are painted through the CSS Custom Highlight API so the transcript's
 * React-owned DOM is never rewritten.
 */
export function FindBar({ containerRef, scopeKey }: FindBarProps) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [count, setCount] = useState(0)
  const [current, setCurrent] = useState(-1)
  const inputRef = useRef<HTMLInputElement>(null)
  const rangesRef = useRef<Range[]>([])
  const highlightable = supportsFindHighlight()

  const close = useCallback(() => {
    setOpen(false)
    setQuery('')
    setCount(0)
    setCurrent(-1)
    rangesRef.current = []
    clearMatches()
  }, [])

  // A new conversation must not inherit the previous one's highlights, which
  // would also leave a stale "3/12" pointing at text that is gone.
  useEffect(() => {
    close()
  }, [scopeKey, close])

  useEffect(() => () => clearMatches(), [])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isOpenFindCombo(event)) {
        event.preventDefault()
        setOpen(true)

        return
      }

      if (!open) {
        return
      }

      const inInput = document.activeElement === inputRef.current
      const action = findBarKeyAction(event, { inInput })

      if (!action) {
        return
      }

      event.preventDefault()

      if (action === 'close') {
        close()

        return
      }

      const direction = action === 'next' ? 'next' : 'previous'
      setCurrent((previous) => {
        const next = stepMatchIndex(previous, rangesRef.current.length, direction)
        const range = rangesRef.current[next]

        if (range) {
          revealMatch(range)
        }

        return next
      })
    }

    window.addEventListener('keydown', onKeyDown, true)

    return () => window.removeEventListener('keydown', onKeyDown, true)
  }, [open, close])

  useEffect(() => {
    if (open) {
      inputRef.current?.focus()
      inputRef.current?.select()
    }
  }, [open])

  // Recomputed when the query changes, and painted in a layout effect so the
  // first match is highlighted and revealed in the same frame the counter
  // updates. `query` is the only input: the transcript keeps streaming, so
  // matches are re-collected on the next keystroke rather than per delta.
  useLayoutEffect(() => {
    const root = containerRef.current

    if (!open || !root || !query) {
      rangesRef.current = []
      setCount(0)
      setCurrent(-1)
      clearMatches()

      return
    }

    const ranges = collectMatchRanges(root, query)
    rangesRef.current = ranges
    setCount(ranges.length)

    if (ranges.length === 0) {
      setCurrent(-1)
      clearMatches()

      return
    }

    setCurrent(0)
    paintMatches(ranges, 0)
    revealMatch(ranges[0])
  }, [open, query, containerRef])

  useLayoutEffect(() => {
    if (!open || rangesRef.current.length === 0) {
      return
    }

    paintMatches(rangesRef.current, current)
  }, [open, current])

  if (!open) {
    return null
  }

  const label = formatMatchLabel(query, current < 0 ? 0 : current + 1, count)
  const emptyResult = Boolean(query) && count === 0

  return (
    <div
      role="search"
      aria-label="Find in transcript"
      className="absolute right-4 top-3 z-20 flex items-center gap-1.5 rounded-full border border-neutral-200 bg-white/95 px-2 py-1 shadow-[0_10px_30px_-12px_rgba(34,31,28,0.35)] backdrop-blur"
    >
      <Search className="h-3.5 w-3.5 shrink-0 text-neutral-400" strokeWidth={2} />
      <input
        ref={inputRef}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder="Find in chat"
        aria-label="Find in chat"
        className="w-40 min-w-0 bg-transparent text-[12.5px] text-neutral-900 placeholder:text-neutral-400 focus:outline-none"
      />
      {query && (
        <span
          className={`shrink-0 tabular-nums text-[11px] ${emptyResult ? 'text-clay-700' : 'text-neutral-400'}`}
          aria-live="polite"
        >
          {label}
        </span>
      )}
      <button
        type="button"
        onClick={() => {
          const next = stepMatchIndex(current, rangesRef.current.length, 'previous')
          const range = rangesRef.current[next]

          if (range) {
            revealMatch(range)
          }

          setCurrent(next)
        }}
        disabled={count === 0}
        aria-label="Previous match"
        className="rounded-full p-1 text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-900 disabled:opacity-40 disabled:hover:bg-transparent"
      >
        <ChevronUp className="h-3.5 w-3.5" strokeWidth={2.2} />
      </button>
      <button
        type="button"
        onClick={() => {
          const next = stepMatchIndex(current, rangesRef.current.length, 'next')
          const range = rangesRef.current[next]

          if (range) {
            revealMatch(range)
          }

          setCurrent(next)
        }}
        disabled={count === 0}
        aria-label="Next match"
        className="rounded-full p-1 text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-900 disabled:opacity-40 disabled:hover:bg-transparent"
      >
        <ChevronDown className="h-3.5 w-3.5" strokeWidth={2.2} />
      </button>
      <button
        type="button"
        onClick={close}
        aria-label="Close find"
        className="rounded-full p-1 text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-900"
      >
        <X className="h-3.5 w-3.5" strokeWidth={2.2} />
      </button>
      {!highlightable && (
        <span className="shrink-0 text-[10px] uppercase tracking-wide text-neutral-400">no tint</span>
      )}
    </div>
  )
}
