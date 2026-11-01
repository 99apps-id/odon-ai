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
} from 'lucide-react';
import { api, OAuthAccountInfo, OAuthLoginResponse } from '../api';
import { ProviderItem, ModelItem, FALLBACK_PROVIDERS, getCustomOpenAiModels, saveCustomOpenAiModels } from '../types';
import { ProviderLogo } from './ProviderLogo';

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
  google: 'https://generativelanguage.googleapis.com/v1beta',
  groq: 'https://api.groq.com/openai/v1',
  mistral: 'https://api.mistral.ai/v1',
  openrouter: 'https://openrouter.ai/api/v1',
  ollama: 'http://localhost:11434/v1',
  openai_compatible: 'https://api.openai.com/v1',
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
  const [activeTab, setActiveTab] = useState<'providers' | 'system'>('providers');
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
  const [apiKeys, setApiKeys] = useState<Record<string, string>>(() => {
    if (typeof window === 'undefined') return {};
    const keys: Record<string, string> = {};
    for (const id of Object.keys(DEFAULT_BASE_URLS)) {
      keys[id] = localStorage.getItem(`odon_key_${id}`) || '';
    }
    return keys;
  });

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

  // Custom models state for OpenAI-Compatible provider
  const [customOpenAiModels, setCustomOpenAiModels] = useState<ModelItem[]>(() => getCustomOpenAiModels());
  const [newModelId, setNewModelId] = useState('');
  const [newModelName, setNewModelName] = useState('');
  const [newModelContext, setNewModelContext] = useState(128000);

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
      if (configs?.length) {
        setApiKeys((prev) => {
          const next = { ...prev };
          for (const c of configs) {
            if (!next[c.provider_id]) {
              next[c.provider_id] = c.masked_key;
            }
          }
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
  const handleSaveKey = async (providerId: string) => {
    const rawKey = (apiKeys[providerId] || '').trim();
    const prefix = KEY_PREFIXES[providerId];

    if (rawKey && prefix && !rawKey.startsWith(prefix)) {
      setKeyError(`${currentProvider.name} API keys usually begin with "${prefix}".`);
    } else {
      setKeyError(null);
    }

    localStorage.setItem(`odon_key_${providerId}`, rawKey);
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
    localStorage.removeItem(`odon_key_${providerId}`);
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
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/50 backdrop-blur-xs animate-in fade-in duration-150">
      <div
        className="w-full max-w-4xl h-[650px] bg-white dark:bg-neutral-900 rounded-2xl shadow-2xl border border-neutral-200 dark:border-neutral-800 flex flex-col overflow-hidden text-neutral-900 dark:text-neutral-100"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800 shrink-0">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-xl bg-blue-50 dark:bg-blue-950/60 flex items-center justify-center text-blue-600 dark:text-blue-400">
              <Sliders className="w-4 h-4" />
            </div>
            <div>
              <h3 className="font-bold text-sm text-neutral-900 dark:text-white">Settings & Models</h3>
              <p className="text-xs text-neutral-500">Configure OAuth accounts, official BYOK keys, and daemon</p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <div className="flex bg-neutral-100 dark:bg-neutral-800 p-1 rounded-lg text-xs font-medium">
              <button
                onClick={() => setActiveTab('providers')}
                className={`px-3 py-1 rounded-md transition-all ${activeTab === 'providers'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                  }`}
              >
                Model Providers
              </button>
              <button
                onClick={() => setActiveTab('system')}
                className={`px-3 py-1 rounded-md transition-all ${activeTab === 'system'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                  }`}
              >
                System & Daemon
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
        {activeTab === 'providers' ? (
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
                                  className="flex items-center gap-1 px-3 py-2 rounded-xl bg-neutral-900 dark:bg-white text-white dark:text-neutral-900 text-xs font-semibold hover:opacity-90 transition-opacity"
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
                            placeholder={`Paste your ${currentProvider.name.split(' (')[0]} API key...`}
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
                          Tambah Model Kustom (OpenAI-Compatible)
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
                            placeholder="misal: deepseek-r1:32b"
                            className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-xs font-mono outline-none focus:border-blue-500"
                          />
                        </div>
                        <div className="sm:col-span-1">
                          <label className="text-[11px] text-neutral-500 font-mono mb-1 block">Nama Tampilan</label>
                          <input
                            type="text"
                            value={newModelName}
                            onChange={(e) => setNewModelName(e.target.value)}
                            placeholder="misal: Local DeepSeek R1"
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
                        <span>Tambah Baris Model</span>
                      </button>
                    </div>

                    {/* Table of custom models */}
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-bold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                          Daftar Model Terkonfigurasi ({customOpenAiModels.length})
                        </span>
                        <span className="text-[11px] text-neutral-400">Klik model untuk menjadikannya model aktif</span>
                      </div>

                      {customOpenAiModels.length === 0 ? (
                        <div className="p-4 rounded-xl border border-dashed border-neutral-300 dark:border-neutral-700 text-center text-xs text-neutral-400">
                          Belum ada model kustom. Masukkan Model ID di atas lalu klik Tambah Baris Model.
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
                                    title="Hapus model baris ini"
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
                setSaved(true);
                setTimeout(() => setSaved(false), 2000);
              }}
              className="flex items-center gap-1.5 px-5 py-2 rounded-xl bg-neutral-900 hover:bg-neutral-800 text-white dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-100 text-xs font-semibold transition-colors shadow-xs"
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
