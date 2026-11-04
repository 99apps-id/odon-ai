import React, { useState, useEffect } from 'react';
import {
  Share2,
  Play,
  Plus,
  Clock,
  CheckCircle2,
  XCircle,
  AlertTriangle,
  RotateCw,
  Trash2,
  Cpu,
  Layers,
  ChevronDown,
  ChevronRight,
  Loader2,
  Calendar,
  ToggleLeft,
  ToggleRight,
} from 'lucide-react';
import { api } from '../api';
import type { ProviderItem } from '../types';

/**
 * The model the operator is currently chatting with. An automation should run
 * on that model unless the form explicitly picks another one, because a bare
 * `odon-auto` id used to fall through to whatever provider held the default
 * slot rather than the model actually in use.
 */
function activeModelId(): string {
  try {
    const provider = localStorage.getItem('odon_active_provider') || '';
    const model = localStorage.getItem('odon_selected_model') || '';
    if (provider && model) return `${provider}::${model}`;
    return model;
  } catch {
    return '';
  }
}

interface AutomationJob {
  id: string;
  name: string;
  description: string;
  trigger_type: string;
  trigger_value: string;
  prompt: string;
  model: string;
  enabled: boolean;
  last_run: string | null;
  last_status: string | null;
  created_at: string;
  retry_count?: number;
  retry_delay_secs?: number;
  depends_on?: string[];
}

interface AutomationLog {
  id: string;
  automation_id: string;
  automation_name: string;
  triggered_at: string;
  completed_at: string | null;
  status: string;
  output: string;
  attempts?: number;
  error?: string | null;
}

