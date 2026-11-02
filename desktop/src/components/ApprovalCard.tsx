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
    <div className="my-4 max-w-2xl overflow-hidden rounded-xl border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 shadow-sm transition-all select-text">
      {/* Header bar - clean subtle tint, no black */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-neutral-200 dark:border-neutral-700/80 bg-neutral-50 dark:bg-neutral-800/50">
        <div className="flex items-center gap-2.5">
          <div className={`w-7 h-7 rounded-lg flex items-center justify-center shrink-0 ${
            isDangerous || isOutsideWorkspace
              ? 'bg-rose-100 text-rose-600 dark:bg-rose-950/60 dark:text-rose-400'
              : 'bg-amber-100 text-amber-700 dark:bg-amber-950/60 dark:text-amber-400'
          }`}>
            {isOutsideWorkspace ? (
              <FolderX className="w-3.5 h-3.5" />
            ) : isDangerous ? (
              <ShieldAlert className="w-3.5 h-3.5" />
            ) : (
              <AlertTriangle className="w-3.5 h-3.5" />
            )}
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold text-neutral-900 dark:text-neutral-100 font-mono">
                {toolName}
              </span>
              <span className={`text-[10px] font-medium px-2 py-0.5 rounded-md ${
                isDangerous || isOutsideWorkspace
                  ? 'bg-rose-50 text-rose-700 border border-rose-200 dark:bg-rose-950/40 dark:text-rose-300 dark:border-rose-800/60'
                  : 'bg-amber-50 text-amber-800 border border-amber-200 dark:bg-amber-950/40 dark:text-amber-300 dark:border-amber-800/60'
              }`}>
                {isOutsideWorkspace ? 'Outside Workspace' : risk}
              </span>
            </div>
            <p className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
              Tindakan ini memerlukan izin eksekusi eksplisit
            </p>
          </div>
        </div>
      </div>

      {/* Body - clean neutral styling, light containers */}
      <div className="p-4 space-y-3">
        {description && (
          <p className="text-xs text-neutral-700 dark:text-neutral-300 leading-relaxed font-sans">
            {description}
          </p>
        )}

        {cmd && (
          <div className="rounded-lg bg-neutral-100 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 p-3">
            <div className="flex items-center gap-1.5 text-[11px] font-medium text-neutral-600 dark:text-neutral-400 mb-1.5 font-mono">
              <Terminal className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
              <span>Perintah terminal:</span>
            </div>
            <pre className="text-xs font-mono text-neutral-900 dark:text-neutral-100 whitespace-pre-wrap break-all select-text">
              {cmd}
            </pre>
          </div>
        )}

        {path && !cmd && (
          <div className="rounded-lg bg-neutral-100 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 p-2.5">
            <div className="flex items-center gap-1.5 text-[11px] text-neutral-600 dark:text-neutral-400 font-mono">
              <Code2 className="w-3.5 h-3.5 text-blue-500" />
              <span>Target path:</span>
              <span className="font-semibold text-neutral-900 dark:text-neutral-100 select-text">{path}</span>
            </div>
          </div>
        )}

        {parameters && Object.keys(parameters).length > 0 && !cmd && (
          <div className="rounded-lg bg-neutral-100 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 p-3">
            <div className="text-[10px] font-mono uppercase tracking-wider text-neutral-500 mb-1">
              Parameter
            </div>
            <pre className="max-h-36 overflow-auto text-[11px] font-mono text-neutral-800 dark:text-neutral-200 leading-relaxed select-text">
              {JSON.stringify(parameters, null, 2)}
            </pre>
          </div>
        )}

        {isDangerous && (
          <div className="flex items-center gap-2 text-[11px] text-rose-800 dark:text-rose-300 bg-rose-50 dark:bg-rose-950/30 border border-rose-200 dark:border-rose-900/50 rounded-lg px-3 py-2">
            <AlertTriangle className="w-3.5 h-3.5 shrink-0 text-rose-600" />
            <span>Peringatan: Tool ini dapat mengubah sistem, menghapus file, atau menjalankan subshell.</span>
          </div>
        )}
      </div>

      {/* Action buttons (Approve Once, Always Allow, Deny) - ChatGPT / Muse style */}
      <div className="flex flex-wrap items-center justify-between gap-2 px-4 py-3 border-t border-neutral-200 dark:border-neutral-700 bg-neutral-50/80 dark:bg-neutral-800/40">
        <button
          type="button"
          onClick={() => onDecide('deny')}
          className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-rose-700 dark:text-rose-400 bg-rose-50 dark:bg-rose-950/40 hover:bg-rose-100 dark:hover:bg-rose-900/50 border border-rose-200 dark:border-rose-800/60 rounded-lg transition-colors cursor-pointer"
        >
          <XCircle className="w-3.5 h-3.5" />
          <span>Deny</span>
        </button>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => onDecide('approve_always')}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-neutral-700 dark:text-neutral-300 bg-white dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-600 hover:bg-neutral-100 dark:hover:bg-neutral-700 rounded-lg shadow-sm transition-all cursor-pointer"
          >
            <CheckCheck className="w-3.5 h-3.5 text-blue-500" />
            <span>Always Allow</span>
          </button>

          <button
            type="button"
            onClick={() => onDecide('approve_once')}
            className="inline-flex items-center gap-1.5 px-4 py-1.5 text-xs font-medium text-white bg-blue-600 hover:bg-blue-500 dark:bg-blue-600 dark:hover:bg-blue-500 rounded-lg shadow-sm transition-all cursor-pointer"
          >
            <Check className="w-3.5 h-3.5" />
            <span>Approve Once</span>
          </button>
        </div>
      </div>
    </div>
  );
};
