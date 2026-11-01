import React from 'react';
import { ShieldAlert, AlertTriangle, Terminal, FolderX, XCircle, Check, CheckCheck, Code2 } from 'lucide-react';

export interface ApprovalCardProps {
  id: string;
  toolName: string;
  risk: string;
  description?: string;
  parameters?: Record<string, unknown>;
  onDecide: (decision: 'approve_once' | 'approve_always' | 'deny') => void;
}

export const ApprovalCard: React.FC<ApprovalCardProps> = ({
  toolName,
  risk,
  description,
  parameters,
  onDecide,
}) => {
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

  const cmd = (parameters?.command as string) || (parameters?.cmd as string);
  const path = (parameters?.path as string) || (parameters?.target as string);

  return (
    <div className="my-4 max-w-2xl overflow-hidden rounded-2xl border border-amber-300 dark:border-amber-500/40 bg-gradient-to-b from-amber-50/90 to-white dark:from-neutral-900 dark:to-neutral-950 shadow-xl shadow-amber-500/5 transition-all">
      {/* Header bar */}
      <div className="flex items-center justify-between px-5 py-3.5 border-b border-amber-200/80 dark:border-neutral-800 bg-amber-100/40 dark:bg-amber-950/20">
        <div className="flex items-center gap-2.5">
          <div className={`w-8 h-8 rounded-xl flex items-center justify-center shrink-0 ${
            isDangerous || isOutsideWorkspace
              ? 'bg-rose-100 dark:bg-rose-950/70 text-rose-600 dark:text-rose-400'
              : 'bg-amber-100 dark:bg-amber-950/70 text-amber-600 dark:text-amber-400'
          }`}>
            {isOutsideWorkspace ? (
              <FolderX className="w-4 h-4" />
            ) : isDangerous ? (
              <ShieldAlert className="w-4 h-4" />
            ) : (
              <AlertTriangle className="w-4 h-4" />
            )}
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="text-xs font-bold tracking-tight text-neutral-900 dark:text-neutral-100 font-mono">
                {toolName}
              </span>
              <span className={`text-[10px] font-semibold px-2 py-0.5 rounded-full ${
                isDangerous || isOutsideWorkspace
                  ? 'bg-rose-100 text-rose-700 dark:bg-rose-950/80 dark:text-rose-300 border border-rose-300/60'
                  : 'bg-amber-100 text-amber-800 dark:bg-amber-950/80 dark:text-amber-300 border border-amber-300/60'
              }`}>
                {isOutsideWorkspace ? 'Outside Workspace' : risk}
              </span>
            </div>
            <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
              Action requires your explicit authorization before proceeding
            </p>
          </div>
        </div>
      </div>

      {/* Body / Context content */}
      <div className="p-5 space-y-3">
        {description && (
          <p className="text-xs text-neutral-700 dark:text-neutral-300 leading-relaxed font-sans">
            {description}
          </p>
        )}

        {cmd && (
          <div className="rounded-xl bg-neutral-950 border border-neutral-800 p-3.5">
            <div className="flex items-center gap-1.5 text-[11px] text-neutral-400 mb-1.5 font-mono">
              <Terminal className="w-3.5 h-3.5 text-emerald-400" />
              <span>Command to execute:</span>
            </div>
            <pre className="text-xs font-mono text-emerald-300 whitespace-pre-wrap break-all selection:bg-emerald-800">
              {cmd}
            </pre>
          </div>
        )}

        {path && !cmd && (
          <div className="rounded-xl bg-neutral-100 dark:bg-neutral-900/90 border border-neutral-200 dark:border-neutral-800 p-3">
            <div className="flex items-center gap-1.5 text-[11px] text-neutral-500 dark:text-neutral-400 font-mono">
              <Code2 className="w-3.5 h-3.5 text-blue-500" />
              <span>Target path:</span>
              <span className="font-semibold text-neutral-800 dark:text-neutral-200">{path}</span>
            </div>
          </div>
        )}

        {parameters && Object.keys(parameters).length > 0 && !cmd && (
          <div className="rounded-xl bg-neutral-950/90 border border-neutral-800/80 p-3">
            <div className="text-[10px] font-mono uppercase tracking-wider text-neutral-500 mb-1">
              Parameters
            </div>
            <pre className="max-h-32 overflow-auto text-[11px] font-mono text-neutral-300 leading-relaxed">
              {JSON.stringify(parameters, null, 2)}
            </pre>
          </div>
        )}

        {isDangerous && (
          <div className="flex items-center gap-2 text-[11px] text-rose-700 dark:text-rose-400 bg-rose-50 dark:bg-rose-950/30 border border-rose-200 dark:border-rose-900/40 rounded-xl px-3 py-2">
            <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
            <span>This tool can modify files or execute terminal processes on your machine.</span>
          </div>
        )}
      </div>

      {/* Action buttons (Approve Once, Always Allow, Deny) */}
      <div className="flex flex-wrap items-center justify-between gap-2 px-5 py-3.5 border-t border-neutral-200/80 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/40">
        <button
          type="button"
          onClick={() => onDecide('deny')}
          className="inline-flex items-center gap-1.5 px-3.5 py-2 text-xs font-medium text-rose-700 dark:text-rose-400 hover:bg-rose-100/60 dark:hover:bg-rose-950/50 rounded-xl transition-colors focus:outline-none focus:ring-2 focus:ring-rose-500/30"
        >
          <XCircle className="w-3.5 h-3.5" />
          <span>Deny</span>
        </button>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => onDecide('approve_always')}
            className="inline-flex items-center gap-1.5 px-3.5 py-2 text-xs font-medium text-neutral-700 dark:text-neutral-300 bg-white dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-700 rounded-xl shadow-sm transition-all focus:outline-none focus:ring-2 focus:ring-neutral-400"
          >
            <CheckCheck className="w-3.5 h-3.5 text-blue-500" />
            <span>Always Allow</span>
          </button>

          <button
            type="button"
            onClick={() => onDecide('approve_once')}
            className="inline-flex items-center gap-1.5 px-4 py-2 text-xs font-semibold text-white bg-neutral-900 hover:bg-neutral-800 dark:bg-blue-600 dark:hover:bg-blue-500 rounded-xl shadow-sm transition-all focus:outline-none focus:ring-2 focus:ring-neutral-500"
          >
            <Check className="w-3.5 h-3.5" />
            <span>Approve Once</span>
          </button>
        </div>
      </div>
    </div>
  );
};
