import React, { useState, useEffect } from 'react';
import {
  GitPullRequest,
  AlertCircle,
  GitBranch,
  RefreshCw,
  Plus,
  Sparkles,
  Terminal,
  Clock,
  User,
  ExternalLink,
  ChevronDown,
  Lock,
  Globe,
  Search,
  Check,
  FolderGit2,
} from 'lucide-react';
import { api, GitHubUser, GitHubRepoItem } from '../api';

interface GitHubViewProps {
  onSendMessage?: (prompt: string) => void;
  onOpenTerminal?: (command: string) => void;
}

export const GitHubView: React.FC<GitHubViewProps> = ({ onSendMessage, onOpenTerminal }) => {
  const [activeTab, setActiveTab] = useState<'prs' | 'issues'>('prs');
  const [authStatus, setAuthStatus] = useState<{
    authenticated: boolean;
    user?: GitHubUser;
    workspace_repo?: { owner: string; repo: string };
  }>({ authenticated: false });

  const [userRepos, setUserRepos] = useState<GitHubRepoItem[]>([]);
  const [owner, setOwner] = useState<string>('');
  const [repo, setRepo] = useState<string>('');
  const [customInput, setCustomInput] = useState<string>('');
  const [isRepoPickerOpen, setIsRepoPickerOpen] = useState<boolean>(false);
  const [repoSearch, setRepoSearch] = useState<string>('');

  const [prs, setPrs] = useState<any[]>([]);
  const [issues, setIssues] = useState<any[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [isCreateOpen, setIsCreateOpen] = useState<boolean>(false);
  const [newTitle, setNewTitle] = useState<string>('');
  const [newBody, setNewBody] = useState<string>('');

  // Initial load: check auth status, user repositories, and workspace repo
  useEffect(() => {
    api.getGitHubStatus().then((status) => {
      setAuthStatus(status);
      if (status.authenticated) {
        api.getGitHubUserRepos().then((data) => {
          const list = data.repos || [];
          setUserRepos(list);

          // Priority: saved repo in localStorage -> workspace repo -> first user repo
          const saved = localStorage.getItem('odon_github_selected_repo');
          if (saved && saved.includes('/')) {
            const [o, r] = saved.split('/');
            setOwner(o);
            setRepo(r);
            setCustomInput(saved);
          } else if (status.workspace_repo) {
            setOwner(status.workspace_repo.owner);
            setRepo(status.workspace_repo.repo);
            setCustomInput(`${status.workspace_repo.owner}/${status.workspace_repo.repo}`);
          } else if (list.length > 0) {
            setOwner(list[0].owner);
            setRepo(list[0].name);
            setCustomInput(list[0].full_name);
          }
        }).catch(() => {});
      } else if (status.workspace_repo) {
        setOwner(status.workspace_repo.owner);
        setRepo(status.workspace_repo.repo);
        setCustomInput(`${status.workspace_repo.owner}/${status.workspace_repo.repo}`);
      }
    }).catch(() => {});
  }, []);

  const loadData = async (targetOwner = owner, targetRepo = repo) => {
    if (!targetOwner || !targetRepo) {
      setPrs([]);
      setIssues([]);
      return;
    }
    setIsLoading(true);
    try {
      const [prsData, issuesData] = await Promise.all([
        api.getGitHubPrs(targetOwner, targetRepo).catch(() => []),
        api.getGitHubIssues(targetOwner, targetRepo).catch(() => []),
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
    if (owner && repo) {
      loadData(owner, repo);
    }
  }, [owner, repo]);

  const handleSelectRepo = (selectedOwner: string, selectedRepo: string) => {
    setOwner(selectedOwner);
    setRepo(selectedRepo);
    const fullName = `${selectedOwner}/${selectedRepo}`;
    setCustomInput(fullName);
    localStorage.setItem('odon_github_selected_repo', fullName);
    setIsRepoPickerOpen(false);
  };

  const handleApplyCustomRepo = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = customInput.trim();
    if (!trimmed.includes('/')) return;
    const [o, r] = trimmed.split('/');
    if (o && r) {
      handleSelectRepo(o.trim(), r.trim());
    }
  };

  const handleReviewPr = (pr: any) => {
    const prompt = `Review Pull Request #${pr.number} on repository ${owner}/${repo}: "${pr.title}". Please check for potential bugs, regressions, logic errors, and security issues.`;
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
    if (!newTitle.trim() || !owner || !repo) return;
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

  const filteredRepos = userRepos.filter((r) =>
    r.full_name.toLowerCase().includes(repoSearch.toLowerCase())
  );

  return (
    <div className="flex flex-col h-full bg-white dark:bg-neutral-900 overflow-hidden font-sans select-text">
      {/* Top Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/40 gap-4 shrink-0">
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-xl bg-neutral-900 text-white flex items-center justify-center shadow-xs">
            <GitPullRequest className="w-4 h-4 text-white" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                GitHub Repository Center
              </h2>
              {authStatus.authenticated && authStatus.user && (
                <div className="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-neutral-100 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 text-[11px] font-medium text-neutral-700 dark:text-neutral-300">
                  <img
                    src={authStatus.user.avatar_url}
                    alt={authStatus.user.login}
                    className="w-3.5 h-3.5 rounded-full"
                  />
                  <span>@{authStatus.user.login}</span>
                </div>
              )}
            </div>
            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              Personal repository management, direct AI code review, and branch checkout
            </p>
          </div>
        </div>

        {/* Repository Picker & Controls */}
        <div className="flex items-center gap-2">
          {/* Active Repo Selector Dropdown */}
          <div className="relative">
            <button
              type="button"
              onClick={() => setIsRepoPickerOpen(!isRepoPickerOpen)}
              className="flex items-center gap-2 px-3 py-1.5 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs font-mono font-medium hover:border-neutral-400 dark:hover:border-neutral-600 transition-colors shadow-2xs cursor-pointer"
            >
              <FolderGit2 className="w-3.5 h-3.5 text-neutral-500" />
              <span className="truncate max-w-[160px]">
                {owner && repo ? `${owner}/${repo}` : 'Select Repository...'}
              </span>
              <ChevronDown className="w-3 h-3 text-neutral-400" />
            </button>

            {isRepoPickerOpen && (
              <div className="absolute right-0 top-full mt-1.5 w-80 rounded-2xl bg-white dark:bg-neutral-850 border border-neutral-200 dark:border-neutral-700 shadow-xl z-50 p-2 space-y-2">
                <div className="relative">
                  <Search className="w-3.5 h-3.5 text-neutral-400 absolute left-2.5 top-2.5" />
                  <input
                    type="text"
                    value={repoSearch}
                    onChange={(e) => setRepoSearch(e.target.value)}
                    placeholder="Search your repositories..."
                    className="w-full pl-8 pr-3 py-1.5 text-xs rounded-lg border border-neutral-200 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800 text-neutral-900 dark:text-white outline-none focus:border-blue-500"
                  />
                </div>

                {/* Custom Owner/Repo Input */}
                <form onSubmit={handleApplyCustomRepo} className="flex gap-1.5 pt-1 border-t border-neutral-100 dark:border-neutral-800">
                  <input
                    type="text"
                    value={customInput}
                    onChange={(e) => setCustomInput(e.target.value)}
                    placeholder="or type owner/repo"
                    className="flex-1 px-2.5 py-1 text-xs font-mono rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-white outline-none"
                  />
                  <button
                    type="submit"
                    className="px-2.5 py-1 text-xs font-medium rounded-lg bg-neutral-900 text-white hover:bg-neutral-800 transition-colors"
                  >
                    Set
                  </button>
                </form>

                {/* Repositories List */}
                <div className="max-h-56 overflow-y-auto space-y-1">
                  {filteredRepos.length > 0 ? (
                    filteredRepos.map((r) => {
                      const isSelected = r.owner === owner && r.name === repo;
                      return (
                        <button
                          key={r.full_name}
                          type="button"
                          onClick={() => handleSelectRepo(r.owner, r.name)}
                          className={`w-full text-left p-2 rounded-lg text-xs transition-colors flex items-center justify-between ${
                            isSelected
                              ? 'bg-blue-50 dark:bg-blue-950/40 text-blue-700 dark:text-blue-300 font-semibold'
                              : 'hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-800 dark:text-neutral-200'
                          }`}
                        >
                          <div className="truncate mr-2">
                            <div className="flex items-center gap-1.5">
                              {r.is_private ? (
                                <Lock className="w-3 h-3 text-amber-500 shrink-0" />
                              ) : (
                                <Globe className="w-3 h-3 text-neutral-400 shrink-0" />
                              )}
                              <span className="font-mono">{r.full_name}</span>
                            </div>
                            {r.description && (
                              <p className="text-[10px] text-neutral-400 truncate mt-0.5 font-sans font-normal">
                                {r.description}
                              </p>
                            )}
                          </div>
                          {isSelected && <Check className="w-3.5 h-3.5 text-blue-600 shrink-0" />}
                        </button>
                      );
                    })
                  ) : (
                    <div className="py-4 text-center text-xs text-neutral-400">
                      {authStatus.authenticated ? 'No matching repositories' : 'Sign in via gh to see your repositories'}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>

          <button
            type="button"
            onClick={() => loadData(owner, repo)}
            title="Refresh repository data"
            className="p-2 rounded-xl text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? 'animate-spin' : ''}`} />
          </button>

          {owner && repo && (
            <button
              type="button"
              onClick={() => setIsCreateOpen(true)}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold text-white bg-neutral-900 hover:bg-neutral-800 transition-all shadow-2xs cursor-pointer"
            >
              <Plus className="w-3.5 h-3.5" />
              <span>New Issue</span>
            </button>
          )}
        </div>
      </div>

      {/* Unauthenticated / Setup Guidance Banner */}
      {!authStatus.authenticated && (
        <div className="px-6 py-3 bg-neutral-100 dark:bg-neutral-800/60 border-b border-neutral-200 dark:border-neutral-700 flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-neutral-700 dark:text-neutral-300">
          <div className="flex items-center gap-2">
            <AlertCircle className="w-4 h-4 text-neutral-500 shrink-0" />
            <span>
              Log in to your personal GitHub account via <strong>gh auth login</strong> to browse your private repositories and submit issues.
            </span>
          </div>
          <button
            type="button"
            onClick={() => onOpenTerminal?.('gh auth login')}
            className="inline-flex items-center gap-1.5 px-3 py-1 rounded-lg bg-white dark:bg-neutral-700 border border-neutral-300 dark:border-neutral-600 text-xs font-semibold hover:bg-neutral-50 transition-colors shrink-0 cursor-pointer text-neutral-900 dark:text-white"
          >
            <Terminal className="w-3.5 h-3.5" />
            <span>gh auth login</span>
          </button>
        </div>
      )}

      {/* Tabs */}
      <div className="flex items-center gap-2 px-6 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 shrink-0">
        <button
          onClick={() => setActiveTab('prs')}
          className={`flex items-center gap-2 px-3 py-1.5 rounded-xl text-xs font-semibold transition-all cursor-pointer ${
            activeTab === 'prs'
              ? 'bg-neutral-900 text-white shadow-2xs'
              : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800'
          }`}
        >
          <GitPullRequest className="w-3.5 h-3.5" />
          <span>Pull Requests ({prs.length})</span>
        </button>
        <button
          onClick={() => setActiveTab('issues')}
          className={`flex items-center gap-2 px-3 py-1.5 rounded-xl text-xs font-semibold transition-all cursor-pointer ${
            activeTab === 'issues'
              ? 'bg-neutral-900 text-white shadow-2xs'
              : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800'
          }`}
        >
          <AlertCircle className="w-3.5 h-3.5" />
          <span>Issues ({issues.length})</span>
        </button>
      </div>

      {/* Content List */}
      <div className="flex-1 overflow-y-auto p-6 space-y-3">
        {!owner || !repo ? (
          <div className="flex flex-col items-center justify-center py-20 text-center text-neutral-400">
            <FolderGit2 className="w-12 h-12 mb-3 opacity-30 text-neutral-500" />
            <p className="text-sm font-semibold text-neutral-700 dark:text-neutral-300">
              No Repository Selected
            </p>
            <p className="text-xs text-neutral-400 mt-1 max-w-sm">
              Select one of your repositories from the dropdown above or enter <strong>owner/repo</strong> to inspect pull requests and issues.
            </p>
          </div>
        ) : activeTab === 'prs' ? (
          <>
            {prs.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 text-center text-neutral-400">
                <GitPullRequest className="w-12 h-12 mb-3 opacity-30" />
                <p className="text-sm font-semibold text-neutral-700 dark:text-neutral-300">
                  No Open Pull Requests
                </p>
                <p className="text-xs text-neutral-400 mt-1 max-w-sm">
                  Active pull requests for {owner}/{repo} will appear here with 1-click AI code reviews and checkout buttons.
                </p>
              </div>
            ) : (
              prs.map((pr) => (
                <div
                  key={pr.id || pr.number}
                  className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/40 hover:border-neutral-300 dark:hover:border-neutral-700 transition-all shadow-2xs space-y-3"
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
                          <GitBranch className="w-3 h-3 text-neutral-500" />
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
                        className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold text-neutral-900 dark:text-white bg-white dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 transition-colors cursor-pointer"
                      >
                        <Sparkles className="w-3.5 h-3.5 text-neutral-700 dark:text-neutral-300" />
                        <span>Review with Agent</span>
                      </button>
                      {pr.head?.ref && (
                        <button
                          type="button"
                          onClick={() => handleCheckoutBranch(pr.head.ref)}
                          title="Checkout branch in terminal"
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-medium text-neutral-700 dark:text-neutral-300 bg-white dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 transition-colors cursor-pointer"
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
        ) : (
          <>
            {issues.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 text-center text-neutral-400">
                <AlertCircle className="w-12 h-12 mb-3 opacity-30" />
                <p className="text-sm font-semibold text-neutral-700 dark:text-neutral-300">
                  No Open Issues
                </p>
                <p className="text-xs text-neutral-400 mt-1">
                  Everything is clear! Click "New Issue" above to file a task or bug report.
                </p>
              </div>
            ) : (
              issues.map((iss) => (
                <div
                  key={iss.id || iss.number}
                  className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/40 hover:border-neutral-300 dark:hover:border-neutral-700 transition-all shadow-2xs space-y-2"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100">
                          {iss.title}
                        </span>
                        <span className="text-xs font-mono text-neutral-400">#{iss.number}</span>
                      </div>
                      <div className="flex items-center gap-3 text-[11px] text-neutral-500 dark:text-neutral-400">
                        {iss.user?.login && (
                          <span className="flex items-center gap-1">
                            <User className="w-3 h-3" />
                            {iss.user.login}
                          </span>
                        )}
                        {iss.updated_at && (
                          <span className="flex items-center gap-1">
                            <Clock className="w-3 h-3" />
                            {new Date(iss.updated_at).toLocaleDateString()}
                          </span>
                        )}
                      </div>
                    </div>

                    <a
                      href={iss.html_url}
                      target="_blank"
                      rel="noreferrer"
                      className="p-1.5 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 transition-colors"
                      title="Open on GitHub"
                    >
                      <ExternalLink className="w-3.5 h-3.5" />
                    </a>
                  </div>
                </div>
              ))
            )}
          </>
        )}
      </div>

      {/* New Issue Modal */}
      {isCreateOpen && (
        <div className="fixed inset-0 bg-black/40 backdrop-blur-xs flex items-center justify-center z-50 p-4">
          <div className="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-2xl w-full max-w-lg p-6 shadow-xl space-y-4">
            <h3 className="text-sm font-bold text-neutral-900 dark:text-white">
              Create Issue on {owner}/{repo}
            </h3>
            <form onSubmit={handleCreateIssue} className="space-y-3">
              <div>
                <label className="text-xs font-medium text-neutral-700 dark:text-neutral-300 block mb-1">
                  Title
                </label>
                <input
                  type="text"
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                  placeholder="Issue title"
                  className="w-full px-3 py-2 text-xs rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-white outline-none focus:border-blue-500"
                  required
                />
              </div>
              <div>
                <label className="text-xs font-medium text-neutral-700 dark:text-neutral-300 block mb-1">
                  Description
                </label>
                <textarea
                  value={newBody}
                  onChange={(e) => setNewBody(e.target.value)}
                  placeholder="Describe the issue, steps to reproduce, or feature request..."
                  rows={4}
                  className="w-full px-3 py-2 text-xs rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-white outline-none focus:border-blue-500"
                />
              </div>
              <div className="flex justify-end gap-2 pt-2">
                <button
                  type="button"
                  onClick={() => setIsCreateOpen(false)}
                  className="px-4 py-2 text-xs font-medium rounded-lg text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-4 py-2 text-xs font-semibold rounded-lg bg-neutral-900 text-white hover:bg-neutral-800"
                >
                  Submit Issue
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
};
