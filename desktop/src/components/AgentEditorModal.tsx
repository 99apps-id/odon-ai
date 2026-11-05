import React, { useEffect, useState } from 'react';
import { X, Save, Loader2 } from 'lucide-react';
import { api } from '../api';
import type { SkillItemInfo } from '../api';
import type {
  Agent,
  AgentTelegramStatus,
  AgentWebhookStatus,
  ProviderItem,
} from '../types';
import { agentOnShift } from './office/model';

interface AgentEditorModalProps {
  agent: Agent | null;
  open: boolean;
  onClose: () => void;
  onSaved: (agent: Agent) => void;
}

const WEEKDAYS: Array<{ value: number; label: string }> = [
  { value: 1, label: 'Mon' },
  { value: 2, label: 'Tue' },
  { value: 3, label: 'Wed' },
  { value: 4, label: 'Thu' },
  { value: 5, label: 'Fri' },
  { value: 6, label: 'Sat' },
  { value: 7, label: 'Sun' },
];

const COMMON_TIMEZONES = [
  'Asia/Jakarta',
  'Asia/Makassar',
  'Asia/Jayapura',
  'Asia/Singapore',
  'Asia/Tokyo',
  'Asia/Dubai',
  'Europe/London',
  'Europe/Berlin',
  'America/New_York',
  'America/Chicago',
  'America/Los_Angeles',
  'Australia/Sydney',
  'Pacific/Auckland',
  'UTC',
];

const EMPTY_AGENT: Agent = {
  id: '',
  name: '',
  role: '',
  color: '#557a95',
  system_prompt: '',
  model: 'odon-auto',
  workspace_root: null,
  skills: [],
  enabled: true,
  sort_order: 0,
  schedule: { start: '09:00', end: '18:00', days: [1, 2, 3, 4, 5], timezone: null },
  created_at: '',
  updated_at: '',
};

/**
 * Edit a persisted agent: identity, specialization, prompt, model, working
 * hours. Saving writes straight to the daemon so the office desk updates on the
 * next poll without a restart.
 */
