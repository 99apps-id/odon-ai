import React, { useEffect, useRef, useState } from 'react';
import {
  AlertTriangle,
  Check,
  CheckCheck,
  Code2,
  CornerDownLeft,
  FolderX,
  ShieldAlert,
  ShieldCheck,
  Terminal,
  XCircle,
} from 'lucide-react';

export type ApprovalDecisionType =
  | 'approve_once'
  | 'approve_always'
  | 'approve_all'
  | 'deny';

export interface ApprovalCardProps {
  id: string;
  toolName: string;
  risk: string;
  description?: string;
  parameters?: Record<string, unknown>;
  onDecide: (
    decision: ApprovalDecisionType,
    reason?: string,
  ) => void | Promise<void>;
}

/* Every option shares one geometry so the stack lines up: same width, padding,
 * radius and centred row. Only colour and elevation differ per intent. */
const optionBase =
  'approval-option group flex min-w-0 items-center justify-between gap-2 rounded-lg px-3 py-2 text-left text-xs whitespace-normal transition-[background-color,border-color,box-shadow,transform] duration-150 active:translate-y-px cursor-pointer';

const optionRowClass = `${optionBase} border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-800/60 text-neutral-800 dark:text-neutral-200 shadow-[0_1px_2px_rgba(34,31,28,0.05),0_3px_10px_-4px_rgba(34,31,28,0.10)] hover:border-blue-300 dark:hover:border-blue-500 hover:bg-blue-50/60 dark:hover:bg-blue-950/30 hover:shadow-[0_2px_4px_rgba(34,31,28,0.06),0_6px_16px_-6px_rgba(63,91,112,0.30)]`;

const primaryRowClass = `${optionBase} border border-blue-600 bg-blue-600 font-medium text-white shadow-[0_1px_2px_rgba(34,31,28,0.12),0_4px_12px_-4px_rgba(63,91,112,0.45)] hover:bg-blue-700 hover:shadow-[0_2px_6px_rgba(34,31,28,0.14),0_8px_20px_-6px_rgba(63,91,112,0.55)]`;

const denyRowClass = `${optionBase} border border-clay-200 dark:border-clay-800/60 bg-clay-50 dark:bg-clay-950/30 font-medium text-clay-700 dark:text-clay-300 shadow-[0_1px_2px_rgba(34,31,28,0.05),0_3px_10px_-4px_rgba(93,61,48,0.16)] hover:border-clay-300 dark:hover:border-clay-700 hover:bg-clay-100 dark:hover:bg-clay-900/40 hover:shadow-[0_2px_4px_rgba(34,31,28,0.06),0_6px_16px_-6px_rgba(93,61,48,0.30)]`;

const kbdClass =
  'inline-flex h-5 min-w-5 items-center justify-center rounded border border-neutral-300 dark:border-neutral-600 bg-neutral-100 dark:bg-neutral-900 px-1 font-mono text-[10px] text-neutral-500 dark:text-neutral-400';

