import React, { useState, useEffect } from 'react';
import {
  X,
  Shield,
  Server,
  Key,
  Sliders,
  Check,
  Cpu,
  Sparkles,
  ExternalLink,
  ChevronRight,
  Globe,
  Lock,
  Eye,
  EyeOff,
  Copy,
  RotateCcw,
  AlertCircle,
  LogOut,
  Radio,
  Plus,
  Trash2,
  Bot,
  Terminal,
  Layers,
  FileCode,
  Play,
  ShieldAlert,
  Wifi,
  FileText,
  Loader2,
  Zap,
} from 'lucide-react';
import { api, OAuthAccountInfo, OAuthLoginResponse, SshProfile, SkillItemInfo } from '../api';
import { ProviderItem, ModelItem, FALLBACK_PROVIDERS, getCustomOpenAiModels, saveCustomOpenAiModels } from '../types';
import { ProviderLogo } from './ProviderLogo';
import { RelaysPanel } from './RelaysPanel';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  isDark: boolean;
  onToggleTheme: () => void;
  activeProviderId?: string;
  onSelectProvider?: (providerId: string) => void;
  currentModel?: string;
  onSelectModel?: (modelId: string) => void;
}

const DEFAULT_BASE_URLS: Record<string, string> = {
  deepseek: 'https://api.deepseek.com/v1',
  qwen: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
  alibaba: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  stepfun: 'https://api.stepfun.com/v1',
  openai: 'https://api.openai.com/v1',
  anthropic: 'https://api.anthropic.com/v1',
  google: 'https://generativelanguage.googleapis.com/v1beta/openai',
  groq: 'https://api.groq.com/openai/v1',
  mistral: 'https://api.mistral.ai/v1',
  openrouter: 'https://openrouter.ai/api/v1',
  ollama: 'http://localhost:11434/v1',
  'openai-compatible': 'http://localhost:11434/v1',
};

