import React, { useState, useEffect } from 'react';
import {
  ShieldAlert,
  ShieldCheck,
  AlertTriangle,
  Terminal,
  FolderX,
  X,
  Check,
  CheckCheck,
  XCircle,
  History,
  Lock,
  Unlock,
  KeyRound,
  Trash2,
  ExternalLink,
  Code2,
  RefreshCw,
} from 'lucide-react';
import { api } from '../api';

interface ApprovalCenterModalProps {
  isOpen: boolean;
  onClose: () => void;
  pendingApproval?: {
    id: string;
    toolName: string;
    risk: string;
    arguments: Record<string, unknown>;
  } | null;
  onDecideApproval: (decision: 'approve_once' | 'approve_always' | 'deny') => void;
}

export const ApprovalCenterModal: React.FC<ApprovalCenterModalProps> = ({
  isOpen,
  onClose,
  pendingApproval,
  onDecideApproval,
}) => {
  const [activeTab, setActiveTab] = useState<'pending' | 'audit' | 'policies'>('pending');
  const [auditLogs, setAuditLogs] = useState<any[]>([]);
  const [policies, setPolicies] = useState<string[]>([]);
  const [executionMode, setExecutionMode] = useState<string>('Autonomous');
  const [isLoading, setIsLoading] = useState<boolean>(false);

  const loadData = async () => {
    setIsLoading(true);
    try {
      const [history, rules, settings] = await Promise.all([
        api.getApprovalHistory(),
        api.getApprovalPolicies(),
        api.getSettings().catch(() => ({ execution_mode: 'Autonomous' })),
      ]);
      setAuditLogs(history || []);
      setPolicies(rules || []);
      if (settings?.execution_mode) {
        setExecutionMode(settings.execution_mode);
      }
    } catch (err) {
      console.error('Failed to load approval center data:', err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (isOpen) {
      loadData();
      if (!pendingApproval) {
        setActiveTab('audit');
      } else {
        setActiveTab('pending');
      }
    }
  }, [isOpen, pendingApproval]);

  const handleRevokePolicy = async (key: string) => {
    try {
      await api.deleteApprovalPolicy(key);
      setPolicies((prev) => prev.filter((p) => p !== key));
    } catch (err) {
      console.error('Failed to revoke policy:', err);
    }
  };

  const handleSetMode = async (mode: string) => {
    try {
      await api.updateSettings({ mode });
      setExecutionMode(mode);
    } catch (err) {
      console.error('Failed to update execution mode:', err);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-200">
      <div className="flex flex-col w-full max-w-3xl max-h-[85vh] bg-white dark:bg-neutral-900 rounded-3xl shadow-2xl border border-neutral-200 dark:border-neutral-800 overflow-hidden">
        {/* Header bar */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950/60 shrink-0">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-2xl bg-amber-100 dark:bg-amber-950/70 text-amber-600 dark:text-amber-400 flex items-center justify-center shadow-xs">
              <ShieldAlert className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100 flex items-center gap-2">
                <span>Security Approval & Audit Center</span>
                {pendingApproval && (
                  <span className="text-[11px] font-semibold bg-rose-100 text-rose-700 dark:bg-rose-950/80 dark:text-rose-300 px-2.5 py-0.5 rounded-full animate-pulse border border-rose-300/60">
                    1 Action Requires Review
                  </span>
                )}
              </h2>
              <p className="text-xs text-neutral-500 dark:text-neutral-400">
                ODON Dual-Key Authorization, Real-time Sandboxing & Decision Audit Trail
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="p-2 rounded-xl text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 hover:bg-neutral-200/60 dark:hover:bg-neutral-800 transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Tab Navigation & Mode Status */}
        <div className="flex items-center justify-between px-6 py-2.5 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 shrink-0">
          <div className="flex items-center gap-1">
            <button
              onClick={() => setActiveTab('pending')}
              className={`flex items-center gap-2 px-3.5 py-1.5 rounded-xl text-xs font-semibold transition-all ${
                activeTab === 'pending'
                  ? 'bg-blue-600 text-white shadow-2xs'
                  : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800'
              }`}
            >
              <span>Pending Review</span>
              {pendingApproval && (
                <span className="w-2 h-2 rounded-full bg-rose-500 animate-ping" />
              )}
            </button>
            <button
              onClick={() => setActiveTab('audit')}
              className={`flex items-center gap-2 px-3.5 py-1.5 rounded-xl text-xs font-semibold transition-all ${
                activeTab === 'audit'
                  ? 'bg-blue-600 text-white shadow-2xs'
                  : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800'
              }`}
            >
              <History className="w-3.5 h-3.5" />
              <span>Audit History ({auditLogs.length})</span>
            </button>
            <button
              onClick={() => setActiveTab('policies')}
              className={`flex items-center gap-2 px-3.5 py-1.5 rounded-xl text-xs font-semibold transition-all ${
                activeTab === 'policies'
                  ? 'bg-blue-600 text-white shadow-2xs'
                  : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800'
              }`}
            >
              <KeyRound className="w-3.5 h-3.5" />
              <span>Allowed Policies ({policies.length})</span>
            </button>
          </div>

          <button
            onClick={loadData}
            title="Refresh logs"
            className="p-1.5 rounded-lg text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? 'animate-spin' : ''}`} />
          </button>
        </div>

        {/* Tab Body */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          {/* TAB 1: Pending Approval */}
          {activeTab === 'pending' && (
            <div>
              {pendingApproval ? (
                <div className="overflow-hidden rounded-2xl border border-amber-300 dark:border-amber-500/40 bg-gradient-to-b from-amber-50/90 to-white dark:from-neutral-950 dark:to-neutral-900 shadow-xl p-5 space-y-4">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2 font-mono">
                      <span className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                        {pendingApproval.toolName}
                      </span>
                      <span className="text-xs font-semibold px-2.5 py-0.5 rounded-full bg-rose-100 text-rose-700 dark:bg-rose-950 dark:text-rose-300 border border-rose-300/60">
                        {pendingApproval.risk}
                      </span>
                    </div>
                  </div>

                  <div className="rounded-xl bg-neutral-950 border border-neutral-800 p-4 font-mono text-xs text-neutral-200">
                    <div className="text-[10px] text-neutral-500 uppercase tracking-wider mb-2">Parameters Payload:</div>
                    <pre className="whitespace-pre-wrap break-all text-emerald-400 leading-relaxed">
                      {JSON.stringify(pendingApproval.arguments, null, 2)}
                    </pre>
                  </div>

                  <div className="flex flex-wrap items-center justify-end gap-2.5 pt-2">
                    <button
                      type="button"
                      onClick={() => onDecideApproval('deny')}
                      className="px-4 py-2 rounded-xl text-xs font-semibold text-rose-700 dark:text-rose-400 hover:bg-rose-100/60 dark:hover:bg-rose-950/50 transition-colors"
                    >
                      <XCircle className="w-3.5 h-3.5 inline mr-1" />
                      Deny Action
                    </button>
                    <button
                      type="button"
                      onClick={() => onDecideApproval('approve_always')}
                      className="px-4 py-2 rounded-xl text-xs font-semibold text-neutral-700 dark:text-neutral-300 bg-white dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-700 transition-all shadow-xs"
                    >
                      <CheckCheck className="w-3.5 h-3.5 inline mr-1 text-blue-500" />
                      Always Allow in Session
                    </button>
                    <button
                      type="button"
                      onClick={() => onDecideApproval('approve_once')}
                      className="px-5 py-2 rounded-xl text-xs font-bold text-white bg-blue-600 hover:bg-blue-500 transition-all shadow-xs"
                    >
                      <Check className="w-3.5 h-3.5 inline mr-1" />
                      Approve Once
                    </button>
                  </div>
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center py-12 text-center text-neutral-400">
                  <ShieldCheck className="w-12 h-12 text-emerald-500 mb-3 opacity-80" />
                  <p className="text-sm font-semibold text-neutral-700 dark:text-neutral-300">
                    No Pending Approvals
                  </p>
                  <p className="text-xs text-neutral-400 mt-1 max-w-sm">
                    All mutating actions have been processed or auto-approved according to current security mode ({executionMode}).
                  </p>
                </div>
              )}
            </div>
          )}

          {/* TAB 2: Audit History */}
          {activeTab === 'audit' && (
            <div className="space-y-3">
              {auditLogs.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-12 text-center text-neutral-400">
                  <History className="w-10 h-10 mb-2 opacity-60" />
                  <p className="text-xs">No approval actions logged yet in this session.</p>
                </div>
              ) : (
                auditLogs.map((log) => (
                  <div
                    key={log.id}
                    className="p-3.5 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/40 flex items-start justify-between gap-4"
                  >
                    <div>
                      <div className="flex items-center gap-2 font-mono">
                        <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100">{log.tool_name}</span>
                        <span className={`text-[10px] font-semibold px-2 py-0.5 rounded-full ${
                          log.decision.includes('approved')
                            ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-950/80 dark:text-emerald-300'
                            : 'bg-rose-100 text-rose-700 dark:bg-rose-950/80 dark:text-rose-300'
                        }`}>
                          {log.decision}
                        </span>
                        <span className="text-[10px] text-neutral-400">Tier {log.risk_tier}</span>
                      </div>
                      <p className="text-xs text-neutral-600 dark:text-neutral-400 mt-1">
                        {log.description || JSON.stringify(log.parameters)}
                      </p>
                    </div>
                    <span className="text-[10px] text-neutral-400 shrink-0 font-mono">
                      {new Date(log.resolved_at).toLocaleTimeString()}
                    </span>
                  </div>
                ))
              )}
            </div>
          )}

          {/* TAB 3: Allowed Policies / Whitelist */}
          {activeTab === 'policies' && (
            <div className="space-y-4">
              <div className="flex items-center justify-between p-4 rounded-2xl bg-neutral-100/60 dark:bg-neutral-800/40 border border-neutral-200 dark:border-neutral-800">
                <div>
                  <div className="text-xs font-bold text-neutral-900 dark:text-neutral-100">Security Mode</div>
                  <div className="text-[11px] text-neutral-500">Autonomous (Auto Tier 0 & 1), Supervised (Requires approvals), Locked (Read only)</div>
                </div>
                <div className="flex items-center gap-1.5">
                  {(['Autonomous', 'Supervised', 'Locked'] as const).map((m) => (
                    <button
                      key={m}
                      onClick={() => handleSetMode(m)}
                      className={`px-3 py-1.5 rounded-xl text-xs font-semibold transition-all ${
                        executionMode.toLowerCase() === m.toLowerCase()
                          ? 'bg-blue-600 text-white shadow-2xs'
                          : 'bg-white dark:bg-neutral-900 text-neutral-600 dark:text-neutral-400 border border-neutral-200 dark:border-neutral-700'
                      }`}
                    >
                      {m}
                    </button>
                  ))}
                </div>
              </div>

              <div className="space-y-2">
                <div className="text-xs font-bold text-neutral-800 dark:text-neutral-200">Whitelisted Actions</div>
                {policies.length === 0 ? (
                  <p className="text-xs text-neutral-400 italic">No persistent whitelist rules defined.</p>
                ) : (
                  policies.map((pol) => (
                    <div
                      key={pol}
                      className="flex items-center justify-between px-3.5 py-2.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900"
                    >
                      <code className="text-xs text-neutral-700 dark:text-neutral-300 font-mono">{pol}</code>
                      <button
                        onClick={() => handleRevokePolicy(pol)}
                        title="Revoke allowed rule"
                        className="p-1 rounded-lg text-rose-500 hover:bg-rose-50 dark:hover:bg-rose-950/40 transition-colors"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  ))
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