export const ApprovalCard: React.FC<ApprovalCardProps> = ({
  toolName,
  risk,
  description,
  parameters,
  onDecide,
}) => {
  const [showDenyFeedback, setShowDenyFeedback] = useState(false);
  const [feedback, setFeedback] = useState('');
  const feedbackRef = useRef<HTMLTextAreaElement>(null);

  const isDangerous =
    toolName.includes('shell') ||
    toolName.includes('bash') ||
    toolName.includes('terminal') ||
    toolName.includes('delete') ||
    risk.toLowerCase().includes('tier 2') ||
    risk.toLowerCase().includes('system');

  const isOutsideWorkspace =
    description?.toLowerCase().includes('outside') ||
    JSON.stringify(parameters || {}).includes('..') ||
    JSON.stringify(parameters || {}).includes('escape');

  const cmd =
    (parameters?.command as string) || (parameters?.cmd as string) || '';
  const path =
    (parameters?.path as string) || (parameters?.target as string) || '';
  const cwd = (parameters?.cwd as string) || '';
  const hasParameters =
    !!parameters && Object.keys(parameters).length > 0;

  const submitDeny = () => {
    const reason = feedback.trim();
    onDecide('deny', reason.length > 0 ? reason : 'Rejected by user');
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const tag = target?.tagName?.toLowerCase();
      if (
        tag === 'textarea' ||
        tag === 'input' ||
        tag === 'select' ||
        target?.isContentEditable
      ) {
        return;
      }
      if (event.key === '1') onDecide('approve_once');
      else if (event.key === '2') onDecide('approve_always');
      else if (event.key === '3') onDecide('approve_all');
      else if (event.key === '4') {
        setShowDenyFeedback(true);
        requestAnimationFrame(() => feedbackRef.current?.focus());
      } else if (event.key === 'Enter') {
        event.preventDefault();
        onDecide('approve_once');
      } else if (event.key === 'Escape') {
        onDecide('deny', 'Dismissed by user');
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onDecide]);

  return (
    <div className="my-4 max-w-2xl overflow-hidden rounded-xl border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 shadow-sm select-text">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-neutral-200 dark:border-neutral-700/80 bg-neutral-50 dark:bg-neutral-800/50">
        <div className="flex items-center gap-2.5">
          <div
            className={`w-7 h-7 rounded-lg flex items-center justify-center shrink-0 ${
              isDangerous || isOutsideWorkspace
                ? 'bg-clay-100 text-clay-600 dark:bg-clay-950/60 dark:text-clay-400'
                : 'bg-amber-100 text-amber-700 dark:bg-amber-950/60 dark:text-amber-400'
            }`}
          >
            {isOutsideWorkspace ? (
              <FolderX className="w-3.5 h-3.5" />
            ) : isDangerous ? (
              <ShieldAlert className="w-3.5 h-3.5" />
            ) : (
              <AlertTriangle className="w-3.5 h-3.5" />
            )}
          </div>
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-xs font-semibold text-neutral-900 dark:text-neutral-100">
                Approval required
              </span>
              <span className="text-xs font-mono text-neutral-500 dark:text-neutral-400">
                {toolName}
              </span>
              <span
                className={`text-[10px] font-medium px-2 py-0.5 rounded-md ${
                  isDangerous || isOutsideWorkspace
                    ? 'bg-clay-50 text-clay-700 border border-clay-200 dark:bg-clay-950/40 dark:text-clay-300 dark:border-clay-800/60'
                    : 'bg-amber-50 text-amber-800 border border-amber-200 dark:bg-amber-950/40 dark:text-amber-300 dark:border-amber-800/60'
                }`}
              >
                {isOutsideWorkspace ? 'Outside Workspace' : risk}
              </span>
            </div>
            {(cwd || path) && (
              <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5 font-mono break-all">
                {cwd || path}
              </p>
            )}
          </div>
        </div>
      </div>

      {/* Body */}
      <div className="p-4 space-y-3">
        {description && (
          <p className="text-xs text-neutral-700 dark:text-neutral-300 leading-relaxed">
            {description}
          </p>
        )}

        {cmd && (
          <div className="rounded-lg bg-neutral-950 p-3">
            <div className="flex items-center gap-1.5 text-[11px] font-medium text-neutral-400 mb-1.5 font-mono">
              <Terminal className="w-3.5 h-3.5 text-emerald-400" />
              <span>Command</span>
            </div>
            <pre className="text-xs font-mono text-neutral-100 whitespace-pre-wrap break-all">
              {cmd}
            </pre>
          </div>
        )}

        {path && !cmd && (
          <div className="rounded-lg bg-neutral-100 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 p-2.5">
            <div className="flex items-center gap-1.5 text-[11px] text-neutral-600 dark:text-neutral-400 font-mono">
              <Code2 className="w-3.5 h-3.5 text-blue-500" />
              <span>Target path:</span>
              <span className="font-semibold text-neutral-900 dark:text-neutral-100">
                {path}
              </span>
            </div>
          </div>
        )}

        {hasParameters && !cmd && (
          <details className="rounded-lg bg-neutral-100 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 p-3">
            <summary className="text-[10px] font-mono uppercase tracking-wider text-neutral-500 cursor-pointer select-none">
              Parameters
            </summary>
            <pre className="mt-1.5 max-h-36 overflow-auto text-[11px] font-mono text-neutral-800 dark:text-neutral-200 leading-relaxed">
              {JSON.stringify(parameters, null, 2)}
            </pre>
          </details>
        )}

        {isDangerous && (
          <div className="flex items-center gap-2 text-[11px] text-clay-800 dark:text-clay-300 bg-clay-50 dark:bg-clay-950/30 border border-clay-200 dark:border-clay-900/50 rounded-lg px-3 py-2">
            <AlertTriangle className="w-3.5 h-3.5 shrink-0 text-clay-600" />
            <span>
              This tool can modify the system, delete files, or run a
              subshell.
            </span>
          </div>
        )}
      </div>

      {/* Codex-style numbered options */}
      <div className="grid grid-cols-1 gap-2 border-t border-neutral-200 bg-neutral-50/80 px-4 py-3 dark:border-neutral-700 dark:bg-neutral-800/40 sm:grid-cols-2 xl:grid-cols-4">
        <button
          type="button"
          onClick={() => onDecide('approve_once')}
          className={primaryRowClass}
        >
          <span className="inline-flex items-center gap-2">
            <Check className="w-3.5 h-3.5" />
            Yes, run it
          </span>
          <span className="inline-flex items-center gap-1.5">
            <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded bg-white/15 px-1 font-mono text-[10px]">
              1
            </kbd>
            <kbd className="inline-flex h-5 items-center gap-0.5 rounded bg-white/15 px-1.5 font-mono text-[10px]">
              <CornerDownLeft className="w-3 h-3" />
            </kbd>
          </span>
        </button>

        <button
          type="button"
          onClick={() => onDecide('approve_always')}
          className={optionRowClass}
        >
          <span className="inline-flex items-center gap-2">
            <CheckCheck className="w-3.5 h-3.5 text-blue-500" />
            <span>
              Yes, and don&apos;t ask again for{' '}
              <span className="font-mono font-semibold">{toolName}</span>
            </span>
          </span>
          <kbd className={kbdClass}>2</kbd>
        </button>

        <button
          type="button"
          onClick={() => onDecide('approve_all')}
          className={optionRowClass}
        >
          <span className="inline-flex items-center gap-2">
            <ShieldCheck className="w-3.5 h-3.5 text-emerald-500" />
            <span>
              Allow all tools, don&apos;t ask again
              <span className="block text-[10px] font-normal text-neutral-500 dark:text-neutral-400">
                Everything runs without approval until you revoke it in
                Approval Center.
              </span>
            </span>
          </span>
          <kbd className={kbdClass}>3</kbd>
        </button>

        {!showDenyFeedback ? (
          <button
            type="button"
            onClick={() => {
              setShowDenyFeedback(true);
              requestAnimationFrame(() => feedbackRef.current?.focus());
            }}
            className={denyRowClass}
          >
            <span className="inline-flex items-center gap-2">
              <XCircle className="w-3.5 h-3.5" />
              No, tell the agent what to do instead
            </span>
            <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded border border-clay-200 dark:border-clay-800 bg-white/60 dark:bg-clay-950/40 px-1 font-mono text-[10px] text-clay-500">
              4
            </kbd>
          </button>
        ) : (
          <div className="col-span-full rounded-lg border border-clay-200 dark:border-clay-800/60 bg-clay-50/60 dark:bg-clay-950/20 p-3 space-y-2">
            <label
              htmlFor="approval-deny-feedback"
              className="block text-[11px] font-medium text-clay-800 dark:text-clay-200"
            >
              What should the agent do instead? (optional)
            </label>
            <textarea
              id="approval-deny-feedback"
              ref={feedbackRef}
              value={feedback}
              onChange={(event) => setFeedback(event.target.value)}
              rows={2}
              placeholder="e.g. Don't delete that file — run git status first instead."
              className="w-full rounded-lg border border-clay-200 dark:border-clay-800/70 bg-white dark:bg-neutral-900 px-2.5 py-2 text-xs text-neutral-800 dark:text-neutral-100 placeholder:text-neutral-400 focus:outline-none focus:ring-2 focus:ring-clay-300 dark:focus:ring-clay-700"
            />
            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={() => setShowDenyFeedback(false)}
                className="px-3 py-1.5 text-xs font-medium text-neutral-600 dark:text-neutral-300 hover:bg-neutral-200 dark:hover:bg-neutral-800 rounded-lg cursor-pointer"
              >
                Back
              </button>
              <button
                type="button"
                onClick={submitDeny}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-white bg-clay-600 hover:bg-clay-500 rounded-lg cursor-pointer"
              >
                <XCircle className="w-3.5 h-3.5" />
                Reject
              </button>
            </div>
          </div>
        )}

        <p className="col-span-full pt-0.5 text-[10px] text-neutral-400 dark:text-neutral-500">
          Press <kbd className={kbdClass}>1</kbd>–<kbd className={kbdClass}>4</kbd>{' '}
          to choose. “Don&apos;t ask again” still shows this card for other
          tools or other folders.
        </p>
      </div>
    </div>
  );
};

export default ApprovalCard;