export const AutomationsView: React.FC = () => {
  const [jobs, setJobs] = useState<AutomationJob[]>([]);
  const [logs, setLogs] = useState<AutomationLog[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [runningJobId, setRunningJobId] = useState<string | null>(null);
  const [isCreateOpen, setIsCreateOpen] = useState<boolean>(false);
  const [expandedLogId, setExpandedLogId] = useState<string | null>(null);

  // New Job form state
  const [name, setName] = useState<string>('');
  const [description, setDescription] = useState<string>('');
  const [triggerType, setTriggerType] = useState<string>('cron');
  const [triggerValue, setTriggerValue] = useState<string>('0 8 * * *');
  const [prompt, setPrompt] = useState<string>('');
  const [model, setModel] = useState<string>(() => activeModelId() || 'odon-auto');
  const [providers, setProviders] = useState<ProviderItem[]>([]);
  const [retryCount, setRetryCount] = useState<number>(0);
  const [retryDelay, setRetryDelay] = useState<number>(30);
  const [dependsOn, setDependsOn] = useState<string>('');

  const loadData = async () => {
    setIsLoading(true);
    try {
      const [jobsData, logsData] = await Promise.all([
        api.getAutomations(),
        api.getAutomationLogs(),
      ]);
      setJobs(Array.isArray(jobsData) ? jobsData : []);
      setLogs(Array.isArray(logsData) ? logsData : []);
    } catch (err) {
      console.error('Failed to load automations:', err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadData();
    const interval = setInterval(loadData, 10000);
    return () => clearInterval(interval);
  }, []);

  // Catalogue for the model picker: only providers that can actually answer.
  useEffect(() => {
    let cancelled = false;
    api
      .getModels()
      .then((data) => {
        if (cancelled) return;
        const list = Array.isArray(data.providers) ? data.providers : [];
        setProviders(
          list.filter(
            (provider) =>
              (provider.configured || provider.connected) &&
              Array.isArray(provider.models) &&
              provider.models.length > 0,
          ),
        );
      })
      .catch(() => {
        if (!cancelled) setProviders([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleToggle = async (job: AutomationJob) => {
    const updated = { ...job, enabled: !job.enabled };
    try {
      await api.saveAutomation(updated);
      setJobs((prev) => prev.map((j) => (j.id === job.id ? updated : j)));
    } catch (err) {
      console.error('Failed to toggle automation:', err);
    }
  };

  const handleRunNow = async (id: string) => {
    setRunningJobId(id);
    try {
      await api.triggerAutomation(id);
      setTimeout(() => {
        setRunningJobId(null);
        loadData();
      }, 1500);
    } catch (err) {
      console.error('Failed to trigger automation:', err);
      setRunningJobId(null);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await api.deleteAutomation(id);
      setJobs((prev) => prev.filter((j) => j.id !== id));
    } catch (err) {
      console.error('Failed to delete automation:', err);
    }
  };

  const handleCreateSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !prompt.trim()) return;

    const newJob: AutomationJob = {
      id: `job-${Date.now()}`,
      name: name.trim(),
      description: description.trim(),
      trigger_type: triggerType,
      trigger_value: triggerValue,
      prompt: prompt.trim(),
      model,
      enabled: true,
      last_run: null,
      last_status: null,
      created_at: new Date().toISOString().split('T')[0],
      retry_count: Number(retryCount) || 0,
      retry_delay_secs: Number(retryDelay) || 30,
      depends_on: dependsOn
        .split(',')
        .map((value) => value.trim())
        .filter(Boolean),
    };

    try {
      await api.saveAutomation(newJob);
      setJobs((prev) => [newJob, ...prev]);
      setIsCreateOpen(false);
      setName('');
      setDescription('');
      setPrompt('');
    } catch (err) {
      console.error('Failed to save automation:', err);
    }
  };

  const applyPreset = (presetType: 'standup' | 'tests' | 'security' | 'hygiene') => {
    switch (presetType) {
      case 'standup':
        setName('Daily Git Standup & Digest');
        setDescription('Summarize git commits, pull requests, and pending tasks every morning');
        setTriggerType('cron');
        setTriggerValue('0 8 * * *');
        setPrompt('Summarize git commits made over the last 24 hours and highlight open PRs.');
        setModel(activeModelId() || 'odon-auto');
        break;
      case 'tests':
        setName('Repository Health & Cargo Tests');
        setDescription('Run test suite and check code quality periodically');
        setTriggerType('interval');
        setTriggerValue('Every 4 hours');
        setPrompt('Run workspace test suites and report any failing tests or lint warnings.');
        setModel(activeModelId() || 'odon-auto');
        break;
      case 'security':
        setName('Automated Secret & Security Audit');
        setDescription('Scan code for accidentally committed secrets, dangerous bash calls, and CVEs');
        setTriggerType('cron');
        setTriggerValue('0 0 * * *');
        setPrompt('Audit workspace for exposed API keys, unpinned dependencies, and security headers.');
        setModel(activeModelId() || 'odon-auto');
        break;
      case 'hygiene':
        setName('Stale Branch & PR Hygiene');
        setDescription('List stale branches without recent commits and suggest cleanup');
        setTriggerType('interval');
        setTriggerValue('Every 7 days');
        setPrompt('Check git branches that have not been modified for over 14 days and flag them.');
        setModel(activeModelId() || 'odon-auto');
        break;
    }
  };

  return (
    <div className="flex flex-col h-full bg-white dark:bg-neutral-900 overflow-hidden font-sans">
      {/* Header bar */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/40 shrink-0">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-2xl bg-blue-100 dark:bg-blue-950/80 text-blue-600 dark:text-blue-400 flex items-center justify-center shadow-xs">
            <Share2 className="w-5 h-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                Automations & Scheduled Agents
              </h2>
              <span className="text-xs px-2.5 py-0.5 rounded-full bg-blue-100 text-blue-700 dark:bg-blue-950 dark:text-blue-300 font-semibold font-mono">
                {jobs.filter((j) => j.enabled).length} Active Jobs
              </span>
            </div>
            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              Cron schedules, automated audits, code reviews, and background trigger pipelines
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={loadData}
            title="Refresh automations"
            className="p-2 rounded-xl text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 hover:bg-neutral-200 dark:hover:bg-neutral-800 transition-colors"
          >
            <RotateCw className={`w-4 h-4 ${isLoading ? 'animate-spin' : ''}`} />
          </button>
          <button
            type="button"
            onClick={() => setIsCreateOpen(true)}
            className="inline-flex items-center gap-1.5 px-4 py-2 rounded-xl text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 transition-all shadow-2xs"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>New Automation</span>
          </button>
        </div>
      </div>

      <div className="mx-6 mt-4 p-3 rounded-xl border border-amber-200 bg-amber-50 text-[11px] text-amber-800 dark:border-amber-900/60 dark:bg-amber-950/40 dark:text-amber-300">
        Automatic cron/interval execution is not enabled in this build. Use <span className="font-semibold">Run Now</span> to execute a workflow manually.
      </div>

      {/* Main content body */}
      <div className="flex-1 overflow-y-auto p-6 space-y-6">
        {/* Workflows Grid */}
        <div className="space-y-3">
          <div className="text-xs font-bold text-neutral-800 dark:text-neutral-200 uppercase tracking-wider">
            Workflows ({jobs.length})
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {jobs.map((job) => (
              <div
                key={job.id}
                className={`p-5 rounded-3xl border transition-all shadow-2xs space-y-3.5 ${
                  job.enabled
                    ? 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 hover:shadow-md'
                    : 'border-neutral-200/60 dark:border-neutral-800/60 bg-neutral-50/50 dark:bg-neutral-950/40 opacity-70'
                }`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-bold text-neutral-900 dark:text-neutral-100">
                        {job.name}
                      </span>
                      {job.last_status && (
                        <span className={`text-[10px] font-semibold px-2 py-0.5 rounded-full ${
                          job.last_status === 'success'
                            ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-950/80 dark:text-emerald-300'
                            : 'bg-clay-100 text-clay-800 dark:bg-clay-950/80 dark:text-clay-300'
                        }`}>
                          {job.last_status}
                        </span>
                      )}
                    </div>
                    <p className="text-xs text-neutral-500 dark:text-neutral-400 line-clamp-2">
                      {job.description || job.prompt}
                    </p>
                  </div>

                  <button
                    type="button"
                    onClick={() => handleToggle(job)}
                    className="shrink-0 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200"
                  >
                    {job.enabled ? (
                      <ToggleRight className="w-6 h-6 text-blue-600 dark:text-blue-400" />
                    ) : (
                      <ToggleLeft className="w-6 h-6 text-neutral-300 dark:text-neutral-600" />
                    )}
                  </button>
                </div>

                <div className="flex flex-wrap items-center gap-2 text-[11px] text-neutral-500 font-mono">
                  <span className="flex items-center gap-1 bg-neutral-100 dark:bg-neutral-800 px-2.5 py-1 rounded-lg">
                    <Clock className="w-3 h-3 text-blue-500" />
                    {job.trigger_value}
                  </span>
                  <span className="flex items-center gap-1 bg-neutral-100 dark:bg-neutral-800 px-2.5 py-1 rounded-lg">
                    <Cpu className="w-3 h-3 text-neutral-500" />
                    {job.model}
                  </span>
                </div>

                <div className="flex items-center justify-between pt-2 border-t border-neutral-100 dark:border-neutral-800/80">
                  <span className="text-[10px] text-neutral-400">
                    {job.last_run ? `Last run: ${job.last_run}` : 'Pending first run'}
                  </span>

                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => handleDelete(job.id)}
                      title="Delete automation"
                      className="p-1.5 rounded-lg text-neutral-400 hover:text-clay-500 hover:bg-clay-50 dark:hover:bg-clay-950/40 transition-colors"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                    <button
                      type="button"
                      onClick={() => handleRunNow(job.id)}
                      disabled={runningJobId === job.id}
                      className="inline-flex items-center gap-1 px-3 py-1.5 rounded-xl text-xs font-semibold text-neutral-800 dark:text-neutral-200 bg-neutral-100 hover:bg-neutral-200 dark:bg-neutral-800 dark:hover:bg-neutral-700 transition-colors"
                    >
                      {runningJobId === job.id ? (
                        <Loader2 className="w-3 h-3 animate-spin text-blue-500" />
                      ) : (
                        <Play className="w-3 h-3 text-blue-500" />
                      )}
                      <span>Run Now</span>
                    </button>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Execution Logs Section */}
        <div className="space-y-3 pt-4 border-t border-neutral-200 dark:border-neutral-800">
          <div className="text-xs font-bold text-neutral-800 dark:text-neutral-200 uppercase tracking-wider">
            Recent Execution Logs ({logs.length})
          </div>

          {logs.length === 0 ? (
            <p className="text-xs text-neutral-400 italic">No execution runs recorded yet.</p>
          ) : (
            <div className="space-y-2">
              {logs.map((log) => {
                const isExpanded = expandedLogId === log.id;
                return (
                  <div
                    key={log.id}
                    className="p-3.5 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/40 space-y-2"
                  >
                    <div
                      onClick={() => setExpandedLogId(isExpanded ? null : log.id)}
                      className="flex items-center justify-between cursor-pointer"
                    >
                      <div className="flex items-center gap-2">
                        {isExpanded ? <ChevronDown className="w-3.5 h-3.5 text-neutral-400" /> : <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />}
                        <span className="text-xs font-semibold text-neutral-900 dark:text-neutral-100">
                          {log.automation_name}
                        </span>
                        <span className={`text-[10px] font-semibold px-2 py-0.5 rounded-full ${
                          log.status === 'success'
                            ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-950/80 dark:text-emerald-300'
                            : 'bg-clay-100 text-clay-800 dark:bg-clay-950/80 dark:text-clay-300'
                        }`}>
                          {log.status}
                        </span>
                      </div>
                      <span className="text-[10px] text-neutral-400 font-mono">
                        {new Date(log.triggered_at).toLocaleTimeString()}
                      </span>
                    </div>

                    {isExpanded && (
                      <div className="pt-2 pl-5 font-mono text-xs text-neutral-700 dark:text-neutral-300 border-t border-neutral-200 dark:border-neutral-800 space-y-1">
                        {typeof log.attempts === 'number' && (
                          <div className="text-[10px] text-neutral-400">
                            Attempts: {log.attempts}
                          </div>
                        )}
                        {log.error && (
                          <div className="text-[11px] text-clay-600 dark:text-clay-400 whitespace-pre-wrap">
                            {log.error}
                          </div>
                        )}
                        <pre className="whitespace-pre-wrap break-all leading-relaxed">
                          {log.output}
                        </pre>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* New Automation Modal */}
      {isCreateOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-neutral-200/70 backdrop-blur-xs">
          <form
            onSubmit={handleCreateSubmit}
            className="flex flex-col w-full max-w-xl bg-white dark:bg-neutral-900 rounded-3xl shadow-2xl border border-neutral-200 dark:border-neutral-800 p-6 space-y-4 max-h-[90vh] overflow-y-auto"
          >
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">Create Scheduled Automation</h3>
              <button
                type="button"
                onClick={() => setIsCreateOpen(false)}
                className="p-1 rounded-lg text-neutral-400 hover:text-neutral-700"
              >
                <XCircle className="w-4 h-4" />
              </button>
            </div>

            {/* Presets */}
            <div>
              <label className="text-xs font-semibold text-neutral-500 block mb-1.5 uppercase tracking-wider">
                Quick Preset Templates:
              </label>
              <div className="grid grid-cols-2 gap-2">
                <button
                  type="button"
                  onClick={() => applyPreset('standup')}
                  className="px-3 py-2 rounded-xl text-left text-xs bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 transition-colors"
                >
                  <div className="font-bold text-neutral-900 dark:text-neutral-100">Daily Git Standup</div>
                  <div className="text-[10px] text-neutral-500">Cron 08:00 AM</div>
                </button>
                <button
                  type="button"
                  onClick={() => applyPreset('tests')}
                  className="px-3 py-2 rounded-xl text-left text-xs bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 transition-colors"
                >
                  <div className="font-bold text-neutral-900 dark:text-neutral-100">Repo Health Check</div>
                  <div className="text-[10px] text-neutral-500">Every 4 hours</div>
                </button>
                <button
                  type="button"
                  onClick={() => applyPreset('security')}
                  className="px-3 py-2 rounded-xl text-left text-xs bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 transition-colors"
                >
                  <div className="font-bold text-neutral-900 dark:text-neutral-100">Security Audit</div>
                  <div className="text-[10px] text-neutral-500">Midnight Cron</div>
                </button>
                <button
                  type="button"
                  onClick={() => applyPreset('hygiene')}
                  className="px-3 py-2 rounded-xl text-left text-xs bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 transition-colors"
                >
                  <div className="font-bold text-neutral-900 dark:text-neutral-100">Branch Hygiene</div>
                  <div className="text-[10px] text-neutral-500">Weekly Cleanup</div>
                </button>
              </div>
            </div>

            <div>
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Workflow Title</label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Daily Code Review..."
                className="w-full px-3.5 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
                required
              />
            </div>

            <div>
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Trigger Schedule</label>
              <input
                type="text"
                value={triggerValue}
                onChange={(e) => setTriggerValue(e.target.value)}
                placeholder="e.g. 0 8 * * * or Every 2 hours"
                className="w-full px-3.5 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
                required
              />
            </div>

            <div>
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">
                Model &amp; Provider
              </label>
              <select
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
              >
                <option value="odon-auto">Auto — model aktif saat ini</option>
                {providers.map((provider) => (
                  <optgroup key={provider.id} label={provider.name}>
                    {provider.models.map((entry) => {
                      const value = `${provider.id}::${entry.id}`;
                      return (
                        <option key={value} value={value}>
                          {entry.name || entry.id}
                        </option>
                      );
                    })}
                  </optgroup>
                ))}
                {model !== 'odon-auto' &&
                  !providers.some((provider) =>
                    provider.models.some((entry) => `${provider.id}::${entry.id}` === model),
                  ) && <option value={model}>{model} (saved)</option>}
              </select>
              <p className="mt-1 text-[11px] text-neutral-500 dark:text-neutral-400">
                Runs on the model picked here. Choose Auto to follow the model you are chatting with.
              </p>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              <div>
                <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Retries</label>
                <input
                  type="number"
                  min={0}
                  value={retryCount}
                  onChange={(e) => setRetryCount(Number(e.target.value))}
                  className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
                />
              </div>
              <div>
                <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Retry Delay (s)</label>
                <input
                  type="number"
                  min={1}
                  value={retryDelay}
                  onChange={(e) => setRetryDelay(Number(e.target.value))}
                  className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
                />
              </div>
              <div>
                <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Depends On</label>
                <input
                  type="text"
                  value={dependsOn}
                  onChange={(e) => setDependsOn(e.target.value)}
                  placeholder="job-id, job-id"
                  className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
                />
              </div>
            </div>

            <div>
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Agent Instructions / Prompt</label>
              <textarea
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                rows={3}
                placeholder="What should the agent execute on each run?"
                className="w-full px-3.5 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
                required
              />
            </div>

            <div className="flex items-center justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setIsCreateOpen(false)}
                className="px-4 py-2 text-xs font-medium text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200 rounded-xl"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="px-4 py-2 text-xs font-bold text-white bg-blue-600 hover:bg-blue-700 rounded-xl transition-colors shadow-2xs"
              >
                Save Automation
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
};

