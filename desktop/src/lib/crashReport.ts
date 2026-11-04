/**
 * Crash forensics for the desktop shell.
 *
 * A render fault is caught by ErrorBoundary and never reaches the console the
 * operator can see: the window that launched from the Start menu has no
 * terminal attached, so without a record on disk the only report available is
 * "it crashed". This module keeps the last faults in localStorage, which
 * survives the reload the fallback offers, and renders them as text the
 * operator can paste into an issue.
 *
 * Recording is intentionally boring: bounded, synchronous, and incapable of
 * throwing. A forensics path that can itself fault turns one crash into two.
 */

export type CrashKind = 'render' | 'error' | 'rejection' | 'boot';

export interface CrashEntry {
  /** Epoch milliseconds. */
  at: number;
  kind: CrashKind;
  /** First line: what broke. */
  message: string;
  /** Stack or component stack, whichever the source provided. */
  detail: string;
  /** Active session at the time, when one was known. */
  sessionId?: string;
}

/** Entries kept on disk. Older ones are dropped; this is a diagnosis aid, not a log. */
export const MAX_CRASH_ENTRIES = 20;

const STORAGE_KEY = 'odon_crash_log';
const MAX_MESSAGE_LENGTH = 500;
const MAX_DETAIL_LENGTH = 4000;

function truncate(value: string, limit: number): string {
  return value.length <= limit ? value : `${value.slice(0, limit)}…`;
}

/**
 * Render a thrown value for the report, preferring a stack over a bare message.
 *
 * Rejections are not always `Error`s: a rejected `fetch` is a `TypeError`, a
 * rejected promise chain can be a plain object, and an aborted request can be a
 * string. All four shapes must produce something readable.
 */
export function describeCrashReason(reason: unknown): string {
  if (reason instanceof Error) {
    return reason.stack || reason.message || reason.name || 'Error';
  }

  if (typeof reason === 'string') {
    return reason || 'Unknown error';
  }

  if (reason && typeof reason === 'object') {
    const message = (reason as { message?: unknown }).message;

    if (typeof message === 'string' && message) {
      const stack = (reason as { stack?: unknown }).stack;

      return typeof stack === 'string' && stack ? stack : message;
    }
  }

  try {
    return JSON.stringify(reason) ?? String(reason);
  } catch {
    return String(reason);
  }
}

/**
 * Build a storable entry from a raw throwable.
 *
 * `detail` is capped hard: a React component stack for a deep tree is tens of
 * kilobytes, and localStorage rejects the whole write when it overflows, which
 * would silently lose every crash instead of the oversized one.
 */
export function makeCrashEntry(
  kind: CrashKind,
  reason: unknown,
  extra: { sessionId?: string; detail?: string } = {},
): CrashEntry {
  const described = describeCrashReason(reason)
  const firstLine = described.split('\n', 1)[0] ?? described

  return {
    at: Date.now(),
    kind,
    message: truncate(firstLine.trim() || 'Unknown error', MAX_MESSAGE_LENGTH),
    detail: truncate((extra.detail ?? described).trim(), MAX_DETAIL_LENGTH),
    sessionId: extra.sessionId,
  };
}

/**
 * Prepend an entry, dropping the oldest beyond `cap`.
 *
 * Prepending keeps the list newest-first for display and makes the cap drop the
 * least useful record first.
 */
export function appendCrash(
  entries: CrashEntry[],
  entry: CrashEntry,
  cap: number = MAX_CRASH_ENTRIES,
): CrashEntry[] {
  const limit = Number.isFinite(cap) && cap > 0 ? Math.floor(cap) : MAX_CRASH_ENTRIES

  return [entry, ...entries].slice(0, limit)
}

function isCrashEntry(value: unknown): value is CrashEntry {
  if (!value || typeof value !== 'object') {
    return false
  }

  const candidate = value as Partial<CrashEntry>
  const kindOk =
    candidate.kind === 'render' ||
    candidate.kind === 'error' ||
    candidate.kind === 'rejection' ||
    candidate.kind === 'boot'

  return (
    typeof candidate.at === 'number' &&
    typeof candidate.message === 'string' &&
    typeof candidate.detail === 'string' &&
    kindOk
  )
}

/** Read the stored log. Corrupt or foreign data yields an empty list. */
export function loadCrashes(): CrashEntry[] {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)

    if (!raw) {
      return []
    }

    const parsed: unknown = JSON.parse(raw)

    return Array.isArray(parsed) ? parsed.filter(isCrashEntry).slice(0, MAX_CRASH_ENTRIES) : []
  } catch {
    return []
  }
}

function saveCrashes(entries: CrashEntry[]): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(entries))
  } catch {
    // Quota or a disabled storage partition: dropping the record is better than
    // throwing out of a crash handler.
  }
}

/** Record one fault and return the updated log. Never throws. */
export function recordCrash(
  kind: CrashKind,
  reason: unknown,
  extra: { sessionId?: string; detail?: string } = {},
): CrashEntry[] {
  const entry = makeCrashEntry(kind, reason, extra)
  const next = appendCrash(loadCrashes(), entry)
  saveCrashes(next)

  if (typeof console !== 'undefined') {
    console.error(`[odon] ${kind} crash`, entry.message, entry.detail)
  }

  return next
}

/** Drop the stored log. */
export function clearCrashes(): void {
  try {
    window.localStorage.removeItem(STORAGE_KEY)
  } catch {
    // Nothing to do: the caller only wants the entries gone.
  }
}

function timestamp(ms: number): string {
  const date = new Date(ms)

  return Number.isNaN(date.getTime()) ? 'unknown time' : date.toISOString()
}

/**
 * Plain-text report for pasting into an issue.
 *
 * Deliberately excludes anything browser-identifying beyond the caller's own
 * note; a crash report should not leak more than the fault itself.
 */
export function formatCrashReport(entries: CrashEntry[]): string {
  if (entries.length === 0) {
    return 'No crashes recorded.'
  }

  return entries
    .map((entry, index) => {
      const header = `${index + 1}. [${entry.kind}] ${timestamp(entry.at)}${
        entry.sessionId ? ` · session ${entry.sessionId}` : ''
      }`

      return `${header}\n${entry.message}\n${entry.detail}`
    })
    .join('\n\n')
}

/**
 * Attach global handlers for faults React never sees: async rejections and
 * errors thrown outside the render path.
 *
 * Returns a disposer so a hot reload does not stack duplicate listeners.
 */
export function installCrashRecorder(): () => void {
  const onError = (event: ErrorEvent) => {
    recordCrash('error', event.error ?? event.message, {
      detail: event.error ? undefined : `${event.message}\n${event.filename}:${event.lineno}`,
    })
  }

  const onRejection = (event: PromiseRejectionEvent) => {
    recordCrash('rejection', event.reason)
  }

  window.addEventListener('error', onError)
  window.addEventListener('unhandledrejection', onRejection)

  return () => {
    window.removeEventListener('error', onError)
    window.removeEventListener('unhandledrejection', onRejection)
  }
}