const KEY_PREFIXES: Record<string, string> = {
  deepseek: 'sk-',
  qwen: 'sk-',
  alibaba: 'sk-',
  openai: 'sk-',
  anthropic: 'sk-ant-',
  google: 'AIza',
  groq: 'gsk_',
  openrouter: 'sk-or-',
};

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  isDark,
  onToggleTheme,
  activeProviderId,
  onSelectProvider,
  currentModel,
  onSelectModel,
}) => {
  type SettingsTab =
    | 'providers'
    | 'endpoints'
    | 'agents'
    | 'skills'
    | 'mcp'
    | 'harness'
    | 'relays'
    | 'ssh'
    | 'system';
  const [activeTab, setActiveTab] = useState<SettingsTab>('providers');

  // Agents & Personas State
  const [activePersona, setActivePersona] = useState<string>(() => {
    if (typeof window !== 'undefined') return localStorage.getItem('odon_active_persona') || 'coder';
    return 'coder';
  });
  const [customInstructions, setCustomInstructions] = useState<string>(() => {
    if (typeof window !== 'undefined') return localStorage.getItem('odon_custom_instructions') || '';
    return '';
  });
  const [effortLevel, setEffortLevel] = useState<string>(() => {
    if (typeof window !== 'undefined') return localStorage.getItem('odon_effort_level') || 'high';
    return 'high';
  });

  // Skills State
  const [skillsList, setSkillsList] = useState<SkillItemInfo[]>([]);
  const [newSkillName, setNewSkillName] = useState<string>('');
  const [newSkillContent, setNewSkillContent] = useState<string>('');
  const [isSkillFormOpen, setIsSkillFormOpen] = useState<boolean>(false);

  // MCP Servers State
  const [mcpServers, setMcpServers] = useState<any[]>([]);
  const [newMcpName, setNewMcpName] = useState<string>('');
  const [newMcpCommand, setNewMcpCommand] = useState<string>('');
  const [newMcpTransport, setNewMcpTransport] = useState<string>('stdio');

  // Harness & Policies State
  const [executionMode, setExecutionMode] = useState<string>('Autonomous');
  const [planMode, setPlanMode] = useState<boolean>(() => {
    if (typeof window !== 'undefined') return localStorage.getItem('odon_plan_mode') === 'true';
    return false;
  });
  const [policies, setPolicies] = useState<string[]>([]);

  // SSH Profiles State
  const [sshProfiles, setSshProfiles] = useState<SshProfile[]>([]);

  const [providers, setProviders] = useState<ProviderItem[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<string>(() => {
    return activeProviderId || (typeof window !== 'undefined' ? localStorage.getItem('odon_active_provider') : null) || 'openai';
  });

  useEffect(() => {
    if (activeProviderId) {
      setSelectedProviderId(activeProviderId);
    }
  }, [activeProviderId]);
  const [daemonUrl, setDaemonUrl] = useState(api.getBaseUrl() || 'http://127.0.0.1:4096');
  const [apiToken, setApiToken] = useState<string>(() => api.getApiToken());
  const [daemonConnected, setDaemonConnected] = useState<boolean | null>(null);

  // OAuth Accounts State
  const [oauthAccounts, setOauthAccounts] = useState<Record<string, OAuthAccountInfo>>({});
  const [oauthBusy, setOauthBusy] = useState<string | null>(null);
  const [oauthPrompt, setOauthPrompt] = useState<{
    provider: string;
    userCode?: string;
    verificationUri?: string;
    authUrl?: string;
  } | null>(null);
  const [copiedCode, setCopiedCode] = useState(false);

  // BYOK keys & base urls state
  // API keys are never stored in the browser. They are sent once to the daemon
  // and persisted server-side; the UI only ever shows the masked value.
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});

  const [baseUrls, setBaseUrls] = useState<Record<string, string>>(() => {
    if (typeof window === 'undefined') return DEFAULT_BASE_URLS;
    const urls: Record<string, string> = {};
    for (const [id, def] of Object.entries(DEFAULT_BASE_URLS)) {
      urls[id] = localStorage.getItem(`odon_base_${id}`) || def;
    }
    return urls;
  });

  const [showKey, setShowKey] = useState<Record<string, boolean>>({});
  const [saved, setSaved] = useState(false);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [configuredKeys, setConfiguredKeys] = useState<Record<string, string>>({});
  const [testing, setTesting] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<Record<string, { ok: boolean; text: string }>>({});

  // Custom models state for OpenAI-Compatible provider
  const [customOpenAiModels, setCustomOpenAiModels] = useState<ModelItem[]>(() => getCustomOpenAiModels());
  const [newModelId, setNewModelId] = useState('');
  const [newModelName, setNewModelName] = useState('');
  const [newModelContext, setNewModelContext] = useState(128000);

  // Multiple OpenAI-compatible endpoints. Each one is stored as its own
  // provider id (compat-*) and registered independently by the daemon.
  type EndpointConfig = {
    provider_id: string;
    masked_key: string;
    base_url?: string;
    is_default: boolean;
    name?: string;
    models?: string[];
  };
  const [providerConfigs, setProviderConfigs] = useState<EndpointConfig[]>([]);
  const [editingEndpointId, setEditingEndpointId] = useState<string | null>(null);
  const [endpointName, setEndpointName] = useState('');
  const [endpointBaseUrl, setEndpointBaseUrl] = useState('https://api.openai.com/v1');
  const [endpointKey, setEndpointKey] = useState('');
  const [endpointModels, setEndpointModels] = useState('');
  const [endpointModelDraft, setEndpointModelDraft] = useState('');
  const [endpointBusy, setEndpointBusy] = useState(false);
  const [endpointError, setEndpointError] = useState<string | null>(null);
  const customEndpoints = providerConfigs.filter((c) => c.provider_id.startsWith('compat-'));
  const endpointModelList = endpointModels
    .split(/[\n,]/)
    .map((model) => model.trim())
    .filter(Boolean);

  const addEndpointModel = () => {
    const modelId = endpointModelDraft.trim();
    if (!modelId) return;
    const next = Array.from(new Set([...endpointModelList, modelId]));
    setEndpointModels(next.join('\n'));
    setEndpointModelDraft('');
  };

  const removeEndpointModel = (modelId: string) => {
    setEndpointModels(endpointModelList.filter((model) => model !== modelId).join('\n'));
  };

  const refreshProviders = async () => {
    const [data, configs] = await Promise.all([
      api.getModels().catch(() => null),
      api.getProviderConfigs().catch(() => [] as EndpointConfig[]),
    ]);
    if (data?.providers?.length) setProviders(data.providers);
    setProviderConfigs(configs || []);
  };

  const slugifyProvider = (value: string) =>
    value
      .toLowerCase()
      .trim()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 40) || 'endpoint';

  const nextEndpointId = (name: string) => {
    const base = `compat-${slugifyProvider(name)}`;
    let id = base;
    let suffix = 2;
    while (providerConfigs.some((c) => c.provider_id === id)) {
      id = `${base}-${suffix++}`;
    }
    return id;
  };

  const resetEndpointForm = () => {
    setEditingEndpointId(null);
    setEndpointName('');
    setEndpointBaseUrl('https://api.openai.com/v1');
    setEndpointKey('');
    setEndpointModels('');
    setEndpointModelDraft('');
    setEndpointError(null);
  };

  const handleSaveEndpoint = async () => {
    if (!endpointName.trim() || !endpointBaseUrl.trim()) {
      setEndpointError('Provider name and Base URL are required.');
      return;
    }
    const models = Array.from(
      new Set(
        [...endpointModelList, endpointModelDraft.trim()]
          .map((m) => m.trim())
          .filter(Boolean),
      ),
    );
    const providerId = editingEndpointId || nextEndpointId(endpointName);
    setEndpointBusy(true);
    setEndpointError(null);
    try {
      await api.saveProviderConfig(
        providerId,
        endpointKey.trim(),
        endpointBaseUrl.trim(),
        false,
        endpointName.trim(),
        models,
      );
      await refreshProviders();
      resetEndpointForm();
    } catch (e: any) {
      setEndpointError(e.message || 'Failed to save endpoint.');
    } finally {
      setEndpointBusy(false);
    }
  };

  const handleEditEndpoint = (c: EndpointConfig) => {
    setEditingEndpointId(c.provider_id);
    setEndpointName(c.name || c.provider_id);
    setEndpointBaseUrl(c.base_url || '');
    setEndpointKey('');
    setEndpointModels((c.models || []).join('\n'));
    setEndpointModelDraft('');
    setEndpointError(null);
  };

  const handleDeleteEndpoint = async (providerId: string) => {
    if (!window.confirm(`Delete endpoint ${providerId}?`)) return;
    try {
      await api.deleteProviderConfig(providerId);
      await refreshProviders();
      if (editingEndpointId === providerId) resetEndpointForm();
    } catch (e: any) {
      setEndpointError(e.message || 'Failed to delete endpoint.');
    }
  };

  const handleUseEndpoint = (c: EndpointConfig) => {
    onSelectProvider?.(c.provider_id);
    setSelectedProviderId(c.provider_id);
    if (typeof window !== 'undefined') {
      localStorage.setItem('odon_active_provider', c.provider_id);
    }
    const firstModel = c.models?.[0];
    if (firstModel) {
      onSelectModel?.(firstModel);
      localStorage.setItem('odon_selected_model', firstModel);
    }
  };

  const handleAddCustomModel = () => {
    const trimmedId = newModelId.trim();
    if (!trimmedId) return;
    const model: ModelItem = {
      id: trimmedId,
      name: newModelName.trim() || trimmedId,
      context: Number(newModelContext) || 128000,
      recommended: false,
    };
    const updated = [...customOpenAiModels.filter((m) => m.id !== model.id), model];
    setCustomOpenAiModels(updated);
    saveCustomOpenAiModels(updated);
    setNewModelId('');
    setNewModelName('');
  };

  const handleDeleteCustomModel = (id: string) => {
    const updated = customOpenAiModels.filter((m) => m.id !== id);
    setCustomOpenAiModels(updated);
    saveCustomOpenAiModels(updated);
  };

  // Probe daemon and fetch model catalog
  useEffect(() => {
    if (!isOpen) return;

    if (typeof window !== 'undefined') {
      const requested = localStorage.getItem('odon_settings_tab');
      if (requested) {
        localStorage.removeItem('odon_settings_tab');
        const valid = ['providers', 'endpoints', 'agents', 'skills', 'mcp', 'harness', 'relays', 'ssh', 'system'];
        if (valid.includes(requested)) setActiveTab(requested as SettingsTab);
      }
    }

    api.getStatus()
      .then(() => setDaemonConnected(true))
      .catch(() => setDaemonConnected(false));

    api.getModels().then((data) => {
      if (data?.providers?.length) {
        setProviders(data.providers);
      } else {
        setProviders(FALLBACK_PROVIDERS);
      }
    }).catch(() => {
      setProviders(FALLBACK_PROVIDERS);
    });

    // Load active OAuth accounts
    api.getOAuthAccounts().then((accounts) => {
      const map: Record<string, OAuthAccountInfo> = {};
      accounts.forEach((a) => {
        map[a.provider.toLowerCase()] = a;
      });
      setOauthAccounts(map);
    }).catch(() => setOauthAccounts({}));

    // Load persisted BYOK configs from daemon
    api.getProviderConfigs().then((configs) => {
      setProviderConfigs(configs || []);
      if (configs?.length) {
        // Never put a masked key into the editable field: saving it would
        // overwrite the real credential with "sk-a…z9".
        setConfiguredKeys(() => {
          const next: Record<string, string> = {};
          for (const c of configs) next[c.provider_id] = c.masked_key;
          return next;
        });
        setBaseUrls((prev) => {
          const next = { ...prev };
          for (const c of configs) {
            if (c.base_url) next[c.provider_id] = c.base_url;
          }
          return next;
        });
      }
    }).catch(() => { });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    if (activeTab === 'skills') {
      api.getSkills().then((d) => setSkillsList(d.skills || [])).catch(() => {});
    } else if (activeTab === 'mcp') {
      api.getMcpServers().then((d) => setMcpServers(d || [])).catch(() => {});
    } else if (activeTab === 'ssh') {
      api.getSshProfiles().then((d) => setSshProfiles(d.profiles || [])).catch(() => {});
    } else if (activeTab === 'harness') {
      api.getApprovalPolicies().then((d) => setPolicies(d || [])).catch(() => {});
      api.getSettings().then((s) => setExecutionMode(s.execution_mode || 'Autonomous')).catch(() => {});
    }
  }, [isOpen, activeTab]);

  const currentProvider = providers.find((p) => p.id === selectedProviderId) || providers[0];

  // OAuth Actions
  const handleOAuthSignIn = async (providerId: string) => {
    setOauthBusy(providerId);
    setKeyError(null);
    setCopiedCode(false);
    try {
      const resp: OAuthLoginResponse = await api.loginOAuth(providerId);
      if (resp.status === 'pending_device' && resp.user_code) {
        setOauthPrompt({
          provider: providerId,
          userCode: resp.user_code,
          verificationUri: resp.verification_uri || 'https://github.com/login/device',
        });
      } else if (resp.auth_url) {
        setOauthPrompt({
          provider: providerId,
          authUrl: resp.auth_url,
        });
        window.open(resp.auth_url, '_blank');
      }

      if (resp.account) {
        setOauthAccounts((prev) => {
          return { ...prev, [providerId.toLowerCase()]: resp.account! };
        });
      }

      // The daemon finishes the token exchange in the background: poll until the
      // account is connected (or the prompt is dismissed / times out after 5 min).
      const started = Date.now();
      const poll = window.setInterval(async () => {
        if (Date.now() - started > 5 * 60 * 1000) {
          window.clearInterval(poll);
          return;
        }
        try {
          const accounts = await api.getOAuthAccounts();
          const hit = accounts.find((a) => a.provider.toLowerCase() === providerId.toLowerCase());
          if (hit?.connected) {
            window.clearInterval(poll);
            setOauthAccounts((prev) => ({ ...prev, [providerId.toLowerCase()]: hit }));
            setOauthPrompt(null);
            api.getModels().then((d) => d?.providers && setProviders(d.providers)).catch(() => { });
          }
        } catch {
          /* daemon briefly unreachable: keep polling */
        }
      }, 2000);
    } catch (err: any) {
      setKeyError(err.message || 'OAuth sign-in failed');
    } finally {
      setOauthBusy(null);
    }
  };

  const handleOAuthDisconnect = async (providerId: string) => {
    setOauthBusy(providerId);
    try {
      await api.disconnectOAuth(providerId);
      setOauthAccounts((prev) => {
        const copy = { ...prev };
        delete copy[providerId.toLowerCase()];
        return copy;
      });
      setOauthPrompt(null);
    } catch (err: any) {
      setKeyError(err.message || 'Disconnect failed');
    } finally {
      setOauthBusy(null);
    }
  };

  const handleCopyCode = (code: string) => {
    navigator.clipboard.writeText(code);
    setCopiedCode(true);
    setTimeout(() => setCopiedCode(false), 2000);
  };

  // BYOK Actions
  const handleTestConnection = async (providerId: string) => {
    setTesting(providerId);
    setKeyError(null);
    setTestResult((prev) => {
      const next = { ...prev };
      delete next[providerId];
      return next;
    });
    try {
      const rawKey = (apiKeys[providerId] || '').trim();
      if (rawKey) {
        await api.saveProviderConfig(
          providerId,
          rawKey,
          baseUrls[providerId] || DEFAULT_BASE_URLS[providerId] || undefined,
          true,
        ).catch(() => undefined);
      }
      const bareModel = currentModel?.includes('::') ? currentModel.split('::')[1] : currentModel;
      const prov = providers.find((p) => p.id === providerId);
      const provModels = providerId === 'openai-compatible' ? customOpenAiModels : (prov?.models || []);
      const preferred =
        provModels.find((m) => m.id === bareModel) ||
        provModels.find((m) => m.recommended) ||
        provModels[0];
      const res = await api.testProvider(providerId, preferred?.id || bareModel || undefined);
      setTestResult((prev) => ({
        ...prev,
        [providerId]: {
          ok: res.ok,
          text: res.ok
            ? `Connected in ${res.latency_ms ?? '?'}ms · ${(res.sample || 'OK').slice(0, 120)}`
            : res.error || 'Connection failed',
        },
      }));
    } catch (err: any) {
      setTestResult((prev) => ({
        ...prev,
        [providerId]: { ok: false, text: err?.message || 'Connection test failed' },
      }));
    } finally {
      setTesting(null);
    }
  };

  const handleSaveKey = async (providerId: string) => {
    let rawKey = (apiKeys[providerId] || '').trim();
    // Defensive: an old build could have prefilled the masked value.
    if (rawKey.includes('•')) rawKey = '';
    const prefix = KEY_PREFIXES[providerId];

    if (rawKey && prefix && !rawKey.startsWith(prefix)) {
      setKeyError(`${currentProvider.name} API keys usually begin with "${prefix}".`);
    } else {
      setKeyError(null);
    }

    localStorage.setItem(`odon_base_${providerId}`, baseUrls[providerId] || DEFAULT_BASE_URLS[providerId] || '');

    // Persist to backend daemon storage & register live provider
    try {
      await api.saveProviderConfig(providerId, rawKey, baseUrls[providerId] || DEFAULT_BASE_URLS[providerId] || undefined, true);
      const data = await api.getModels();
      if (data?.providers?.length) {
        setProviders(data.providers);
      }
    } catch (e: any) {
      console.warn('Backend saveProviderConfig warning:', e.message);
    }

    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const handleClearKey = async (providerId: string) => {
    setApiKeys((prev) => ({ ...prev, [providerId]: '' }));
    setConfiguredKeys((prev) => {
      const next = { ...prev };
      delete next[providerId];
      return next;
    });
    try {
      await api.deleteProviderConfig(providerId);
      const data = await api.getModels();
      if (data?.providers?.length) {
        setProviders(data.providers);
      }
    } catch { }
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const maskKey = (key: string) => {
    if (!key) return '';
    if (key.length <= 8) return '•'.repeat(key.length);
    return `${key.slice(0, 4)}${'•'.repeat(Math.max(4, key.length - 8))}${key.slice(-4)}`;
  };

  if (!isOpen) return null;

  const currentOAuth = oauthAccounts[selectedProviderId.toLowerCase()];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-neutral-200/70 backdrop-blur-xs animate-in fade-in duration-150">
      <div
        className="w-full max-w-5xl h-[700px] bg-white dark:bg-neutral-900 rounded-2xl shadow-2xl border border-neutral-200 dark:border-neutral-800 flex flex-col overflow-hidden text-neutral-900 dark:text-neutral-100"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800 shrink-0">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-xl bg-blue-50 dark:bg-blue-950/60 flex items-center justify-center text-blue-600 dark:text-blue-400">
              <Sliders className="w-4 h-4" />
            </div>
            <div>
              <h3 className="font-bold text-sm text-neutral-900 dark:text-white">ODON Settings Center</h3>
              <p className="text-xs text-neutral-500">Configure models, personas, skills, MCP, relays, safety harness & remote SSH</p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <div className="flex bg-neutral-100 dark:bg-neutral-800 p-1 rounded-xl text-xs font-medium overflow-x-auto max-w-2xl gap-0.5">
              <button
                onClick={() => setActiveTab('providers')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'providers'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Cpu className="w-3.5 h-3.5" />
                <span>Providers</span>
              </button>
              <button
                onClick={() => setActiveTab('endpoints')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'endpoints'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Server className="w-3.5 h-3.5" />
                <span>Endpoints</span>
              </button>
              <button
                onClick={() => setActiveTab('agents')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'agents'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Bot className="w-3.5 h-3.5" />
                <span>Agents</span>
              </button>
              <button
                onClick={() => setActiveTab('skills')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'skills'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Sparkles className="w-3.5 h-3.5" />
                <span>Skills</span>
              </button>
              <button
                onClick={() => setActiveTab('mcp')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'mcp'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Layers className="w-3.5 h-3.5" />
                <span>MCP</span>
              </button>
              <button
                onClick={() => setActiveTab('harness')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'harness'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Shield className="w-3.5 h-3.5" />
                <span>Harness</span>
              </button>
              <button
                onClick={() => setActiveTab('relays')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'relays'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Radio className="w-3.5 h-3.5" />
                <span>Relays</span>
              </button>
              <button
                onClick={() => setActiveTab('ssh')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'ssh'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Server className="w-3.5 h-3.5" />
                <span>SSH</span>
              </button>
              <button
                onClick={() => setActiveTab('system')}
                className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg transition-all ${
                  activeTab === 'system'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                <Globe className="w-3.5 h-3.5" />
                <span>System</span>
              </button>
            </div>
            <button
              onClick={onClose}
              className="text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 p-1.5 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 ml-2"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Modal Body */}
        {activeTab === 'endpoints' ? (
          <div className="flex-1 overflow-y-auto p-6 space-y-6 bg-white dark:bg-neutral-900">
            <div className="flex items-start justify-between gap-4">
              <div>
                <h3 className="text-base font-bold text-neutral-900 dark:text-white">
                  OpenAI-Compatible Endpoints
                </h3>
                <p className="text-[12.5px] text-neutral-500 dark:text-neutral-400 mt-1 max-w-2xl">
                  Add multiple OpenAI-compatible providers. Each endpoint has its own API key,
                  Base URL, model list, and provider id, and can be selected independently from
                  the main page.
                </p>
              </div>
              <span className="text-[11px] font-semibold px-2.5 py-1 rounded-full bg-blue-100 dark:bg-blue-950 text-blue-700 dark:text-blue-300 shrink-0">
                {customEndpoints.length} endpoint
              </span>
            </div>

            <div className="rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/60 dark:bg-neutral-850/40 p-5 space-y-4">
              <div className="flex items-center justify-between">
                <h4 className="text-xs font-bold uppercase tracking-wider text-neutral-700 dark:text-neutral-300 flex items-center gap-2">
                  <Plus className="w-3.5 h-3.5" />
                  {editingEndpointId ? 'Edit Endpoint' : 'Add New Endpoint'}
                </h4>
                {editingEndpointId && (
                  <button
                    onClick={resetEndpointForm}
                    className="text-[11px] font-medium text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200"
                  >
                    Cancel edit
                  </button>
                )}
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                <label className="space-y-1">
                  <span className="text-[11px] font-semibold text-neutral-600 dark:text-neutral-400">
                    Provider Name
                  </span>
                  <input
                    value={endpointName}
                    onChange={(e) => setEndpointName(e.target.value)}
                    placeholder="OpenRouter / LiteLLM / vLLM"
                    className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs text-neutral-900 dark:text-neutral-100 outline-none focus:border-blue-500"
                  />
                </label>
                <label className="space-y-1">
                  <span className="text-[11px] font-semibold text-neutral-600 dark:text-neutral-400">
                    Base URL
                  </span>
                  <input
                    value={endpointBaseUrl}
                    onChange={(e) => setEndpointBaseUrl(e.target.value)}
                    placeholder="https://api.openai.com/v1"
                    className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs font-mono text-neutral-900 dark:text-neutral-100 outline-none focus:border-blue-500"
                  />
                </label>
                <label className="space-y-1">
                  <span className="text-[11px] font-semibold text-neutral-600 dark:text-neutral-400">
                    API Key
                  </span>
                  <input
                    type="password"
                    value={endpointKey}
                    onChange={(e) => setEndpointKey(e.target.value)}
                    placeholder={
                      editingEndpointId
                        ? 'Leave blank to keep the saved key'
                        : 'sk-...'
                    }
                    className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs font-mono text-neutral-900 dark:text-neutral-100 outline-none focus:border-blue-500"
                  />
                </label>
                <div className="space-y-1 md:col-span-2">
                  <span className="text-[11px] font-semibold text-neutral-600 dark:text-neutral-400">
                    Models
                  </span>
                  <div className="flex items-center gap-2">
                    <input
                      value={endpointModelDraft}
                      onChange={(e) => setEndpointModelDraft(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') {
                          e.preventDefault();
                          addEndpointModel();
                        }
                      }}
                      placeholder="Model ID (e.g. gpt-4o-mini)"
                      className="flex-1 px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs font-mono text-neutral-900 dark:text-neutral-100 outline-none focus:border-blue-500"
                    />
                    <button
                      type="button"
                      onClick={addEndpointModel}
                      className="px-3 py-2 rounded-xl bg-neutral-900 dark:bg-neutral-100 hover:bg-neutral-800 dark:hover:bg-neutral-200 text-white dark:text-neutral-900 text-xs font-semibold shrink-0"
                    >
                      Add Model
                    </button>
                  </div>
                  <div className="flex flex-wrap gap-1.5 pt-1">
                    {endpointModelList.length === 0 ? (
                      <span className="text-[11px] text-neutral-400">
                        No models added yet.
                      </span>
                    ) : (
                      endpointModelList.map((model) => (
                        <span
                          key={model}
                          className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-neutral-100 dark:bg-neutral-800 text-[11px] font-mono text-neutral-700 dark:text-neutral-300"
                        >
                          {model}
                          <button
                            type="button"
                            onClick={() => removeEndpointModel(model)}
                            className="text-neutral-400 hover:text-rose-500"
                            title="Remove model"
                          >
                            <X className="w-3 h-3" />
                          </button>
                        </span>
                      ))
                    )}
                  </div>
                </div>
              </div>

              {endpointError && (
                <div className="p-2.5 rounded-xl bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-900/60 text-rose-700 dark:text-rose-300 text-xs flex items-center gap-2">
                  <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                  <span>{endpointError}</span>
                </div>
              )}

              <div className="flex justify-end gap-2">
                <button
                  onClick={resetEndpointForm}
                  className="px-3 py-2 rounded-xl border border-neutral-200 dark:border-neutral-700 text-xs font-medium text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                >
                  Reset
                </button>
                <button
                  onClick={handleSaveEndpoint}
                  disabled={endpointBusy}
                  className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-white text-xs font-semibold shadow-xs"
                >
                  {endpointBusy
                    ? 'Saving...'
                    : editingEndpointId
                    ? 'Save Changes'
                    : 'Add Endpoint'}
                </button>
              </div>
            </div>

            <div className="space-y-3">
              {customEndpoints.length === 0 ? (
                <div className="p-8 rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-700 text-center">
                  <Server className="w-6 h-6 text-neutral-300 dark:text-neutral-600 mx-auto mb-2" />
                  <p className="text-xs text-neutral-500 dark:text-neutral-400">
                    No additional endpoints yet. Add one above; it will appear as a separate
                    provider in the model list.
                  </p>
                </div>
              ) : (
                customEndpoints.map((c) => (
                  <div
                    key={c.provider_id}
                    className="rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-4 flex items-start justify-between gap-4"
                  >
                    <div className="min-w-0 space-y-1.5">
                      <div className="flex items-center gap-2 flex-wrap">
                        <Server className="w-4 h-4 text-blue-600 shrink-0" />
                        <span className="font-bold text-sm text-neutral-900 dark:text-white">
                          {c.name || c.provider_id}
                        </span>
                        <code className="text-[10px] px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400">
                          {c.provider_id}
                        </code>
                      </div>
                      <div className="text-[11.5px] font-mono text-neutral-500 dark:text-neutral-400 truncate">
                        {c.base_url || 'Base URL not set'}
                      </div>
                      <div className="flex flex-wrap gap-1">
                        {(c.models || []).length === 0 ? (
                          <span className="text-[11px] text-amber-600 dark:text-amber-400">
                            No models yet — edit to add some.
                          </span>
                        ) : (
                          (c.models || []).map((m) => (
                            <span
                              key={m}
                              className="text-[10.5px] font-mono px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300"
                            >
                              {m}
                            </span>
                          ))
                        )}
                      </div>
                      <div className="text-[10.5px] text-neutral-400">
                        Key: {c.masked_key}
                      </div>
                    </div>
                    <div className="flex items-center gap-1.5 shrink-0">
                      <button
                        onClick={() => handleUseEndpoint(c)}
                        className="px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-[11px] font-semibold"
                      >
                        Use
                      </button>
                      <button
                        onClick={() => handleEditEndpoint(c)}
                        className="px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 text-[11px] font-medium text-neutral-700 dark:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                      >
                        Edit
                      </button>
                      <button
                        onClick={() => handleDeleteEndpoint(c.provider_id)}
                        className="p-1.5 rounded-lg text-rose-500 hover:bg-rose-50 dark:hover:bg-rose-950/40"
                        title="Delete endpoint"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        ) : activeTab === 'providers' ? (
          <div className="flex-1 flex overflow-hidden">
            {/* Left: Providers List */}
            <div className="w-64 border-r border-neutral-200 dark:border-neutral-800 bg-neutral-50/60 dark:bg-neutral-900/30 overflow-y-auto p-3 space-y-4">
              {/* OAuth Providers */}
              <div>
                <div className="text-[10px] uppercase tracking-wider font-bold text-neutral-400 dark:text-neutral-500 px-2 mb-1.5 flex items-center justify-between">
                  <span>OAuth Authorization</span>
                  <span className="text-[9px] bg-blue-100 dark:bg-blue-950 text-blue-700 dark:text-blue-300 px-1 rounded font-mono">
                    Token
                  </span>
                </div>
                <div className="space-y-1">
                  {providers
                    .filter((p) => p.category === 'oauth')
                    .map((p) => {
                      const isConnected = !!oauthAccounts[p.id.toLowerCase()];
                      return (
                        <button
                          key={p.id}
                          onClick={() => {
                            setSelectedProviderId(p.id);
                            onSelectProvider?.(p.id);
                            if (typeof window !== 'undefined') {
                              localStorage.setItem('odon_active_provider', p.id);
                            }
                            setKeyError(null);
                          }}
                          className={`w-full text-left px-2.5 py-2 rounded-xl text-[12px] font-medium transition-all flex items-center justify-between ${selectedProviderId === p.id
                              ? 'bg-blue-600 text-white shadow-xs font-semibold'
                              : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-200/60 dark:hover:bg-neutral-800'
                            }`}
                        >
                          <div className="flex items-center gap-2 truncate">
                            <ProviderLogo providerId={p.id} size={15} colored={selectedProviderId !== p.id} />
                            <span className="truncate">{p.name.split(' (')[0]}</span>
                          </div>
                          {isConnected && (
                            <span className="w-2 h-2 rounded-full bg-emerald-400 shrink-0" title="Connected" />
                          )}
                        </button>
                      );
                    })}
                </div>
              </div>

              {/* BYOK Providers */}
              <div>
                <div className="text-[10px] uppercase tracking-wider font-bold text-neutral-400 dark:text-neutral-500 px-2 mb-1.5 flex items-center justify-between">
                  <span>BYOK (API Key)</span>
                  <span className="text-[9px] bg-neutral-200 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 px-1 rounded font-mono">
                    Secret
                  </span>
                </div>
                <div className="space-y-1">
                  {providers
                    .filter((p) => p.category === 'byok')
                    .map((p) => {
                      const hasKey = !!apiKeys[p.id];
                      return (
                        <button
                          key={p.id}
                          onClick={() => {
                            setSelectedProviderId(p.id);
                            setKeyError(null);
                          }}
                          className={`w-full text-left px-2.5 py-2 rounded-xl text-[12px] font-medium transition-all flex items-center justify-between ${selectedProviderId === p.id
                              ? 'bg-blue-600 text-white shadow-xs font-semibold'
                              : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-200/60 dark:hover:bg-neutral-800'
                            }`}
                        >
                          <div className="flex items-center gap-2 truncate">
                            <ProviderLogo providerId={p.id} size={15} colored={selectedProviderId !== p.id} />
                            <span className="truncate">{p.name.split(' (')[0]}</span>
                          </div>
                          {hasKey && (
                            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 shrink-0" title="Key Configured" />
                          )}
                        </button>
                      );
                    })}
                </div>
              </div>
            </div>

            {/* Right: Selected Provider Details */}
            {currentProvider && (
              <div className="flex-1 overflow-y-auto p-6 space-y-6">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <div className="flex items-center gap-2.5 mb-1">
                      <ProviderLogo providerId={currentProvider.id} size={22} />
                      <h4 className="text-base font-bold text-neutral-900 dark:text-white">
                        {currentProvider.name}
                      </h4>
                      <span className="text-[11px] font-semibold px-2 py-0.5 rounded-full bg-blue-100 dark:bg-blue-950 text-blue-700 dark:text-blue-300">
                        {currentProvider.auth_type === 'oauth' ? 'OAuth 2.0 Auth' : 'BYOK Gateway'}
                      </span>
                      {(activeProviderId || selectedProviderId) === currentProvider.id && (
                        <span className="text-[11px] font-semibold px-2 py-0.5 rounded-full bg-emerald-100 dark:bg-emerald-950 text-emerald-700 dark:text-emerald-300 flex items-center gap-1">
                          <Check className="w-3 h-3" />
                          Active on Front Page
                        </span>
                      )}
                    </div>
                    <p className="text-[12.5px] text-neutral-500 dark:text-neutral-400">
                      {currentProvider.description}
                    </p>
                  </div>

                  {(activeProviderId || selectedProviderId) !== currentProvider.id && (
                    <button
                      onClick={() => {
                        setSelectedProviderId(currentProvider.id);
                        onSelectProvider?.(currentProvider.id);
                        if (typeof window !== 'undefined') {
                          localStorage.setItem('odon_active_provider', currentProvider.id);
                        }
                        const rec = currentProvider.models.find(m => m.recommended) || currentProvider.models[0];
                        if (rec) {
                          onSelectModel?.(rec.id);
                          if (typeof window !== 'undefined') {
                            localStorage.setItem('odon_selected_model', rec.id);
                          }
                        }
                      }}
                      className="px-3 py-1.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold shadow-xs transition-colors shrink-0 cursor-pointer"
                    >
                      Set as Active Provider
                    </button>
                  )}
                </div>

                {/* Error Banner */}
                {keyError && (
                  <div className="p-3 rounded-xl bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-800/60 flex items-center gap-2 text-rose-700 dark:text-rose-300 text-xs">
                    <AlertCircle className="w-4 h-4 shrink-0" />
                    <span>{keyError}</span>
                  </div>
                )}

                {/* Connection Test Result */}
                {testResult[currentProvider.id] && (
                  <div
                    className={`p-3 rounded-xl border flex items-start gap-2 text-xs ${
                      testResult[currentProvider.id].ok
                        ? 'bg-emerald-50 dark:bg-emerald-950/40 border-emerald-200 dark:border-emerald-800/60 text-emerald-700 dark:text-emerald-300'
                        : 'bg-rose-50 dark:bg-rose-950/40 border-rose-200 dark:border-rose-800/60 text-rose-700 dark:text-rose-300'
                    }`}
                  >
                    {testResult[currentProvider.id].ok ? (
                      <Check className="w-4 h-4 shrink-0 mt-0.5" />
                    ) : (
                      <AlertCircle className="w-4 h-4 shrink-0 mt-0.5" />
                    )}
                    <span className="break-words">{testResult[currentProvider.id].text}</span>
                  </div>
                )}

                {/* OAuth Flow Card (Termigo Style) */}
                {currentProvider.auth_type === 'oauth' ? (
                  <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-850/50 space-y-4">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <ProviderLogo providerId={currentProvider.id} size={16} />
                        <span className="font-semibold text-xs text-neutral-800 dark:text-neutral-200">
                          {currentProvider.name.split(' (')[0]} Account
                        </span>
                      </div>

                      {currentOAuth ? (
                        <span className="flex items-center gap-1 text-[11px] font-medium text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-950/60 px-2.5 py-0.5 rounded-full border border-emerald-200 dark:border-emerald-800">
                          <Check className="w-3 h-3 text-emerald-500" />
                          Connected
                        </span>
                      ) : (
                        <span className="text-[11px] text-neutral-500 bg-neutral-100 dark:bg-neutral-800 px-2 py-0.5 rounded-full">
                          Disconnected
                        </span>
                      )}
                    </div>

                    {currentOAuth ? (
                      /* Connected state */
                      <div className="space-y-3 pt-1">
                        <div className="flex items-center justify-between p-3 rounded-xl bg-white dark:bg-neutral-800 border border-neutral-200/80 dark:border-neutral-700/80">
                          <div>
                            <div className="font-semibold text-xs text-neutral-900 dark:text-white">
                              {currentOAuth.email || 'developer@odon.local'}
                            </div>
                            <div className="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5 flex items-center gap-2">
                              <span>Plan: <strong className="text-neutral-700 dark:text-neutral-300">{currentOAuth.plan || 'Standard'}</strong></span>
                              <span>·</span>
                              <span className="text-emerald-600 dark:text-emerald-400">Tokens Active</span>
                            </div>
                          </div>

                          <button
                            onClick={() => handleOAuthDisconnect(currentProvider.id)}
                            disabled={oauthBusy === currentProvider.id}
                            className="flex items-center gap-1 px-3 py-1.5 rounded-lg text-xs font-medium text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/40 border border-rose-200 dark:border-rose-900/50 transition-colors"
                          >
                            <LogOut className="w-3.5 h-3.5" />
                            <span>Disconnect</span>
                          </button>
                        </div>
                      </div>
                    ) : (
                      /* Disconnected state */
                      <div className="space-y-3">
                        {oauthPrompt && oauthPrompt.provider === currentProvider.id ? (
                          /* Device Code prompt (GitHub Copilot, xAI, Muse) */
                          <div className="p-4 rounded-xl bg-blue-50/70 dark:bg-blue-950/30 border border-blue-200 dark:border-blue-800 space-y-3">
                            <div className="text-xs font-semibold text-blue-900 dark:text-blue-200">
                              Device Authorization Required
                            </div>
                            <p className="text-[11.5px] text-neutral-600 dark:text-neutral-400 leading-relaxed">
                              Copy the one-time user code below and open the authorization page:
                            </p>

                            {oauthPrompt.userCode && (
                              <div className="flex items-center gap-2">
                                <div className="px-4 py-2 rounded-xl bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 font-mono text-base font-bold tracking-widest text-neutral-900 dark:text-white">
                                  {oauthPrompt.userCode}
                                </div>
                                <button
                                  onClick={() => handleCopyCode(oauthPrompt.userCode!)}
                                  className="flex items-center gap-1 px-3 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold transition-colors shadow-2xs"
                                >
                                  {copiedCode ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                                  <span>{copiedCode ? 'Copied!' : 'Copy Code'}</span>
                                </button>
                              </div>
                            )}

                            {oauthPrompt.verificationUri && (
                              <a
                                href={oauthPrompt.verificationUri}
                                target="_blank"
                                rel="noreferrer"
                                className="inline-flex items-center gap-1.5 text-xs font-semibold text-blue-600 dark:text-blue-400 hover:underline pt-1"
                              >
                                <span>Open {oauthPrompt.verificationUri}</span>
                                <ExternalLink className="w-3.5 h-3.5" />
                              </a>
                            )}
                          </div>
                        ) : (
                          <div className="flex items-center justify-between">
                            <p className="text-xs text-neutral-500">
                              Sign in with your official account to authenticate model requests directly.
                            </p>
                            <button
                              onClick={() => handleOAuthSignIn(currentProvider.id)}
                              disabled={oauthBusy === currentProvider.id}
                              className="flex items-center gap-2 px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold transition-colors shadow-xs"
                            >
                              <ProviderLogo providerId={currentProvider.id} size={15} colored={false} />
                              <span>{oauthBusy === currentProvider.id ? 'Connecting...' : `Sign in with ${currentProvider.name.split(' (')[0]}`}</span>
                            </button>
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                ) : (
                  /* BYOK Flow Card (Termigo Style) */
                  <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-850/50 space-y-4">
                    {/* API Key */}
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between">
                        <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 flex items-center gap-1.5">
                          <Key className="w-3.5 h-3.5 text-neutral-400" />
                          API Key (Secret)
                        </label>
                        {KEY_PREFIXES[currentProvider.id] && (
                          <span className="text-[10.5px] text-neutral-400 font-mono">
                            Prefix: {KEY_PREFIXES[currentProvider.id]}...
                          </span>
                        )}
                      </div>

                      <div className="flex items-center gap-2">
                        <div className="relative flex-1">
                          <input
                            type={showKey[currentProvider.id] ? 'text' : 'password'}
                            value={apiKeys[currentProvider.id] || ''}
                            onChange={(e) =>
                              setApiKeys({ ...apiKeys, [currentProvider.id]: e.target.value })
                            }
                            placeholder={
                              configuredKeys[currentProvider.id] && !apiKeys[currentProvider.id]
                                ? `Saved key: ${configuredKeys[currentProvider.id]} — paste a new key to replace`
                                : `Paste your ${currentProvider.name.split(' (')[0]} API key...`
                            }
                            className="w-full px-3.5 py-2.5 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500 pr-10"
                          />
                          <button
                            type="button"
                            onClick={() =>
                              setShowKey({
                                ...showKey,
                                [currentProvider.id]: !showKey[currentProvider.id],
                              })
                            }
                            className="absolute right-3 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200"
                          >
                            {showKey[currentProvider.id] ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                          </button>
                        </div>

                        <button
                          onClick={() => handleTestConnection(currentProvider.id)}
                          disabled={testing === currentProvider.id}
                          className="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-xl border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-700 dark:text-neutral-200 text-xs font-semibold transition-colors shrink-0 disabled:opacity-50"
                          title="Send a tiny request to verify the key, Base URL, and model"
                        >
                          {testing === currentProvider.id ? (
                            <Loader2 className="w-3.5 h-3.5 animate-spin" />
                          ) : (
                            <Zap className="w-3.5 h-3.5" />
                          )}
                          Test
                        </button>

                        <button
                          onClick={() => handleSaveKey(currentProvider.id)}
                          className="px-4 py-2.5 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold transition-colors shrink-0"
                        >
                          Save Key
                        </button>

                        {apiKeys[currentProvider.id] && (
                          <button
                            onClick={() => handleClearKey(currentProvider.id)}
                            className="px-3 py-2.5 rounded-xl border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-500 text-xs font-medium transition-colors shrink-0"
                            title="Clear saved key"
                          >
                            Clear
                          </button>
                        )}
                      </div>
                    </div>

                    {/* Endpoint Base URL */}
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between">
                        <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 flex items-center gap-1.5">
                          <Globe className="w-3.5 h-3.5 text-neutral-400" />
                          Endpoint Base URL
                        </label>
                        <button
                          onClick={() =>
                            setBaseUrls({
                              ...baseUrls,
                              [currentProvider.id]: DEFAULT_BASE_URLS[currentProvider.id] || '',
                            })
                          }
                          className="text-[10.5px] text-blue-600 dark:text-blue-400 hover:underline"
                        >
                          Reset Default
                        </button>
                      </div>
                      <input
                        type="text"
                        value={baseUrls[currentProvider.id] || currentProvider.default_base_url || ''}
                        onChange={(e) =>
                          setBaseUrls({ ...baseUrls, [currentProvider.id]: e.target.value })
                        }
                        placeholder={DEFAULT_BASE_URLS[currentProvider.id] || 'https://...'}
                        className="w-full px-3.5 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500"
                      />
                    </div>
                  </div>
                )}

                {/* Available Models in this provider */}
                {currentProvider.id === 'openai-compatible' ? (
                  <div className="space-y-4">
                    <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-850 space-y-3">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold uppercase tracking-wider text-neutral-800 dark:text-neutral-200">
                          Add Custom Model (OpenAI-Compatible)
                        </span>
                        <span className="text-[11px] text-neutral-400">vLLM, Ollama, LM Studio, LiteLLM</span>
                      </div>

                      <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
                        <div className="sm:col-span-1">
                          <label className="text-[11px] text-neutral-500 font-mono mb-1 block">Model ID *</label>
                          <input
                            type="text"
                            value={newModelId}
                            onChange={(e) => setNewModelId(e.target.value)}
                            placeholder="e.g. deepseek-r1:32b"
                            className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs font-mono outline-none focus:border-blue-500"
                          />
                        </div>
                        <div className="sm:col-span-1">
                          <label className="text-[11px] text-neutral-500 font-mono mb-1 block">Display Name</label>
                          <input
                            type="text"
                            value={newModelName}
                            onChange={(e) => setNewModelName(e.target.value)}
                            placeholder="e.g. Local DeepSeek R1"
                            className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs outline-none focus:border-blue-500"
                          />
                        </div>
                        <div className="sm:col-span-1">
                          <label className="text-[11px] text-neutral-500 font-mono mb-1 block">Context Window</label>
                          <input
                            type="number"
                            value={newModelContext}
                            onChange={(e) => setNewModelContext(Number(e.target.value))}
                            placeholder="128000"
                            className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs font-mono outline-none focus:border-blue-500"
                          />
                        </div>
                      </div>

                      <button
                        type="button"
                        onClick={handleAddCustomModel}
                        disabled={!newModelId.trim()}
                        className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium text-white bg-blue-600 hover:bg-blue-700 disabled:opacity-40 transition-colors cursor-pointer"
                      >
                        <Plus className="w-3.5 h-3.5" />
                        <span>Add Model Row</span>
                      </button>
                    </div>

                    {/* Table of custom models */}
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                          Configured Models ({customOpenAiModels.length})
                        </span>
                        <span className="text-[11px] text-neutral-400">Click a model to make it active</span>
                      </div>

                      {customOpenAiModels.length === 0 ? (
                        <div className="p-4 rounded-xl border border-dashed border-neutral-300 dark:border-neutral-700 text-center text-xs text-neutral-400">
                          No custom models yet. Enter a Model ID above and click Add Model Row.
                        </div>
                      ) : (
                        <div className="grid grid-cols-1 gap-2">
                          {customOpenAiModels.map((m) => {
                            const isSelected = currentModel === m.id || currentModel === `openai-compatible::${m.id}`;
                            return (
                              <div
                                key={m.id}
                                className={`p-3 rounded-xl border transition-all flex items-center justify-between ${
                                  isSelected
                                    ? 'border-blue-500 bg-blue-50/40 dark:bg-blue-950/30'
                                    : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-850'
                                }`}
                              >
                                <div>
                                  <div className="flex items-center gap-2">
                                    <span className="font-semibold text-xs text-neutral-900 dark:text-white">
                                      {m.name}
                                    </span>
                                  </div>
                                  <div className="text-[11px] text-neutral-400 font-mono mt-0.5 flex items-center gap-2">
                                    <span>id: <strong className="text-neutral-600 dark:text-neutral-300">{m.id}</strong></span>
                                    <span>·</span>
                                    <span>{(m.context / 1000).toFixed(0)}k context</span>
                                  </div>
                                </div>

                                <div className="flex items-center gap-2">
                                  {isSelected ? (
                                    <span className="text-xs font-semibold text-blue-600 dark:text-blue-400 flex items-center gap-1">
                                      <Check className="w-3.5 h-3.5" />
                                      Active
                                    </span>
                                  ) : (
                                    <button
                                      type="button"
                                      onClick={() => {
                                        setSelectedProviderId('openai-compatible');
                                        onSelectProvider?.('openai-compatible');
                                        if (typeof window !== 'undefined') {
                                          localStorage.setItem('odon_active_provider', 'openai-compatible');
                                          localStorage.setItem('odon_selected_model', m.id);
                                        }
                                        onSelectModel?.(m.id);
                                      }}
                                      className="px-2.5 py-1 rounded-lg text-xs font-medium text-neutral-600 dark:text-neutral-300 bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 transition-colors cursor-pointer"
                                    >
                                      Use Model
                                    </button>
                                  )}
                                  <button
                                    type="button"
                                    onClick={() => handleDeleteCustomModel(m.id)}
                                    className="p-1 rounded-lg text-neutral-400 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-950/40 transition-colors cursor-pointer"
                                    title="Delete this model row"
                                  >
                                    <Trash2 className="w-3.5 h-3.5" />
                                  </button>
                                </div>
                              </div>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  </div>
                ) : (
                  <div className="space-y-2.5">
                    <div className="flex items-center justify-between">
                      <span className="text-xs font-bold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                        Official Provider Models ({currentProvider.models.length})
                      </span>
                      <span className="text-[11px] text-neutral-400">Click to set as active session model</span>
                    </div>

                    <div className="grid grid-cols-1 gap-2">
                      {currentProvider.models.map((m) => {
                        const wireId = `${currentProvider.id}::${m.id}`;
                        const isSelected = currentModel === m.id || currentModel === wireId || currentModel === m.name;
                        return (
                          <div
                            key={m.id}
                            onClick={() => {
                              setSelectedProviderId(currentProvider.id);
                              onSelectProvider?.(currentProvider.id);
                              if (typeof window !== 'undefined') {
                                localStorage.setItem('odon_active_provider', currentProvider.id);
                                localStorage.setItem('odon_selected_model', m.id);
                              }
                              onSelectModel?.(m.id);
                            }}
                            className={`p-3 rounded-xl border transition-all cursor-pointer flex items-center justify-between ${
                              isSelected
                                ? 'border-blue-500 bg-blue-50/40 dark:bg-blue-950/30'
                                : 'border-neutral-200 dark:border-neutral-800 hover:border-neutral-300 dark:hover:border-neutral-700 bg-white dark:bg-neutral-850'
                            }`}
                          >
                            <div>
                              <div className="flex items-center gap-2">
                                <span className="font-semibold text-xs text-neutral-900 dark:text-white">
                                  {m.name}
                                </span>
                                {m.recommended && (
                                  <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-emerald-100 dark:bg-emerald-950 text-emerald-700 dark:text-emerald-300">
                                    Recommended
                                  </span>
                                )}
                              </div>
                              <div className="text-[11px] text-neutral-400 font-mono mt-0.5 flex items-center gap-2">
                                <span>wire: <strong className="text-neutral-600 dark:text-neutral-300">{m.id}</strong></span>
                                <span>·</span>
                                <span>{(m.context / 1000).toFixed(0)}k context</span>
                              </div>
                            </div>

                            {isSelected ? (
                              <span className="text-xs font-semibold text-blue-600 dark:text-blue-400 flex items-center gap-1">
                                <Check className="w-3.5 h-3.5" />
                                Active
                              </span>
                            ) : (
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  setSelectedProviderId(currentProvider.id);
                                  onSelectProvider?.(currentProvider.id);
                                  if (typeof window !== 'undefined') {
                                    localStorage.setItem('odon_active_provider', currentProvider.id);
                                    localStorage.setItem('odon_selected_model', m.id);
                                  }
                                  onSelectModel?.(m.id);
                                }}
                                className="px-2.5 py-1 rounded-lg text-xs font-medium text-neutral-600 dark:text-neutral-300 bg-neutral-100 dark:bg-neutral-800 hover:bg-neutral-200 dark:hover:bg-neutral-700 transition-colors"
                              >
                                Use Model
                              </button>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        ) : activeTab === 'agents' ? (
          /* Agents & Personas Tab */
          <div className="flex-1 overflow-y-auto p-6 space-y-6 text-[13px]">
            <div className="space-y-3">
              <div>
                <span className="font-bold text-sm text-neutral-900 dark:text-white block">
                  Autonomous Agent Personas
                </span>
                <span className="text-xs text-neutral-500">
                  Select the active behavioral specialization for the AI engineer.
                </span>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {[
                  {
                    id: 'coder',
                    name: 'Autonomous Coder',
                    badge: 'Fullstack Dev',
                    desc: 'Decisive pair-programming, fast surgical edits, right-sized code without over-engineering.',
                    icon: FileCode,
                  },
                  {
                    id: 'auditor',
                    name: 'Security Auditor',
                    badge: 'Code Review & Audit',
                    desc: 'Deep multi-layer static analysis, traces caller graphs, validates invariants & race conditions.',
                    icon: ShieldAlert,
                  },
                  {
                    id: 'pentest',
                    name: 'Pentester & RE',
                    badge: 'Offensive Security',
                    desc: 'Authorized security auditing, port scanning, web application recon, and vulnerability verification.',
                    icon: Wifi,
                  },
                  {
                    id: 'architect',
                    name: 'Systems Architect',
                    badge: 'RFC & Architecture',
                    desc: 'High-level systems design, PRDs, component interaction flows, and database schemas.',
                    icon: Layers,
                  },
                  {
                    id: 'sentinel',
                    name: 'Autonomous Sentinel',
                    badge: 'DevOps & Watchdog',
                    desc: 'Continuous background monitoring, automated health probing, log analysis, and auto-healing.',
                    icon: Server,
                  },
                ].map((persona) => {
                  const isSelected = activePersona === persona.id;
                  const Icon = persona.icon;
                  return (
                    <div
                      key={persona.id}
                      onClick={() => {
                        setActivePersona(persona.id);
                        if (typeof window !== 'undefined') {
                          localStorage.setItem('odon_active_persona', persona.id);
                        }
                      }}
                      className={`p-3.5 rounded-xl border transition-all cursor-pointer space-y-1.5 ${
                        isSelected
                          ? 'border-blue-500 bg-blue-50/50 dark:bg-blue-950/30'
                          : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-850 hover:border-neutral-300'
                      }`}
                    >
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          <Icon className={`w-4 h-4 ${isSelected ? 'text-blue-600' : 'text-neutral-500'}`} />
                          <span className="font-semibold text-xs text-neutral-900 dark:text-white">
                            {persona.name}
                          </span>
                        </div>
                        <span className="text-[10px] px-2 py-0.5 rounded-full font-medium bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400">
                          {persona.badge}
                        </span>
                      </div>
                      <p className="text-[11.5px] text-neutral-500 leading-snug">{persona.desc}</p>
                    </div>
                  );
                })}
              </div>
            </div>

            {/* Custom System Prompt */}
            <div className="space-y-2">
              <label className="font-semibold text-neutral-800 dark:text-neutral-200 block">
                Custom User Instructions & Guardrails
              </label>
              <textarea
                value={customInstructions}
                onChange={(e) => {
                  setCustomInstructions(e.target.value);
                  if (typeof window !== 'undefined') {
                    localStorage.setItem('odon_custom_instructions', e.target.value);
                  }
                }}
                rows={4}
                placeholder="e.g. I am a fullstack developer and cybersecurity specialist with a focus on clean design..."
                className="w-full p-3 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800/80 text-xs text-neutral-800 dark:text-neutral-200 outline-none focus:border-blue-500 font-mono"
              />
              <p className="text-[11.5px] text-neutral-500">
                These instructions are injected into context on every agent interaction.
              </p>
            </div>

            {/* Effort Level */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 flex items-center justify-between">
              <div>
                <span className="font-semibold text-neutral-800 dark:text-neutral-200 block">
                  Thinking Reasoning Effort Level
                </span>
                <span className="text-xs text-neutral-500">
                  Controls depth of scratchpad reasoning tokens before execution.
                </span>
              </div>
              <div className="flex gap-1.5 bg-neutral-100 dark:bg-neutral-800 p-1 rounded-lg text-xs font-semibold">
                {['low', 'medium', 'high'].map((lvl) => (
                  <button
                    key={lvl}
                    onClick={() => {
                      setEffortLevel(lvl);
                      if (typeof window !== 'undefined') {
                        localStorage.setItem('odon_effort_level', lvl);
                      }
                    }}
                    className={`px-3 py-1 rounded-md uppercase tracking-wider text-[11px] transition-all ${
                      effortLevel === lvl
                        ? 'bg-blue-600 text-white shadow-xs'
                        : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                    }`}
                  >
                    {lvl}
                  </button>
                ))}
              </div>
            </div>
          </div>
        ) : activeTab === 'skills' ? (
          /* Skills System Tab */
          <div className="flex-1 overflow-y-auto p-6 space-y-5 text-[13px]">
            <div className="flex items-center justify-between">
              <div>
                <span className="font-bold text-sm text-neutral-900 dark:text-white block">
                  Autonomous Skills (.odon/skills)
                </span>
                <span className="text-xs text-neutral-500">
                  Reusable markdown procedures and automated routines callable by the agent.
                </span>
              </div>
              <button
                onClick={() => setIsSkillFormOpen(!isSkillFormOpen)}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 cursor-pointer shadow-xs"
              >
                <Plus className="w-3.5 h-3.5" />
                <span>New Skill</span>
              </button>
            </div>

            {/* Create Skill Form */}
            {isSkillFormOpen && (
              <div className="p-4 rounded-xl border border-blue-200 dark:border-blue-900 bg-blue-50/40 dark:bg-blue-950/30 space-y-3">
                <div className="flex items-center justify-between">
                  <span className="font-semibold text-xs text-neutral-900 dark:text-white">Record New Skill</span>
                  <button onClick={() => setIsSkillFormOpen(false)} className="text-neutral-400 hover:text-neutral-700">
                    <X className="w-3.5 h-3.5" />
                  </button>
                </div>
                <input
                  type="text"
                  value={newSkillName}
                  onChange={(e) => setNewSkillName(e.target.value)}
                  placeholder="Skill name (e.g. pentest-recon, deploy-vps)"
                  className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 text-xs font-mono outline-none focus:border-blue-500 bg-white dark:bg-neutral-800"
                />
                <textarea
                  value={newSkillContent}
                  onChange={(e) => setNewSkillContent(e.target.value)}
                  rows={4}
                  placeholder="# Pentest Recon Routine&#10;1. Scan target domain ports&#10;2. Check HTTP security headers&#10;3. Report findings"
                  className="w-full p-3 rounded-lg border border-neutral-300 dark:border-neutral-700 text-xs font-mono outline-none focus:border-blue-500 bg-white dark:bg-neutral-800"
                />
                <div className="flex justify-end gap-2">
                  <button
                    onClick={async () => {
                      if (!newSkillName.trim() || !newSkillContent.trim()) return;
                      await api.saveSkill(newSkillName.trim(), newSkillContent);
                      const res = await api.getSkills();
                      setSkillsList(res.skills || []);
                      setNewSkillName('');
                      setNewSkillContent('');
                      setIsSkillFormOpen(false);
                    }}
                    disabled={!newSkillName.trim()}
                    className="px-3.5 py-1.5 rounded-lg text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 disabled:opacity-40 shadow-xs"
                  >
                    Save Skill
                  </button>
                </div>
              </div>
            )}

            {/* Skills List */}
            {skillsList.length === 0 ? (
              <div className="p-8 rounded-xl border border-dashed border-neutral-300 dark:border-neutral-700 text-center text-xs text-neutral-400">
                No custom skills in `.odon/skills` yet. Click New Skill above to create an automation playbook.
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-2.5">
                {skillsList.map((skill) => (
                  <div
                    key={skill.name}
                    className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-850 flex items-center justify-between"
                  >
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <Sparkles className="w-4 h-4 text-amber-500" />
                        <span className="font-semibold text-xs text-neutral-900 dark:text-white">
                          {skill.title}
                        </span>
                        <span className="text-[10px] font-mono bg-neutral-100 dark:bg-neutral-800 px-1.5 py-0.5 rounded text-neutral-600">
                          {skill.name}.md
                        </span>
                      </div>
                      <p className="text-[11.5px] text-neutral-500">{skill.description || 'Autonomous multi-step procedure.'}</p>
                    </div>

                    <button
                      onClick={async () => {
                        if (!confirm(`Delete skill ${skill.name}?`)) return;
                        await api.deleteSkill(skill.name);
                        const res = await api.getSkills();
                        setSkillsList(res.skills || []);
                      }}
                      className="p-1.5 rounded-lg text-neutral-400 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-950/40 cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        ) : activeTab === 'mcp' ? (
          /* MCP Servers Tab */
          <div className="flex-1 overflow-y-auto p-6 space-y-5 text-[13px]">
            <div>
              <span className="font-bold text-sm text-neutral-900 dark:text-white block">
                Model Context Protocol (MCP) Servers
              </span>
              <span className="text-xs text-neutral-500">
                Register external MCP servers (stdio or SSE). Execution requires the MCP bridge, which is not enabled in this build.
              </span>
            </div>

            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-850/50 space-y-3">
              <span className="font-semibold text-xs text-neutral-800 dark:text-neutral-200 block">
                Add MCP Server Connection
              </span>
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
                <div>
                  <label className="text-[11px] text-neutral-500 font-mono mb-1 block">Server ID</label>
                  <input
                    type="text"
                    value={newMcpName}
                    onChange={(e) => setNewMcpName(e.target.value)}
                    placeholder="github-mcp, filesystem-mcp"
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 text-xs font-mono outline-none bg-white dark:bg-neutral-800"
                  />
                </div>
                <div>
                  <label className="text-[11px] text-neutral-500 font-mono mb-1 block">Transport</label>
                  <select
                    value={newMcpTransport}
                    onChange={(e) => setNewMcpTransport(e.target.value)}
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 text-xs outline-none bg-white dark:bg-neutral-800"
                  >
                    <option value="stdio">stdio (Local Command)</option>
                    <option value="sse">sse (HTTP Server-Sent Events)</option>
                  </select>
                </div>
                <div>
                  <label className="text-[11px] text-neutral-500 font-mono mb-1 block">Command / Endpoint</label>
                  <input
                    type="text"
                    value={newMcpCommand}
                    onChange={(e) => setNewMcpCommand(e.target.value)}
                    placeholder="npx -y @modelcontextprotocol/server-..."
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 text-xs font-mono outline-none bg-white dark:bg-neutral-800"
                  />
                </div>
              </div>
              <button
                onClick={async () => {
                  if (!newMcpName.trim() || !newMcpCommand.trim()) return;
                  const name = newMcpName.trim();
                  const id = name.toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '');
                  const parts = newMcpCommand.trim().split(/\s+/);
                  await api.saveMcpServer({
                    id: id || `mcp-${Date.now()}`,
                    name,
                    transport: newMcpTransport,
                    command: newMcpTransport === 'stdio' ? parts[0] : undefined,
                    args: newMcpTransport === 'stdio' ? parts.slice(1) : undefined,
                    url: newMcpTransport === 'sse' ? newMcpCommand.trim() : undefined,
                    enabled: true,
                    tools: [],
                  });
                  const servers = await api.getMcpServers();
                  setMcpServers(servers || []);
                  setNewMcpName('');
                  setNewMcpCommand('');
                }}
                disabled={!newMcpName.trim() || !newMcpCommand.trim()}
                className="px-3.5 py-1.5 rounded-lg text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 disabled:opacity-40 cursor-pointer shadow-xs"
              >
                Register MCP Server
              </button>
            </div>

            {/* List MCP Servers */}
            <div className="space-y-2">
              <span className="text-xs font-bold uppercase tracking-wider text-neutral-500">
                Configured MCP Servers ({mcpServers.length})
              </span>
              {mcpServers.length === 0 ? (
                <div className="p-6 rounded-xl border border-dashed border-neutral-300 dark:border-neutral-700 text-center text-xs text-neutral-400">
                  No custom MCP servers configured. Built-in tools are active.
                </div>
              ) : (
                mcpServers.map((s, idx) => (
                  <div key={idx} className="p-3.5 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-850 flex items-center justify-between">
                    <div>
                      <div className="font-semibold text-xs text-neutral-900 dark:text-white">{s.name}</div>
                      <div className="text-[11px] text-neutral-400 font-mono">
                        {s.transport === 'sse' ? s.url : [s.command, ...(s.args || [])].filter(Boolean).join(' ')}
                      </div>
                    </div>
                    <span className="text-[10px] px-2 py-0.5 rounded-full bg-neutral-100 text-neutral-600 font-semibold">
                      Configured
                    </span>
                  </div>
                ))
              )}
            </div>
          </div>
        ) : activeTab === 'harness' ? (
          /* Mode Harness & Security Tab */
          <div className="flex-1 overflow-y-auto p-6 space-y-5 text-[13px]">
            <div>
              <span className="font-bold text-sm text-neutral-900 dark:text-white block">
                Mode Harness & Execution Safety Policy
              </span>
              <span className="text-xs text-neutral-500">
                Configure agent autonomy, plan mode, and approval gates for dangerous operations.
              </span>
            </div>

            {/* Execution Autonomy */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-850/50 space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <span className="font-semibold text-xs text-neutral-900 dark:text-white block">
                    Execution Mode: {executionMode}
                  </span>
                  <span className="text-xs text-neutral-500">
                    Autonomous mode automatically executes safe tools; Supervised pauses for confirmation.
                  </span>
                </div>
                <div className="flex gap-1.5 bg-neutral-200 dark:bg-neutral-800 p-1 rounded-lg text-xs font-semibold">
                  <button
                    onClick={async () => {
                      setExecutionMode('Autonomous');
                      await api.updateSettings({ mode: 'Autonomous' });
                    }}
                    className={`px-3 py-1 rounded-md transition-all ${
                      executionMode === 'Autonomous' ? 'bg-white text-blue-600 shadow-xs' : 'text-neutral-500'
                    }`}
                  >
                    Autonomous
                  </button>
                  <button
                    onClick={async () => {
                      setExecutionMode('Supervised');
                      await api.updateSettings({ mode: 'Supervised' });
                    }}
                    className={`px-3 py-1 rounded-md transition-all ${
                      executionMode === 'Supervised' ? 'bg-white text-blue-600 shadow-xs' : 'text-neutral-500'
                    }`}
                  >
                    Supervised
                  </button>
                </div>
              </div>
            </div>

            {/* Plan Mode Toggle */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 flex items-center justify-between">
              <div>
                <span className="font-semibold text-xs text-neutral-900 dark:text-white block">
                  Enforce Plan Mode
                </span>
                <span className="text-xs text-neutral-500">
                  Agent must formulate a structured plan (todo_write) before executing any mutating tool.
                </span>
              </div>
              <button
                onClick={() => {
                  const next = !planMode;
                  setPlanMode(next);
                  if (typeof window !== 'undefined') {
                    localStorage.setItem('odon_plan_mode', String(next));
                  }
                }}
                className={`px-3.5 py-1.5 rounded-xl text-xs font-semibold transition-colors ${
                  planMode ? 'bg-blue-600 text-white' : 'bg-neutral-100 dark:bg-neutral-800 text-neutral-600'
                }`}
              >
                {planMode ? 'Plan Mode ON' : 'Plan Mode OFF'}
              </button>
            </div>

            {/* Stored Approval Policies */}
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-bold uppercase tracking-wider text-neutral-500">
                  Always-Allowed Permissions ({policies.length})
                </span>
                {policies.length > 0 && (
                  <button
                    onClick={async () => {
                      for (const p of policies) {
                        await api.deleteApprovalPolicy(p);
                      }
                      setPolicies([]);
                    }}
                    className="text-xs text-red-600 hover:underline cursor-pointer"
                  >
                    Revoke All
                  </button>
                )}
              </div>

              {policies.length === 0 ? (
                <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 text-center text-xs text-neutral-400">
                  No permanently remembered approval decisions yet. Click "Always Allow" on any approval prompt to whitelist.
                </div>
              ) : (
                <div className="space-y-1.5">
                  {policies.map((p) => (
                    <div
                      key={p}
                      className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 flex items-center justify-between text-xs font-mono"
                    >
                      <span className="truncate">{p}</span>
                      <button
                        onClick={async () => {
                          await api.deleteApprovalPolicy(p);
                          setPolicies((prev) => prev.filter((item) => item !== p));
                        }}
                        className="text-neutral-400 hover:text-red-600 p-1 cursor-pointer"
                      >
                        <Trash2 className="w-3 h-3" />
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        ) : activeTab === 'ssh' ? (
          /* SSH Remote Profiles Tab */
          <div className="flex-1 overflow-y-auto p-6 space-y-5 text-[13px]">
            <div>
              <span className="font-bold text-sm text-neutral-900 dark:text-white block">
                Saved Remote SSH Host Profiles
              </span>
              <span className="text-xs text-neutral-500">
                Manage pre-configured SSH targets for one-click connection and remote agent operations.
              </span>
            </div>

            <div className="grid grid-cols-1 gap-3">
              {sshProfiles.map((p) => (
                <div
                  key={p.id}
                  className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-850 flex items-center justify-between"
                >
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <Server className="w-4 h-4 text-neutral-700 dark:text-neutral-300" />
                      <span className="font-semibold text-xs text-neutral-900 dark:text-white">{p.alias}</span>
                      <span className="text-[11px] font-mono px-2 py-0.5 rounded-md bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 border border-neutral-200 dark:border-neutral-700">
                        {p.user}@{p.host}:{p.port}
                      </span>
                    </div>
                    <p className="text-xs text-neutral-500">{p.description || 'Remote Linux workstation/server.'}</p>
                    {p.key_path && (
                      <div className="text-[11px] text-neutral-400 font-mono flex items-center gap-1">
                        <Key className="w-3 h-3" />
                        <span>{p.key_path}</span>
                      </div>
                    )}
                  </div>

                  <div className="flex items-center gap-2">
                    <button
                      onClick={async () => {
                        if (!confirm(`Delete profile ${p.alias}?`)) return;
                        const res = await api.deleteSshProfile(p.id);
                        setSshProfiles(res.profiles || []);
                      }}
                      className="p-1.5 rounded-lg text-neutral-400 hover:text-red-600 hover:bg-red-50 cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        ) : activeTab === 'relays' ? (
          <RelaysPanel />
        ) : (
          /* System & Daemon Tab */
          <div className="flex-1 overflow-y-auto p-6 space-y-6 text-[13px]">
            {/* Daemon Status Card */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-850/50 space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Server className="w-4 h-4 text-blue-600 dark:text-blue-400" />
                  <span className="font-semibold text-neutral-800 dark:text-neutral-200">
                    ODON Daemon Engine Status
                  </span>
                </div>
                <div className="flex items-center gap-2">
                  <span
                    className={`flex items-center gap-1.5 text-xs font-semibold px-2.5 py-1 rounded-full ${daemonConnected
                        ? 'bg-emerald-100 dark:bg-emerald-950 text-emerald-700 dark:text-emerald-300'
                        : 'bg-amber-100 dark:bg-amber-950 text-amber-700 dark:text-amber-300'
                      }`}
                  >
                    <span className={`w-1.5 h-1.5 rounded-full ${daemonConnected ? 'bg-emerald-500' : 'bg-amber-500 animate-pulse'}`} />
                    {daemonConnected ? 'Online (Listening on port 4096)' : 'Offline / Connecting'}
                  </span>
                  <button
                    onClick={() => {
                      api.getStatus()
                        .then(() => setDaemonConnected(true))
                        .catch(() => setDaemonConnected(false));
                    }}
                    className="p-1 rounded-md text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 hover:bg-neutral-200 dark:hover:bg-neutral-700"
                    title="Probe daemon status"
                  >
                    <RotateCcw className="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>
              <p className="text-[12px] text-neutral-500">
                The headless daemon (`odond serve`) provides SQLite state persistence, interactive PowerShell sessions, agent tool dispatch, and live SSE event distribution.
              </p>
            </div>

            {/* Daemon URL override */}
            <div className="space-y-1.5">
              <label className="flex items-center gap-1.5 font-semibold text-neutral-800 dark:text-neutral-200">
                <Globe className="w-4 h-4 text-neutral-500" />
                Daemon Connection URL
              </label>
              <input
                type="text"
                value={daemonUrl}
                onChange={(e) => setDaemonUrl(e.target.value)}
                placeholder="http://127.0.0.1:4096"
                className="w-full px-3.5 py-2.5 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800/80 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500"
              />
              <p className="text-[11.5px] text-neutral-500">
                Leave empty or use `http://127.0.0.1:4096` for standard local workspace integration.
              </p>
            </div>

            {/* API access token */}
            <div className="space-y-1.5">
              <label className="flex items-center gap-1.5 font-semibold text-neutral-800 dark:text-neutral-200">
                <Lock className="w-4 h-4 text-neutral-500" />
                API Access Token
              </label>
              <input
                type="password"
                value={apiToken}
                onChange={(e) => setApiToken(e.target.value)}
                placeholder="Required when the daemon is started with ODON_API_TOKEN"
                className="w-full px-3.5 py-2.5 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800/80 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500"
              />
              <p className="text-[11.5px] text-neutral-500">
                Must match the daemon's <code>ODON_API_TOKEN</code>. Required for non-loopback binds.
              </p>
            </div>

            {/* Theme Toggle */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 flex items-center justify-between">
              <div>
                <span className="font-semibold text-neutral-800 dark:text-neutral-200 block">
                  Interface Theme
                </span>
                <span className="text-xs text-neutral-500">
                  Switch between clean light canvas and dark developer terminal aesthetic
                </span>
              </div>
              <button
                onClick={onToggleTheme}
                className="px-4 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-100 dark:bg-neutral-800 text-neutral-800 dark:text-neutral-200 text-xs font-semibold hover:bg-neutral-200 dark:hover:bg-neutral-700 transition-colors"
              >
                {isDark ? '🌙 Dark Mode' : '☀️ Light Mode'}
              </button>
            </div>

            {/* Security Mode */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 space-y-2">
              <div className="flex items-center gap-2">
                <Shield className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
                <span className="font-semibold text-neutral-800 dark:text-neutral-200">
                  Execution Safety Tier
                </span>
              </div>
              <p className="text-xs text-neutral-500">
                Autonomous engineer mode with interactive approval gates for destructive commands (Tier 3 risk, shell write, file deletion).
              </p>
            </div>
          </div>
        )}

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-3.5 bg-neutral-50 dark:bg-neutral-900 border-t border-neutral-200 dark:border-neutral-800 shrink-0">
          <div className="text-xs text-neutral-500">
            {activeTab === 'providers' && currentModel ? (
              <span>Active Model: <strong className="text-neutral-800 dark:text-neutral-200 font-mono">{currentModel}</strong></span>
            ) : null}
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={onClose}
              className="px-4 py-2 rounded-xl text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-200 text-xs font-medium"
            >
              Close
            </button>
            <button
              onClick={() => {
                api.setBaseUrl(daemonUrl);
                api.setApiToken(apiToken);
                setSaved(true);
                setTimeout(() => setSaved(false), 2000);
              }}
              className="flex items-center gap-1.5 px-5 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold transition-colors shadow-xs"
            >
              {saved ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : null}
              <span>{saved ? 'Saved!' : 'Save Configuration'}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
