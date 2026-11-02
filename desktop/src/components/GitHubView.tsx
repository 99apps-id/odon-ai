import React, { useState, useEffect } from 'react';
import {
  GitPullRequest,
  AlertCircle,
  GitBranch,
  MessageSquare,
  CheckCircle2,
  XCircle,
  ExternalLink,
  Plus,
  RefreshCw,
  Sparkles,
  Terminal,
  Clock,
  User,
  Tag,
} from 'lucide-react';
import { api } from '../api';

interface GitHubViewProps {
  onSendMessage?: (prompt: string) => void;
  onOpenTerminal?: (command: string) => void;
}

export const GitHubView: React.FC<GitHubViewProps> = ({ onSendMessage, onOpenTerminal }) => {
  const [activeTab, setActiveTab] = useState<'prs' | 'issues'>('prs');
  const [owner, setOwner] = useState<string>('99apps-id');
  const [repo, setRepo] = useState<string>('odon-ai');
  const [prs, setPrs] = useState<any[]>([]);
  const [issues, setIssues] = useState<any[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [isCreateOpen, setIsCreateOpen] = useState<boolean>(false);
  const [newTitle, setNewTitle] = useState<string>('');
  const [newBody, setNewBody] = useState<string>('');

  const loadData = async () => {
    setIsLoading(true);
    try {
      const [prsData, issuesData] = await Promise.all([
        api.getGitHubPrs(owner, repo).catch(() => []),
        api.getGitHubIssues(owner, repo).catch(() => []),
      ]);
      setPrs(Array.isArray(prsData) ? prsData : []);
      setIssues(Array.isArray(issuesData) ? issuesData : []);
    } catch (err) {
      console.error('Failed to load GitHub data:', err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, [owner, repo]);

  const handleReviewPr = (pr: any) => {
    const prompt = `Review Pull Request #${pr.number}: "${pr.title}". Check code standards, potential regressions, performance, and test coverage.`;
    if (onSendMessage) {
      onSendMessage(prompt);
    }
  };

  const handleCheckoutBranch = (branchName: string) => {
    const cmd = `git fetch origin ${branchName} && git checkout ${branchName}`;
    if (onOpenTerminal) {
      onOpenTerminal(cmd);
    }
  };

  const handleCreateIssue = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newTitle.trim()) return;
    try {
      await api.createGitHubIssue(owner, repo, newTitle.trim(), newBody.trim());
      setNewTitle('');
      setNewBody('');
      setIsCreateOpen(false);
      loadData();
    } catch (err) {
      console.error('Failed to create issue:', err);
    }
  };

  return (
    <div className="flex flex-col h-full bg-white dark:bg-neutral-900 overflow-hidden font-sans">
      {/* Top Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/40 shrink-0">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-2xl bg-neutral-900 text-white dark:bg-white dark:text-neutral-900 flex items-center justify-center shadow-xs">
            <GitPullRequest className="w-5 h-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                GitHub Repository Center
              </h2>
              <span className="text-xs px-2.5 py-0.5 rounded-full bg-blue-100 dark:bg-blue-950/80 text-blue-700 dark:text-blue-300 font-mono font-semibold">
                {owner}/{repo}
              </span>
            </div>
            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              Direct PR review, branch checkout, issue tracking, and workflow automation
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={loadData}
            title="Refresh repository data"
            className="p-2 rounded-xl text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors"
          >
            <RefreshCw className={`w-4 h-4 ${isLoading ? 'animate-spin' : ''}`} />
          </button>
          <button
            type="button"
            onClick={() => setIsCreateOpen(true)}
            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 transition-all shadow-2xs"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>New Issue</span>
          </button>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-2 px-6 py-2.5 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 shrink-0">
        <button
          onClick={() => setActiveTab('prs')}
          className={`flex items-center gap-2 px-4 py-1.5 rounded-xl text-xs font-semibold transition-all ${
            activeTab === 'prs'
              ? 'bg-blue-600 text-white shadow-2xs'
              : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800'
          }`}
        >
          <GitPullRequest className="w-3.5 h-3.5" />
          <span>Pull Requests ({prs.length})</span>
        </button>
        <button
          onClick={() => setActiveTab('issues')}
          className={`flex items-center gap-2 px-4 py-1.5 rounded-xl text-xs font-semibold transition-all ${
            activeTab === 'issues'
              ? 'bg-blue-600 text-white shadow-2xs'
              : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800'
          }`}
        >
          <AlertCircle className="w-3.5 h-3.5" />
          <span>Issues ({issues.length})</span>
        </button>
      </div>

      {/* Content List */}
      <div className="flex-1 overflow-y-auto p-6 space-y-3">
        {activeTab === 'prs' && (
          <>
            {prs.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 text-center text-neutral-400">
                <GitPullRequest className="w-12 h-12 mb-3 opacity-40" />
                <p className="text-sm font-semibold text-neutral-700 dark:text-neutral-300">
                  No Open Pull Requests
                </p>
                <p className="text-xs text-neutral-400 mt-1 max-w-sm">
                  Active pull requests for {owner}/{repo} will appear here with automated 1-click AI code reviews and checkout buttons.
                </p>
              </div>
            ) : (
              prs.map((pr) => (
                <div
                  key={pr.id || pr.number}
                  className="p-4 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/30 hover:border-blue-400/60 dark:hover:border-blue-500/50 transition-all shadow-2xs space-y-3"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100 hover:text-blue-600 transition-colors">
                          {pr.title}
                        </span>
                        <span className="text-xs font-mono text-neutral-400">#{pr.number}</span>
                        <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-emerald-100 text-emerald-800 dark:bg-emerald-950/80 dark:text-emerald-300">
                          {pr.state}
                        </span>
                      </div>
                      <div className="flex items-center gap-3 text-[11px] text-neutral-500 dark:text-neutral-400">
                        <span className="flex items-center gap-1 font-mono">
                          <GitBranch className="w-3 h-3 text-blue-500" />
                          {pr.head?.ref || 'feature'} ➔ {pr.base?.ref || 'main'}
                        </span>
                        {pr.user?.login && (
                          <span className="flex items-center gap-1">
                            <User className="w-3 h-3" />
                            {pr.user.login}
                          </span>
                        )}
                        {pr.updated_at && (
                          <span className="flex items-center gap-1">
                            <Clock className="w-3 h-3" />
                            {new Date(pr.updated_at).toLocaleDateString()}
                          </span>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 shrink-0">
                      <button
                        type="button"
                        onClick={() => handleReviewPr(pr)}
                        className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-950/60 hover:bg-blue-100 dark:hover:bg-blue-900/40 transition-colors"
                      >
                        <Sparkles className="w-3.5 h-3.5" />
                        <span>Review with Agent</span>
                      </button>
                      {pr.head?.ref && (
                        <button
                          type="button"
                          onClick={() => handleCheckoutBranch(pr.head.ref)}
                          title="Checkout branch in terminal"
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium text-neutral-700 dark:text-neutral-300 bg-white dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 transition-colors"
                        >
                          <Terminal className="w-3.5 h-3.5" />
                          <span>Checkout</span>
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              ))
            )}
          </>
        )}

        {activeTab === 'issues' && (
          <>
            {issues.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 text-center text-neutral-400">
                <AlertCircle className="w-12 h-12 mb-3 opacity-40" />
                <p className="text-sm font-semibold text-neutral-700 dark:text-neutral-300">
                  No Open Issues
                </p>
                <p className="text-xs text-neutral-400 mt-1">
                  Everything is clear! Click "New Issue" to file a bug or feature request.
                </p>
              </div>
            ) : (
              issues.map((issue) => (
                <div
                  key={issue.id || issue.number}
                  className="p-4 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/30 flex items-start justify-between gap-4"
                >
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100">
                        {issue.title}
                      </span>
                      <span className="text-xs font-mono text-neutral-400">#{issue.number}</span>
                    </div>
                    <div className="flex items-center gap-3 text-[11px] text-neutral-500">
                      {issue.user?.login && <span>Opened by {issue.user.login}</span>}
                      {issue.comments > 0 && (
                        <span className="flex items-center gap-1">
                          <MessageSquare className="w-3 h-3" />
                          {issue.comments} comments
                        </span>
                      )}
                    </div>
                  </div>

                  <button
                    type="button"
                    onClick={() => onSendMessage && onSendMessage(`Analyze issue #${issue.number}: "${issue.title}". Propose an implementation plan.`)}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold text-neutral-700 dark:text-neutral-300 bg-white dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 transition-colors"
                  >
                    <Sparkles className="w-3.5 h-3.5 text-blue-500" />
                    <span>Plan Fix</span>
                  </button>
                </div>
              ))
            )}
          </>
        )}
      </div>

      {/* Create Issue Modal */}
      {isCreateOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs">
          <form
            onSubmit={handleCreateIssue}
            className="flex flex-col w-full max-w-lg bg-white dark:bg-neutral-900 rounded-3xl shadow-2xl border border-neutral-200 dark:border-neutral-800 p-6 space-y-4"
          >
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">Create GitHub Issue</h3>
              <button
                type="button"
                onClick={() => setIsCreateOpen(false)}
                className="p-1 rounded-lg text-neutral-400 hover:text-neutral-700"
              >
                <XCircle className="w-4 h-4" />
              </button>
            </div>

            <div>
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Title</label>
              <input
                type="text"
                value={newTitle}
                onChange={(e) => setNewTitle(e.target.value)}
                placeholder="Bug description or feature summary..."
                className="w-full px-3.5 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
                required
              />
            </div>

            <div>
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Body (Markdown)</label>
              <textarea
                value={newBody}
                onChange={(e) => setNewBody(e.target.value)}
                rows={4}
                placeholder="Steps to reproduce or expected behavior..."
                className="w-full px-3.5 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
              />
            </div>

            <div className="flex items-center justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setIsCreateOpen(false)}
                className="px-4 py-2 text-xs font-medium text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 rounded-xl"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="px-4 py-2 text-xs font-bold text-white bg-blue-600 hover:bg-blue-500 rounded-xl transition-colors shadow-2xs"
              >
                Submit Issue
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
};

