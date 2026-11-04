import React from 'react';
import { AlertTriangle, RefreshCw } from 'lucide-react';
import type { BootFailure } from '../lib/bootFailure';

interface BootFailureNoticeProps {
  failure: BootFailure;
  /** True while the restart command is in flight. */
  retrying: boolean;
  onRetry: () => void;
  /** Set when the retry itself failed, so the operator sees why. */
  retryError?: string | null;
}

/**
 * Names why the daemon never answered, and what to do next.
 *
 * Three things are always said explicitly: the cause, the concrete steps, and
 * the raw line the shell reported. Hiding the raw line is what turns a five
 * second fix into a support thread.
 */
export function BootFailureNotice({
  failure,
  retrying,
  onRetry,
  retryError,
}: BootFailureNoticeProps) {
  return (
    <div className="odon-splash-failure mt-6 w-[26rem] max-w-[calc(100vw-3rem)] text-left">
      <div className="flex items-start gap-2">
        <AlertTriangle className="mt-[2px] h-4 w-4 shrink-0 text-clay-600" strokeWidth={2} />
        <div className="min-w-0">
          <p className="text-[13px] font-semibold text-clay-800">{failure.title}</p>
          <p className="mt-1 text-[12px] leading-relaxed text-neutral-600">{failure.detail}</p>
        </div>
      </div>

      {failure.steps.length > 0 && (
        <ul className="mt-3 space-y-1.5 border-t border-sand-200 pt-3">
          {failure.steps.map((step) => (
            <li key={step} className="flex gap-2 text-[11.5px] leading-relaxed text-neutral-600">
              <span className="mt-[6px] h-[3px] w-[3px] shrink-0 rounded-full bg-neutral-400" aria-hidden="true" />
              <span className="min-w-0 break-words">{step}</span>
            </li>
          ))}
        </ul>
      )}

      {failure.raw && (
        <pre className="mt-3 max-h-24 overflow-auto rounded-[6px] border border-sand-200 bg-white px-2.5 py-2 text-[10.5px] leading-relaxed whitespace-pre-wrap break-words text-neutral-500">
          {failure.raw}
        </pre>
      )}

      {retryError && (
        <p className="mt-3 text-[11.5px] leading-relaxed text-clay-700">Restart failed: {retryError}</p>
      )}

      <button
        type="button"
        onClick={onRetry}
        disabled={retrying}
        className="mt-4 inline-flex items-center gap-1.5 rounded-[6px] bg-blue-600 px-3 py-1.5 text-[11.5px] font-medium text-white transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
      >
        <RefreshCw className={`h-3 w-3 ${retrying ? 'animate-spin' : ''}`} strokeWidth={2.2} />
        {retrying ? 'Restarting…' : 'Restart daemon'}
      </button>
    </div>
  );
}
