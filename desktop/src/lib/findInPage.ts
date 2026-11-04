/**
 * Find-in-transcript primitives.
 *
 * The pure half (counter projection + key routing) decides what a keypress
 * means without touching the DOM, so the accelerator set can be reasoned about
 * in one place. The DOM half turns a query into Ranges and paints them through
 * the CSS Custom Highlight API — deliberately *not* by wrapping text nodes in
 * `<mark>`, because the transcript is React-owned: mutating its text nodes
 * fights reconciliation and loses the edit on the next streamed delta.
 */

/** Names registered in `CSS.highlights`; mirrored in index.css `::highlight()`. */
export const FIND_ALL_HIGHLIGHT = 'odon-find-all'
export const FIND_CURRENT_HIGHLIGHT = 'odon-find-current'

/** Matches painted at once. A pathological transcript must not stall a frame. */
export const MAX_FIND_MATCHES = 500

/** Text nodes longer than this are skipped; they are never a chat transcript. */
const MAX_SCANNED_TEXT_NODE = 20000

/**
 * Counter shown next to the input.
 *
 * Three distinct states, and they are not the same thing:
 * - empty query -> `''` (the counter hides rather than claiming "0/0" early)
 * - a query with no matches -> `'0/0'`
 * - a query with matches -> `'<ordinal>/<count>'`
 *
 * `ordinal` is clamped into `[0, count]` instead of trusted: it can legitimately
 * be 0 for the frame between a query change and the first match being selected.
 */
export function formatMatchLabel(query: string, ordinal: number, count: number): string {
  if (!query) {
    return ''
  }

  const safeCount = Number.isFinite(count) && count > 0 ? Math.floor(count) : 0

  if (safeCount === 0) {
    return '0/0'
  }

  const raw = Number.isFinite(ordinal) ? Math.floor(ordinal) : 0
  const capped = Math.min(Math.max(raw, 0), safeCount)

  return `${capped}/${safeCount}`
}

/** What a keypress means to an open find bar. `null` = not ours, let it through. */
export type FindBarKeyAction = 'close' | 'next' | 'previous' | null

/** The subset of a keyboard event the matcher needs; works for DOM and React. */
export interface FindBarKeyEvent {
  key: string
  shiftKey?: boolean
  metaKey?: boolean
  ctrlKey?: boolean
  altKey?: boolean
  /** IME composition state: a composition commit must not step matches. */
  isComposing?: boolean
  keyCode?: number
}

/**
 * Map a keypress to a find action while the bar is open.
 *
 * - `Escape` closes from anywhere (but not `Alt+Escape`, which the OS owns).
 * - `mod+G` / `mod+Shift+G` step next/previous from anywhere.
 * - Bare `Enter` / `Shift+Enter` step only while the input holds focus, so
 *   Enter inside the composer keeps meaning "send".
 * - `Alt` is disqualifying, so `Alt+G` falls through instead of being eaten.
 */
export function findBarKeyAction(
  event: FindBarKeyEvent,
  options: { inInput?: boolean } = {},
): FindBarKeyAction {
  if (event.altKey) {
    return null
  }

  const mod = Boolean(event.metaKey || event.ctrlKey)

  if (event.key === 'Escape') {
    return mod ? null : 'close'
  }

  if (mod && event.key.toLowerCase() === 'g') {
    return event.shiftKey ? 'previous' : 'next'
  }

  // 229 is the legacy IME keyCode some Windows builds still report.
  const composing = Boolean(event.isComposing) || event.keyCode === 229

  if (event.key === 'Enter' && !mod && options.inInput && !composing) {
    return event.shiftKey ? 'previous' : 'next'
  }

  return null
}