export function AgentEditorModal({ agent, open, onClose, onSaved }: AgentEditorModalProps) {
  const [draft, setDraft] = useState<Agent>(agent || EMPTY_AGENT);
  const [skillsCatalog, setSkillsCatalog] = useState<SkillItemInfo[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [providers, setProviders] = useState<ProviderItem[]>([]);
  const [providerId, setProviderId] = useState('');
  const [modelFilter, setModelFilter] = useState('');
  const [telegram, setTelegram] = useState<AgentTelegramStatus | null>(null);
  const [telegramToken, setTelegramToken] = useState('');
  const [telegramUsers, setTelegramUsers] = useState('');
  const [telegramDisconnect, setTelegramDisconnect] = useState(false);
  const [telegramBusy, setTelegramBusy] = useState(false);
  const [telegramError, setTelegramError] = useState('');
  const [webhook, setWebhook] = useState<AgentWebhookStatus | null>(null);
  const [webhookBusy, setWebhookBusy] = useState(false);
  const [webhookError, setWebhookError] = useState('');
  const [webhookCopied, setWebhookCopied] = useState(false);

  useEffect(() => {
    if (!open) return;
    const next = agent || EMPTY_AGENT;
    setDraft({ ...next, schedule: { ...next.schedule } });
    const parts = (next.model || '').includes('::') ? next.model.split('::') : ['', next.model];
    setProviderId(parts[0] || '');
    setModelFilter('');
    setError('');
  }, [agent, open]);

  useEffect(() => {
    if (!open) return;
    api
      .getSkills()
      .then((data) => setSkillsCatalog(data.skills || []))
      .catch(() => setSkillsCatalog([]));
  }, [open]);

  useEffect(() => {
    if (!open) return;
    api
      .getModels()
      .then((data) => setProviders(data.providers || []))
      .catch(() => setProviders([]));
  }, [open]);

  // Telegram status is per persisted agent: a new agent has nothing to load.
  useEffect(() => {
    if (!open || !draft.id) {
      setTelegram(null);
      setTelegramToken('');
      setTelegramUsers('');
      setTelegramDisconnect(false);
      setTelegramError('');
      setWebhook(null);
      setWebhookError('');
      return;
    }
    let cancelled = false;
    api
      .getAgentTelegram(draft.id)
      .then((status) => {
        if (cancelled) return;
        setTelegram(status);
        setTelegramUsers((status.allowed_users || []).join(', '));
        setTelegramDisconnect(false);
        setTelegramError('');
      })
      .catch(() => {
        if (!cancelled) {
          setTelegram(null);
          setTelegramError('Could not load the Telegram status for this agent.');
        }
      });
    api
      .getAgentWebhook(draft.id)
      .then((status) => {
        if (!cancelled) {
          setWebhook(status);
          setWebhookError('');
        }
      })
      .catch(() => {
        if (!cancelled) setWebhook(null);
      });
    return () => {
      cancelled = true;
    };
  }, [open, draft.id]);

  // A model saved without a provider prefix still belongs to exactly one
  // provider; infer it so the dropdown opens on the right row.
  useEffect(() => {
    if (!open || providerId || !draft.model || draft.model === 'odon-auto') return;
    const match = providers.find((provider) =>
      provider.models.some((model) => model.id === draft.model),
    );
    if (match) setProviderId(match.id);
  }, [open, providerId, draft.model, providers]);

  const selectedProvider = providers.find((provider) => provider.id === providerId);
  const skillChoices = Array.from(
    new Set([...skillsCatalog.map((skill) => skill.name), ...(draft.skills || [])]),
  );
  const toggleSkill = (name: string) => {
    const current = draft.skills || [];
    patch({
      skills: current.includes(name)
        ? current.filter((skill) => skill !== name)
        : [...current, name],
    });
  };
  const providerOptions = providers.filter(
    (provider) => provider.configured || provider.connected || provider.id === providerId,
  );
  const query = modelFilter.trim().toLowerCase();
  const modelOptions = (selectedProvider?.models || []).filter(
    (model) =>
      !query ||
      `${model.id} ${model.name || ''}`.toLowerCase().includes(query),
  );
  const knownModel = modelOptions.some(
    (model) => `${selectedProvider?.id}::${model.id}` === draft.model,
  );

  const chooseProvider = (id: string) => {
    setProviderId(id);
    if (!id) {
      patch({ model: 'odon-auto' });
      return;
    }
    const provider = providers.find((row) => row.id === id);
    const recommended =
      provider?.models.find((model) => model.recommended) || provider?.models[0];
    patch({ model: recommended ? `${id}::${recommended.id}` : 'odon-auto' });
  };

  if (!open) return null;

  const patch = (values: Partial<Agent>) => setDraft((prev) => ({ ...prev, ...values }));
  const patchSchedule = (values: Partial<Agent['schedule']>) =>
    setDraft((prev) => ({ ...prev, schedule: { ...prev.schedule, ...values } }));

  const toggleDay = (day: number) => {
    const days = draft.schedule.days.includes(day)
      ? draft.schedule.days.filter((value) => value !== day)
      : [...draft.schedule.days, day].sort((a, b) => a - b);
    patchSchedule({ days });
  };

  const generateWebhook = async () => {
    if (!draft.id) return;
    setWebhookBusy(true);
    setWebhookError('');
    try {
      setWebhook(await api.saveAgentWebhook(draft.id));
    } catch (e: any) {
      setWebhookError(e?.message || 'Could not create the webhook.');
    } finally {
      setWebhookBusy(false);
    }
  };

  const removeWebhook = async () => {
    if (!draft.id) return;
    setWebhookBusy(true);
    setWebhookError('');
    try {
      await api.deleteAgentWebhook(draft.id);
      setWebhook({ configured: false, url: '', method: 'POST' });
    } catch (e: any) {
      setWebhookError(e?.message || 'Could not remove the webhook.');
    } finally {
      setWebhookBusy(false);
    }
  };

  const copyWebhook = () => {
    if (!webhook?.url) return;
    void navigator.clipboard.writeText(webhook.url);
    setWebhookCopied(true);
    window.setTimeout(() => setWebhookCopied(false), 2000);
  };

  const save = async () => {
    if (!draft.name.trim()) {
      setError('Name is required.');
      return;
    }
    setSaving(true);
    setError('');
    try {
      const payload: Partial<Agent> & { id?: string } = {
        id: draft.id || undefined,
        name: draft.name.trim(),
        role: draft.role.trim() || 'General',
        color: draft.color || '#557a95',
        system_prompt: draft.system_prompt,
        model: draft.model.trim() || 'odon-auto',
        workspace_root: draft.workspace_root?.trim() || null,
        skills: [...(draft.skills || [])],
        enabled: draft.enabled,
        sort_order: draft.sort_order,
        schedule: draft.schedule,
      };
      const saved = draft.id
        ? await api.updateAgent(draft.id, payload)
        : await api.saveAgent(payload);
      if (saved.id) {
        const users = telegramUsers
          .split(',')
          .map((value) => value.trim())
          .filter(Boolean);
        const usersChanged =
          users.join(',') !== (telegram?.allowed_users || []).join(',');
        if (telegramDisconnect) {
          setTelegramBusy(true);
          await api.deleteAgentTelegram(saved.id);
        } else if (telegramToken.trim() || (telegram?.configured && usersChanged)) {
          setTelegramBusy(true);
          await api.saveAgentTelegram(saved.id, telegramToken.trim(), users);
        }
      }
      onSaved(saved);
      onClose();
    } catch (e: any) {
      setError(e?.message || 'Could not save the agent.');
    } finally {
      setSaving(false);
      setTelegramBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-neutral-900/30 p-4">
      <div className="flex max-h-[88vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-neutral-200 bg-white shadow-2xl">
        <div className="flex items-center justify-between border-b border-neutral-100 px-4 py-3">
          <div>
            <h3 className="text-sm font-semibold text-neutral-900">
              {draft.id ? `Edit ${draft.name || 'agent'}` : 'New agent'}
            </h3>
            <p className="text-[11px] text-neutral-500">
              Identity, specialization, prompt and working hours update the office desk.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1.5 text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="flex-1 space-y-3 overflow-y-auto px-4 py-3">
          <div className="grid grid-cols-2 gap-3">
            <label className="text-[11px] font-medium text-neutral-600">
              Name
              <input
                value={draft.name}
                onChange={(e) => patch({ name: e.target.value })}
                className="mt-1 w-full rounded-lg border border-neutral-200 px-2.5 py-1.5 text-xs text-neutral-800 outline-none focus:border-blue-500"
                placeholder="Coding"
              />
            </label>
            <label className="text-[11px] font-medium text-neutral-600">
              Specialization
              <input
                value={draft.role}
                onChange={(e) => patch({ role: e.target.value })}
                className="mt-1 w-full rounded-lg border border-neutral-200 px-2.5 py-1.5 text-xs text-neutral-800 outline-none focus:border-blue-500"
                placeholder="Software Engineering"
              />
            </label>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <label className="text-[11px] font-medium text-neutral-600">
              Provider
              <select
                value={providerId}
                onChange={(e) => chooseProvider(e.target.value)}
                className="mt-1 w-full rounded-lg border border-neutral-200 bg-white px-2.5 py-1.5 text-xs text-neutral-800 outline-none focus:border-blue-500"
              >
                <option value="">Auto — follow the composer</option>
                {providerOptions.map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.name}
                    {provider.configured || provider.connected ? '' : ' (not configured)'}
                  </option>
                ))}
              </select>
            </label>
            <label className="text-[11px] font-medium text-neutral-600">
              Model
              <select
                value={draft.model}
                onChange={(e) => patch({ model: e.target.value })}
                className="mt-1 w-full rounded-lg border border-neutral-200 bg-white px-2.5 py-1.5 text-xs text-neutral-800 outline-none focus:border-blue-500"
              >
                <option value="odon-auto">odon-auto — follow the composer</option>
                {draft.model && draft.model !== 'odon-auto' && !knownModel && (
                  <option value={draft.model}>{draft.model} (custom)</option>
                )}
                {modelOptions.map((model) => (
                  <option key={model.id} value={`${providerId}::${model.id}`}>
                    {model.name || model.id}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <label className="text-[11px] font-medium text-neutral-600">
              Filter models
              <input
                value={modelFilter}
                onChange={(e) => setModelFilter(e.target.value)}
                className="mt-1 w-full rounded-lg border border-neutral-200 px-2.5 py-1.5 text-xs text-neutral-800 outline-none focus:border-blue-500"
                placeholder="Type to narrow the model list…"
              />
              <span className="mt-1 block text-[10px] font-normal leading-snug text-neutral-400">
                The saved value is always <code>provider::model</code>, so the agent never
                falls back to the default provider.
              </span>
            </label>
            <label className="text-[11px] font-medium text-neutral-600">
              Desk color
              <input
                type="color"
                value={draft.color || '#557a95'}
                onChange={(e) => patch({ color: e.target.value })}
                className="mt-1 h-8 w-full cursor-pointer rounded-lg border border-neutral-200 bg-white"
              />
            </label>
          </div>

          <label className="block text-[11px] font-medium text-neutral-600">
            System prompt / task
            <textarea
              value={draft.system_prompt}
              onChange={(e) => patch({ system_prompt: e.target.value })}
              rows={5}
              className="mt-1 w-full resize-y rounded-lg border border-neutral-200 px-2.5 py-1.5 text-xs leading-relaxed text-neutral-800 outline-none focus:border-blue-500"
              placeholder="What this agent owns, what it must not do, and when it must ask for approval."
            />
          </label>

          <div className="grid grid-cols-2 gap-3">
            <label className="text-[11px] font-medium text-neutral-600">
              Workspace (optional)
              <input
                value={draft.workspace_root || ''}
                onChange={(e) => patch({ workspace_root: e.target.value })}
                className="mt-1 w-full rounded-lg border border-neutral-200 px-2.5 py-1.5 font-mono text-xs text-neutral-800 outline-none focus:border-blue-500"
                placeholder="Use the daemon default"
              />
            </label>
            <div className="text-[11px] font-medium text-neutral-600">
              Skills
              {(skillChoices.length > 0 || (draft.skills || []).length > 0) && (
                <div className="mt-1 flex flex-wrap gap-1">
                  {skillChoices.map((name) => {
                    const active = (draft.skills || []).includes(name);
                    const info = skillsCatalog.find((skill) => skill.name === name);
                    return (
                      <button
                        key={name}
                        type="button"
                        aria-pressed={active}
                        title={info?.description || name}
                        onClick={() => toggleSkill(name)}
                        className={`rounded-md px-1.5 py-0.5 text-[10px] font-medium transition-colors ${
                          active
                            ? 'bg-blue-600 text-white hover:bg-blue-700'
                            : 'bg-white text-neutral-500 ring-1 ring-neutral-200 hover:bg-neutral-100'
                        }`}
                      >
                        {info?.title || name}
                      </button>
                    );
                  })}
                </div>
              )}
              <span className="mt-1 block text-[10px] font-normal leading-snug text-neutral-400">
                {skillChoices.length > 0
                  ? 'Tap to assign the skills this agent may load.'
                  : 'No skills published yet.'}
              </span>
            </div>
          </div>

          <div className="rounded-xl border border-neutral-200 bg-neutral-50/60 p-3">
            <div className="text-[11px] font-semibold text-neutral-700">Working hours</div>
            <div className="mt-2 flex items-center gap-2">
              <input
                type="time"
                value={draft.schedule.start}
                onChange={(e) => patchSchedule({ start: e.target.value })}
                className="rounded-lg border border-neutral-200 px-2 py-1 text-xs text-neutral-800"
              />
              <span className="text-[11px] text-neutral-400">to</span>
              <input
                type="time"
                value={draft.schedule.end}
                onChange={(e) => patchSchedule({ end: e.target.value })}
                className="rounded-lg border border-neutral-200 px-2 py-1 text-xs text-neutral-800"
              />
            </div>
            <label className="mt-2 block text-[10px] font-medium text-neutral-500">
              Timezone (IANA, optional — defaults to this machine)
              <input
                value={draft.schedule.timezone || ''}
                onChange={(e) => patchSchedule({ timezone: e.target.value.trim() || null })}
                list="agent-timezones"
                className="mt-1 w-full rounded-lg border border-neutral-200 px-2 py-1 font-mono text-xs text-neutral-800 outline-none focus:border-blue-500"
                placeholder="Asia/Jakarta"
              />
              <datalist id="agent-timezones">
                {COMMON_TIMEZONES.map((zone) => (
                  <option key={zone} value={zone} />
                ))}
              </datalist>
            </label>
            <div className="mt-2 flex flex-wrap gap-1">
              {WEEKDAYS.map((day) => (
                <button
                  key={day.value}
                  type="button"
                  onClick={() => toggleDay(day.value)}
                  className={`rounded-md px-2 py-1 text-[10px] font-medium transition-colors ${
                    draft.schedule.days.includes(day.value)
                      ? 'bg-blue-600 text-white'
                      : 'bg-white text-neutral-500 ring-1 ring-neutral-200 hover:bg-neutral-100'
                  }`}
                >
                  {day.label}
                </button>
              ))}
            </div>
            <p className="mt-2 text-[10px] leading-snug text-neutral-500">
              Outside these hours the desk is empty; idle agents fall asleep during the shift.
            </p>
            <div
              className={`mt-2 inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[10px] font-semibold ${
                agentOnShift(draft)
                  ? 'bg-emerald-100 text-emerald-700'
                  : 'bg-neutral-200 text-neutral-600'
              }`}
            >
              <span className="h-1.5 w-1.5 rounded-full bg-current" />
              Right now: {agentOnShift(draft) ? 'on shift' : 'off shift'}
            </div>
          </div>

          <div className="rounded-xl border border-sky-200 bg-sky-50/40 p-3">
            <div className="flex items-start justify-between gap-2">
              <div>
                <div className="text-[11px] font-semibold text-neutral-700">
                  Telegram connector
                </div>
                <div className="text-[10px] leading-snug text-neutral-500">
                  One bot for this agent. WhatsApp stays the single main relay.
                </div>
              </div>
              <span
                className={`shrink-0 rounded-full px-2 py-0.5 text-[9px] font-semibold ${
                  telegram?.configured
                    ? telegram.online
                      ? 'bg-emerald-100 text-emerald-700'
                      : 'bg-amber-100 text-amber-800'
                    : 'bg-neutral-200 text-neutral-600'
                }`}
              >
                {telegram?.configured
                  ? telegram.online
                    ? 'online'
                    : 'offline'
                  : 'not configured'}
              </span>
            </div>
            {telegram?.configured && telegram.token_masked && (
              <div className="mt-1 font-mono text-[10px] text-neutral-500">
                {telegram.token_masked}
              </div>
            )}
            <input
              type="password"
              value={telegramToken}
              onChange={(e) => setTelegramToken(e.target.value)}
              className="mt-2 w-full rounded-lg border border-neutral-200 px-2.5 py-1.5 font-mono text-xs text-neutral-800 outline-none focus:border-blue-500 disabled:bg-neutral-100"
              placeholder={
                telegram?.configured
                  ? 'Leave blank to keep the current token'
                  : '123456:ABC-DEF...'
              }
              disabled={telegramDisconnect}
            />
            <input
              value={telegramUsers}
              onChange={(e) => setTelegramUsers(e.target.value)}
              className="mt-2 w-full rounded-lg border border-neutral-200 px-2.5 py-1.5 text-xs text-neutral-800 outline-none focus:border-blue-500 disabled:bg-neutral-100"
              placeholder="Allowed user ids/usernames, comma separated"
              disabled={telegramDisconnect}
            />
            <p className="mt-1 text-[10px] leading-snug text-neutral-500">
              Empty allowlist = the bot rejects everyone (secure default). Send /chatid to
              the bot to discover an id.
            </p>
            {telegramError && (
              <div className="mt-2 rounded-lg border border-clay-200 bg-clay-50 px-2 py-1 text-[10px] text-clay-800">
                {telegramError}
              </div>
            )}
            {telegram?.configured && (
              <button
                type="button"
                onClick={() => setTelegramDisconnect((value) => !value)}
                className={`mt-2 rounded-lg border px-2.5 py-1 text-[10px] font-semibold transition-colors ${
                  telegramDisconnect
                    ? 'border-clay-300 bg-clay-50 text-clay-800'
                    : 'border-neutral-200 bg-white text-neutral-600 hover:bg-neutral-100'
                }`}
              >
                {telegramDisconnect
                  ? 'Will disconnect on save (click to cancel)'
                  : 'Disconnect Telegram bot'}
              </button>
            )}
          </div>

          <div className="rounded-xl border border-violet-200 bg-violet-50/40 p-3">
            <div className="flex items-start justify-between gap-2">
              <div>
                <div className="text-[11px] font-semibold text-neutral-700">
                  Webhook / N8N
                </div>
                <div className="text-[10px] leading-snug text-neutral-500">
                  Inbound trigger for this agent. POST JSON with a text field and get the
                  reply back in the same request.
                </div>
              </div>
              <span
                className={`shrink-0 rounded-full px-2 py-0.5 text-[9px] font-semibold ${
                  webhook?.configured
                    ? 'bg-emerald-100 text-emerald-700'
                    : 'bg-neutral-200 text-neutral-600'
                }`}
              >
                {webhook?.configured ? 'active' : 'not configured'}
              </span>
            </div>

            {webhook?.configured ? (
              <>
                <div className="mt-2 flex items-center gap-1.5">
                  <input
                    readOnly
                    value={webhook.url}
                    className="min-w-0 flex-1 rounded-lg border border-neutral-200 bg-white px-2.5 py-1.5 font-mono text-[10px] text-neutral-700"
                  />
                  <button
                    type="button"
                    onClick={copyWebhook}
                    className="shrink-0 rounded-lg border border-neutral-200 bg-white px-2 py-1.5 text-[10px] font-semibold text-neutral-600 hover:bg-neutral-100"
                  >
                    {webhookCopied ? 'Copied' : 'Copy'}
                  </button>
                </div>
                <pre className="mt-2 overflow-x-auto rounded-lg bg-neutral-900 p-2 text-[10px] leading-snug text-neutral-100">
{`POST ${webhook.url}
Content-Type: application/json

{"text":"..."}`}
                </pre>
                <div className="mt-2 flex gap-1.5">
                  <button
                    type="button"
                    onClick={generateWebhook}
                    disabled={webhookBusy}
                    className="rounded-lg border border-neutral-200 bg-white px-2.5 py-1 text-[10px] font-semibold text-neutral-600 hover:bg-neutral-100 disabled:opacity-50"
                  >
                    {webhookBusy ? 'Working…' : 'Rotate secret'}
                  </button>
                  <button
                    type="button"
                    onClick={removeWebhook}
                    disabled={webhookBusy}
                    className="rounded-lg border border-clay-200 bg-clay-50 px-2.5 py-1 text-[10px] font-semibold text-clay-800 hover:bg-clay-100 disabled:opacity-50"
                  >
                    Remove
                  </button>
                </div>
                <p className="mt-1 text-[10px] leading-snug text-neutral-500">
                  Rotating invalidates the old URL immediately. Optional fields:
                  <code> session_id</code> (your own conversation id), <code>source</code>,
                  <code> attachments</code>.
                </p>
              </>
            ) : (
              <button
                type="button"
                onClick={generateWebhook}
                disabled={!draft.id || webhookBusy}
                className="mt-2 rounded-lg bg-violet-600 px-2.5 py-1.5 text-[10px] font-semibold text-white hover:bg-violet-700 disabled:opacity-50"
              >
                {!draft.id
                  ? 'Save the agent first'
                  : webhookBusy
                    ? 'Creating…'
                    : 'Generate webhook URL'}
              </button>
            )}
            {webhookError && (
              <div className="mt-2 rounded-lg border border-clay-200 bg-clay-50 px-2 py-1 text-[10px] text-clay-800">
                {webhookError}
              </div>
            )}
          </div>

          <label className="flex items-center gap-2 text-[11px] font-medium text-neutral-600">
            <input
              type="checkbox"
              checked={draft.enabled}
              onChange={(e) => patch({ enabled: e.target.checked })}
              className="h-3.5 w-3.5 rounded border-neutral-300"
            />
            Show this agent in the office
          </label>

          {error && (
            <div className="rounded-lg border border-clay-200 bg-clay-50 px-2.5 py-1.5 text-[11px] text-clay-800">
              {error}
            </div>
          )}
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-neutral-100 px-4 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-200 px-3 py-1.5 text-xs font-medium text-neutral-600 hover:bg-neutral-100"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={save}
            disabled={saving}
            className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Save className="h-3.5 w-3.5" />}
            Save agent
          </button>
        </div>
      </div>
    </div>
  );
}
