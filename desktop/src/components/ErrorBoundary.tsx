import { Component, type ErrorInfo, type ReactNode } from 'react';
import {
  clearCrashes,
  formatCrashReport,
  loadCrashes,
  recordCrash,
  type CrashEntry,
} from '../lib/crashReport';

export interface ErrorBoundaryFallbackProps {
  error: Error;
  reset: () => void;
  label: string;
}

interface ErrorBoundaryProps {
  children: ReactNode;
  fallback?: (props: ErrorBoundaryFallbackProps) => ReactNode;
  label?: string;
  onError?: (error: Error, info: ErrorInfo) => void;
}

interface ErrorBoundaryState {
  error: Error | null;
  /** Crashes kept on disk, newest first; filled in when the fallback renders. */
  history: CrashEntry[];
}

/**
 * Classes of failure that settle once the fiber tree finishes a commit.
 * Hermes retries only these, never arbitrary render failures, so a real bug
 * still surfaces to the operator instead of looping invisibly.
 */
const TRANSIENT_PATTERNS: RegExp[] = [
  /(useClientLookup|tapClient(?:Lookup|Resource)).*out of bounds/u,
  /^Tried to unmount a fiber (?:that is already|that was not) unmounted/u,
  /Cannot read properties of undefined \(reading 'length'\)/u,
];

const MAX_AUTO_RECOVERIES = 3;
const AUTO_RECOVERY_WINDOW_MS = 5_000;

function isTransient(error: Error): boolean {
  return TRANSIENT_PATTERNS.some((pattern) => pattern.test(error.message));
}

/**
 * Catches render failures so one unexpected field in an API payload cannot
 * blank the whole window. Without this, a bad shape renders nothing at all and
 * the operator sees white with no message and no recovery path.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null, history: [] };
  private recoveries: number[] = [];

  static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    this.props.onError?.(error, info);
    if (typeof console !== 'undefined') {
      console.error('[odon] render error', this.props.label ?? 'root', error, info.componentStack);
    }
    // Persist before any recovery attempt: a transient error that recovers is
    // still worth knowing about, and the record must not depend on the retry
    // succeeding.
    const history = recordCrash('render', error, {
      detail: `${error.stack ?? error.message}\n\n${info.componentStack ?? ''}`,
    });
    this.setState({ history });
    if (isTransient(error)) {
      const now = Date.now();
      this.recoveries = this.recoveries.filter((at) => now - at < AUTO_RECOVERY_WINDOW_MS);
      if (this.recoveries.length < MAX_AUTO_RECOVERIES) {
        this.recoveries.push(now);
        queueMicrotask(() => this.setState({ error: null }));
      }
    }
  }

  private reset = () => {
    this.setState({ error: null });
  };

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    const label = this.props.label ?? 'application';
    const history = this.state.history;
    if (this.props.fallback) {
      return this.props.fallback({ error, reset: this.reset, label });
    }

    return (
      <div className="flex min-h-screen items-center justify-center bg-neutral-50 px-6 py-10">
        <div className="w-full max-w-xl rounded-2xl border border-neutral-200 bg-white p-8">
          <div className="text-[11px] font-semibold tracking-[0.18em] text-neutral-400">
            {label.toUpperCase()} CRASHED
          </div>
          <h1 className="mt-3 text-lg font-semibold text-neutral-900">
            Something failed while rendering
          </h1>
          <p className="mt-2 text-sm leading-relaxed text-neutral-600">
            The window caught the error before it blanked the screen. Your sessions and files on
            disk are untouched.
          </p>
          <pre className="mt-4 max-h-40 overflow-auto rounded-xl border border-neutral-200 bg-neutral-50 p-3 text-[11px] leading-relaxed text-neutral-700">
            {error.message}
          </pre>
          {history.length > 0 && (
            <div className="mt-4">
              <div className="flex items-center justify-between">
                <div className="text-[11px] font-semibold tracking-[0.18em] text-neutral-400">
                  RECENT CRASHES ({history.length})
                </div>
                <div className="flex items-center gap-3">
                  <button
                    type="button"
                    onClick={() => {
                      void navigator.clipboard?.writeText(formatCrashReport(history));
                    }}
                    className="text-[11px] font-medium text-blue-600 hover:text-blue-700"
                  >
                    Copy report
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      clearCrashes();
                      this.setState({ history: [] });
                    }}
                    className="text-[11px] font-medium text-neutral-500 hover:text-neutral-700"
                  >
                    Clear
                  </button>
                </div>
              </div>
              <ol className="mt-2 max-h-40 space-y-1 overflow-auto rounded-xl border border-neutral-200 bg-neutral-50 p-2">
                {history.map((entry) => (
                  <li key={`${entry.at}-${entry.message}`} className="text-[11px] text-neutral-600">
                    <span className="tabular-nums text-neutral-400">
                      {new Date(entry.at).toLocaleTimeString()}
                    </span>{' '}
                    <span className="rounded bg-neutral-200/70 px-1 text-[10px] uppercase tracking-wide text-neutral-600">
                      {entry.kind}
                    </span>{' '}
                    <span className="break-words">{entry.message}</span>
                  </li>
                ))}
              </ol>
            </div>
          )}
          <div className="mt-5 flex items-center gap-2">
            <button
              type="button"
              onClick={this.reset}
              className="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
            >
              Try again
            </button>
            <button
              type="button"
              onClick={() => window.location.reload()}
              className="rounded-lg border border-neutral-200 px-4 py-2 text-sm font-medium text-neutral-700 hover:bg-neutral-50"
            >
              Reload window
            </button>
          </div>
        </div>
      </div>
    );
  }
}