/** True when the combo opens the find bar (Cmd/Ctrl+F). */
export function isOpenFindCombo(event: {
  key: string
  metaKey?: boolean
  ctrlKey?: boolean
  altKey?: boolean
}): boolean {
  if (event.altKey) {
    return false
  }

  return Boolean(event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f'
}

/**
 * Step an index within `[0, count)`, wrapping at both ends.
 *
 * Returns -1 when there is nothing to step through, which callers treat as
 * "no current match" instead of pinning to index 0 and rendering a bogus `1/0`.
 */
export function stepMatchIndex(current: number, count: number, direction: 'next' | 'previous'): number {
  if (count <= 0) {
    return -1
  }

  const from = current < 0 || current >= count ? (direction === 'next' ? -1 : 0) : current
  const delta = direction === 'next' ? 1 : -1
  const next = (from + delta + count) % count

  return next
}

const SKIPPED_PARENT_TAGS = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEXTAREA'])

/**
 * Every occurrence of `query` inside `root`, as live Ranges.
 *
 * Case-insensitive plain substring matching, not a regex: a transcript is prose
 * and a user typing `(` must not throw. Matching never spans text-node
 * boundaries, which keeps each Range valid for the highlight registry.
 */
export function collectMatchRanges(root: HTMLElement, query: string): Range[] {
  const needle = query.toLowerCase()

  if (!needle) {
    return []
  }

  const doc = root.ownerDocument
  const ranges: Range[] = []
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const parent = node.parentElement

      if (!parent || SKIPPED_PARENT_TAGS.has(parent.tagName)) {
        return NodeFilter.FILTER_REJECT
      }

      const value = node.nodeValue

      if (!value || value.length > MAX_SCANNED_TEXT_NODE) {
        return NodeFilter.FILTER_REJECT
      }

      return NodeFilter.FILTER_ACCEPT
    },
  })

  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = node.nodeValue ?? ''
    const haystack = text.toLowerCase()
    let from = 0

    for (;;) {
      const at = haystack.indexOf(needle, from)

      if (at < 0) {
        break
      }

      const range = doc.createRange()
      range.setStart(node, at)
      range.setEnd(node, at + needle.length)
      ranges.push(range)

      if (ranges.length >= MAX_FIND_MATCHES) {
        return ranges
      }

      from = at + needle.length
    }
  }

  return ranges
}

interface HighlightRegistryLike {
  set: (name: string, highlight: unknown) => void
  delete: (name: string) => void
}

interface HighlightCtor {
  new (...ranges: Range[]): unknown
}

function highlightRegistry(): HighlightRegistryLike | null {
  const registry = (CSS as unknown as { highlights?: HighlightRegistryLike }).highlights

  return registry && typeof registry.set === 'function' ? registry : null
}

function highlightCtor(): HighlightCtor | null {
  const ctor = (globalThis as unknown as { Highlight?: HighlightCtor }).Highlight

  return typeof ctor === 'function' ? ctor : null
}

/** True when this engine can paint matches without touching the React tree. */
export function supportsFindHighlight(): boolean {
  return highlightRegistry() !== null && highlightCtor() !== null
}

/**
 * Paint every match, with the current one in a stronger colour.
 *
 * Returns false when the engine lacks the Custom Highlight API (older WebView);
 * callers then keep the counter and scrolling, which still navigates the
 * transcript even though nothing is tinted.
 */
export function paintMatches(ranges: Range[], currentIndex: number): boolean {
  const registry = highlightRegistry()
  const Highlight = highlightCtor()

  if (!registry || !Highlight) {
    return false
  }

  if (ranges.length === 0) {
    clearMatches()

    return true
  }

  registry.set(FIND_ALL_HIGHLIGHT, new Highlight(...ranges))

  const current = ranges[currentIndex]

  if (current) {
    registry.set(FIND_CURRENT_HIGHLIGHT, new Highlight(current))
  } else {
    registry.delete(FIND_CURRENT_HIGHLIGHT)
  }

  return true
}

/** Drop both highlight groups. Safe to call when nothing is painted. */
export function clearMatches(): void {
  const registry = highlightRegistry()

  if (!registry) {
    return
  }

  registry.delete(FIND_ALL_HIGHLIGHT)
  registry.delete(FIND_CURRENT_HIGHLIGHT)
}

/**
 * Bring a match into view inside the scroll container.
 *
 * `block: 'center'` is what makes stepping feel stable: with `nearest` a match
 * near the edge scrolls by one line per press and the counter runs ahead of the
 * viewport.
 */
export function revealMatch(range: Range): void {
  const target =
    range.startContainer.nodeType === Node.ELEMENT_NODE
      ? (range.startContainer as Element)
      : range.startContainer.parentElement

  target?.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'auto' })
}
