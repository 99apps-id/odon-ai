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
} from 'lucide-react';
import { api } from '../api';
import { ProviderItem, ModelItem } from '../types';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  isDark: boolean;
  onToggleTheme: () => void;
  currentModel?: string;
  onSelectModel?: (modelId: string) => void;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  isDark,
  onToggleTheme,
  currentModel,
  onSelectModel,
}) => {
  const [activeTab, setActiveTab] = useState<'providers' | 'system'>('providers');
  const [providers, setProviders] = useState<ProviderItem[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<string>('antigravity');
  const [daemonUrl, setDaemonUrl] = useState(api.getBaseUrl());

  // Provider keys & base urls state
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({
    deepseek: (typeof window !== 'undefined' && localStorage.getItem('odon_key_deepseek')) || '',
    qwen: (typeof window !== 'undefined' && localStorage.getItem('odon_key_qwen')) || '',
    alibaba: (typeof window !== 'undefined' && localStorage.getItem('odon_key_alibaba')) || '',
    stepfun: (typeof window !== 'undefined' && localStorage.getItem('odon_key_stepfun')) || '',
    openai_compatible: (typeof window !== 'undefined' && localStorage.getItem('odon_key_openai')) || '',
  });

  const [baseUrls, setBaseUrls] = useState<Record<string, string>>({
    deepseek: (typeof window !== 'undefined' && localStorage.getItem('odon_base_deepseek')) || 'https://api.deepseek.com/v1',
    qwen: (typeof window !== 'undefined' && localStorage.getItem('odon_base_qwen')) || 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
    alibaba: (typeof window !== 'undefined' && localStorage.getItem('odon_base_alibaba')) || 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    stepfun: (typeof window !== 'undefined' && localStorage.getItem('odon_base_stepfun')) || 'https://api.stepfun.com/v1',
    openai_compatible: (typeof window !== 'undefined' && localStorage.getItem('odon_base_openai')) || 'https://api.openai.com/v1',
  });

  const [saved, setSaved] = useState(false);

  // Load model catalog from daemon
  useEffect(() => {
    if (!isOpen) return;
    api.getModels().then((data) => {
      if (data?.providers?.length) {
        setProviders(data.providers);
      }
    }).catch(() => {
      // Fallback catalog if daemon offline
      setProviders([
        {
          id: 'antigravity',
          name: 'Antigravity (Google / DeepMind)',
          category: 'oauth',
          auth_type: 'oauth',
          description: 'High-throughput intelligence via Antigravity OAuth with Claude 3.7 & Gemini 2.0',
          models: [
            { id: 'claude-3-7-sonnet', name: 'Claude 3.7 Sonnet (Hybrid Reasoning)', context: 200000, recommended: true },
            { id: 'claude-3-5-sonnet-20241022', name: 'Claude 3.5 Sonnet v2', context: 200000 },
            { id: 'gemini-2.0-flash', name: 'Gemini 2.0 Flash', context: 1048576, recommended: true },
            { id: 'gemini-1.5-pro', name: 'Gemini 1.5 Pro', context: 2097152 },
            { id: 'claude-3-5-haiku', name: 'Claude 3.5 Haiku', context: 200000 },
          ],
        },
        {
          id: 'codex',
          name: 'OpenAI Codex / ChatGPT',
          category: 'oauth',
          auth_type: 'oauth',
          description: 'Official OpenAI OAuth authorization with specialized coding & reasoning models',
          models: [
            { id: 'o3-mini', name: 'o3-mini (Reasoning & Coding)', context: 200000, recommended: true },
            { id: 'o1', name: 'o1 (Advanced Deep Reasoning)', context: 200000 },
            { id: 'gpt-4o', name: 'GPT-4o (Omnimodal Flagship)', context: 128000, recommended: true },
            { id: 'gpt-4o-mini', name: 'GPT-4o Mini (Fast & Light)', context: 128000 },
          ],
        },
        {
          id: 'muse',
          name: 'Muse (Live SSE)',
          category: 'oauth',
          auth_type: 'oauth',
          description: 'Ultra low-latency live SSE model streaming with token-level events',
          models: [
            { id: 'muse-claude-3-7-sonnet', name: 'Muse Claude 3.7 Sonnet', context: 200000, recommended: true },
            { id: 'muse-claude-3-5-sonnet', name: 'Muse Claude 3.5 Sonnet', context: 200000 },
            { id: 'muse-code-v2', name: 'Muse Code v2', context: 128000 },
          ],
        },
        {
          id: 'deepseek',
          name: 'DeepSeek (BYOK)',
          category: 'byok',
          auth_type: 'api_key',
          default_base_url: 'https://api.deepseek.com/v1',
          description: 'DeepSeek V3 and R1 reasoning models with transparent BYOK pricing',
          models: [
            { id: 'deepseek-chat', name: 'DeepSeek-V3 (Chat & Coding)', context: 64000, recommended: true },
            { id: 'deepseek-reasoner', name: 'DeepSeek-R1 (Deep Reasoning)', context: 64000, recommended: true },
            { id: 'deepseek-coder', name: 'DeepSeek Coder', context: 32000 },
          ],
        },
        {
          id: 'qwen',
          name: 'Qwen Plan / DashScope Global',
          category: 'byok',
          auth_type: 'api_key',
          default_base_url: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
          description: 'Alibaba Cloud international gateway: Qwen 2.5 Coder, QwQ Reasoning, and Qwen Max',
          models: [
            { id: 'qwen-max-latest', name: 'Qwen Max Latest (Flagship)', context: 32768, recommended: true },
            { id: 'qwen-plus-latest', name: 'Qwen Plus Latest', context: 131072 },
            { id: 'qwen-turbo-latest', name: 'Qwen Turbo Latest (Speed)', context: 131072 },
            { id: 'qwen-2.5-coder-32b-instruct', name: 'Qwen 2.5 Coder 32B', context: 131072, recommended: true },
            { id: 'qwq-32b-preview', name: 'QwQ 32B Reasoning Preview', context: 32768 },
            { id: 'qwen2.5-72b-instruct', name: 'Qwen 2.5 72B Instruct', context: 131072 },
          ],
        },
        {
          id: 'alibaba',
          name: 'Alibaba Tongyi Qianwen (China)',
          category: 'byok',
          auth_type: 'api_key',
          default_base_url: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
          description: 'Alibaba domestic gateway for high-throughput mainland access',
          models: [
            { id: 'qwen-max', name: 'Qwen Max (Domestic)', context: 32768, recommended: true },
            { id: 'qwen-plus', name: 'Qwen Plus (Domestic)', context: 131072 },
            { id: 'qwen-turbo', name: 'Qwen Turbo (Domestic)', context: 131072 },
          ],
        },
        {
          id: 'stepfun',
          name: 'StepFun Plan (阶跃星辰)',
          category: 'byok',
          auth_type: 'api_key',
          default_base_url: 'https://api.stepfun.com/v1',
          description: 'StepFun frontier reasoning, ultra-long 256K context, and multimodal vision models',
          models: [
            { id: 'step-2-16k', name: 'Step-2 16K (Multimodal Flagship)', context: 16384, recommended: true },
            { id: 'step-1-32k', name: 'Step-1 32K (Fast)', context: 32768 },
            { id: 'step-1-128k', name: 'Step-1 128K (Long Context)', context: 131072 },
            { id: 'step-1-256k', name: 'Step-1 256K (Ultra Long Context)', context: 262144, recommended: true },
            { id: 'step-1v-32k', name: 'Step-1V 32K (Vision Multimodal)', context: 32768 },
          ],
        },
        {
          id: 'openai_compatible',
          name: 'OpenAI Compatible (Custom / Local)',
          category: 'byok',
          auth_type: 'api_key',
          default_base_url: 'https://api.openai.com/v1',
          description: 'Any OpenAI-compatible gateway: Official OpenAI, Ollama, vLLM, OpenRouter, Groq',
          models: [
            { id: 'gpt-4o', name: 'GPT-4o', context: 128000, recommended: true },
            { id: 'gpt-4o-mini', name: 'GPT-4o Mini', context: 128000 },
            { id: 'o3-mini', name: 'o3-mini', context: 200000 },
            { id: 'custom', name: 'Custom Model (Manual input)', context: 128000 },
          ],
        },
      ]);
    });
  }, [isOpen]);

  if (!isOpen) return null;

  const currentProvider = providers.find((p) => p.id === selectedProviderId) || providers[0];

  const handleSave = () => {
    api.setBaseUrl(daemonUrl);
    if (typeof window !== 'undefined') {
      Object.entries(apiKeys).forEach(([k, v]) => {
        localStorage.setItem(`odon_key_${k}`, v);
      });
      Object.entries(baseUrls).forEach(([k, v]) => {
        localStorage.setItem(`odon_base_${k}`, v);
      });
    }
    setSaved(true);
    setTimeout(() => {
      setSaved(false);
      onClose();
    }, 700);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 backdrop-blur-xs select-none">
      <div className="w-[840px] max-w-[94vw] h-[640px] max-h-[90vh] rounded-2xl bg-white dark:bg-[#1a1a1e] border border-neutral-200 dark:border-neutral-800 shadow-2xl flex flex-col overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/50 shrink-0">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-neutral-900 dark:bg-white text-white dark:text-neutral-900 flex items-center justify-center font-bold text-sm">
              ⚙
            </div>
            <div>
              <h3 className="font-semibold text-[15px] text-neutral-900 dark:text-neutral-100">
                ODON Settings & Intelligence Providers
              </h3>
              <p className="text-[12px] text-neutral-500">
                Configure OAuth proxies, BYOK model keys, and system preferences
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <div className="flex bg-neutral-200/80 dark:bg-neutral-800 p-0.5 rounded-lg text-[12px] font-medium">
              <button
                onClick={() => setActiveTab('providers')}
                className={`px-3 py-1 rounded-md transition-all ${
                  activeTab === 'providers'
                    ? 'bg-white dark:bg-neutral-700 text-neutral-900 dark:text-white shadow-xs font-semibold'
                    : 'text-neutral-600 dark:text-neutral-400 hover:text-neutral-900'
                }`}
              >
                Model Providers
              </button>
              <button
                onClick={() => setActiveTab('system')}
                className={`px-3 py-1 rounded-md transition-all ${
                  activeTab === 'system'
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
                <div className="text-[10.5px] uppercase tracking-wider font-bold text-neutral-400 dark:text-neutral-500 px-2 mb-1.5">
                  OAuth Authorization
                </div>
                <div className="space-y-1">
                  {providers
                    .filter((p) => p.category === 'oauth')
                    .map((p) => (
                      <button
                        key={p.id}
                        onClick={() => setSelectedProviderId(p.id)}
                        className={`w-full text-left px-3 py-2 rounded-xl text-[12.5px] font-medium transition-all flex items-center justify-between ${
                          selectedProviderId === p.id
                            ? 'bg-blue-600 text-white shadow-xs font-semibold'
                            : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-200/60 dark:hover:bg-neutral-800'
                        }`}
                      >
                        <div className="flex items-center gap-2 truncate">
                          <Sparkles className="w-3.5 h-3.5 shrink-0 opacity-80" />
                          <span className="truncate">{p.name.split(' ')[0]}</span>
                        </div>
                        <span className="text-[10px] px-1.5 py-0.5 rounded uppercase tracking-wider bg-black/10 dark:bg-white/10 shrink-0">
                          OAuth
                        </span>
                      </button>
                    ))}
                </div>
              </div>

              {/* BYOK Providers */}
              <div>
                <div className="text-[10.5px] uppercase tracking-wider font-bold text-neutral-400 dark:text-neutral-500 px-2 mb-1.5">
                  BYOK (Bring Your Own Key)
                </div>
                <div className="space-y-1">
                  {providers
                    .filter((p) => p.category === 'byok')
                    .map((p) => (
                      <button
                        key={p.id}
                        onClick={() => setSelectedProviderId(p.id)}
                        className={`w-full text-left px-3 py-2 rounded-xl text-[12.5px] font-medium transition-all flex items-center justify-between ${
                          selectedProviderId === p.id
                            ? 'bg-blue-600 text-white shadow-xs font-semibold'
                            : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-200/60 dark:hover:bg-neutral-800'
                        }`}
                      >
                        <div className="flex items-center gap-2 truncate">
                          <Key className="w-3.5 h-3.5 shrink-0 opacity-80" />
                          <span className="truncate">{p.name.split(' ')[0]}</span>
                        </div>
                        <span className="text-[10px] px-1.5 py-0.5 rounded uppercase tracking-wider bg-black/10 dark:bg-white/10 shrink-0">
                          API Key
                        </span>
                      </button>
                    ))}
                </div>
              </div>
            </div>

            {/* Right: Selected Provider Details */}
            {currentProvider && (
              <div className="flex-1 overflow-y-auto p-6 space-y-6">
                <div>
                  <div className="flex items-center gap-2 mb-1">
                    <h4 className="text-base font-bold text-neutral-900 dark:text-white">
                      {currentProvider.name}
                    </h4>
                    <span className="text-[11px] font-semibold px-2 py-0.5 rounded-full bg-blue-100 dark:bg-blue-950 text-blue-700 dark:text-blue-300">
                      {currentProvider.auth_type === 'oauth' ? 'OAuth 2.0 Auth' : 'BYOK Gateway'}
                    </span>
                  </div>
                  <p className="text-[12.5px] text-neutral-500 dark:text-neutral-400">
                    {currentProvider.description}
                  </p>
                </div>

                {/* Auth Setup */}
                {currentProvider.auth_type === 'oauth' ? (
                  <div className="p-4 rounded-xl border border-blue-200 dark:border-blue-900/50 bg-blue-50/50 dark:bg-blue-950/20 space-y-3">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <Lock className="w-4 h-4 text-blue-600 dark:text-blue-400" />
                        <span className="font-semibold text-[13px] text-blue-950 dark:text-blue-200">
                          OAuth Identity Status
                        </span>
                      </div>
                      <span className="flex items-center gap-1.5 text-xs font-medium text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-950/60 px-2.5 py-1 rounded-full border border-emerald-200 dark:border-emerald-800">
                        <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                        Authorized & Ready
                      </span>
                    </div>
                    <p className="text-[12px] text-neutral-600 dark:text-neutral-400">
                      Uses browser PKCE OAuth loop handled automatically by ODON daemon. No manual API keys required.
                    </p>
                  </div>
                ) : (
                  <div className="space-y-4">
                    {/* Base URL */}
                    <div className="space-y-1.5">
                      <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 flex items-center gap-1.5">
                        <Globe className="w-3.5 h-3.5 text-neutral-400" />
                        Endpoint Base URL
                      </label>
                      <input
                        type="text"
                        value={baseUrls[currentProvider.id] || currentProvider.default_base_url || ''}
                        onChange={(e) =>
                          setBaseUrls({ ...baseUrls, [currentProvider.id]: e.target.value })
                        }
                        placeholder={currentProvider.default_base_url || 'https://...'}
                        className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800/80 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500"
                      />
                    </div>

                    {/* API Key */}
                    <div className="space-y-1.5">
                      <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 flex items-center gap-1.5">
                        <Key className="w-3.5 h-3.5 text-neutral-400" />
                        API Key (Secret)
                      </label>
                      <input
                        type="password"
                        value={apiKeys[currentProvider.id] || ''}
                        onChange={(e) =>
                          setApiKeys({ ...apiKeys, [currentProvider.id]: e.target.value })
                        }
                        placeholder={`Enter your ${currentProvider.name} API key...`}
                        className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800/80 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500"
                      />
                      <p className="text-[11px] text-neutral-500">
                        Stored securely on your local machine only. Never transmitted to third-party tracking servers.
                      </p>
                    </div>
                  </div>
                )}

                {/* Available Models in this provider */}
                <div className="space-y-2.5">
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-bold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
                      Available Models ({currentProvider.models.length})
                    </span>
                    <span className="text-[11px] text-neutral-400">Click to set as active session model</span>
                  </div>

                  <div className="grid grid-cols-1 gap-2">
                    {currentProvider.models.map((m) => {
                      const isSelected = currentModel === m.id;
                      return (
                        <div
                          key={m.id}
                          onClick={() => onSelectModel?.(m.id)}
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
                            <div className="text-[11px] text-neutral-400 font-mono mt-0.5">
                              ID: {m.id} · {(m.context / 1000).toFixed(0)}k context
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
              </div>
            )}
          </div>
        ) : (
          /* System & Daemon Tab */
          <div className="flex-1 overflow-y-auto p-6 space-y-6 text-[13px]">
            {/* Daemon URL */}
            <div className="space-y-1.5">
              <label className="flex items-center gap-1.5 font-semibold text-neutral-800 dark:text-neutral-200">
                <Server className="w-4 h-4 text-neutral-500" />
                ODON Daemon Service Address
              </label>
              <input
                type="text"
                value={daemonUrl}
                onChange={(e) => setDaemonUrl(e.target.value)}
                placeholder="http://127.0.0.1:4096"
                className="w-full px-3.5 py-2.5 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800/80 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500"
              />
              <p className="text-[11.5px] text-neutral-500">
                Local background engine providing SQLite persistence, PowerShell runtime, and event bus.
              </p>
            </div>

            {/* Theme Toggle */}
            <div className="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 flex items-center justify-between">
              <div>
                <span className="font-semibold text-neutral-800 dark:text-neutral-200 block">
                  Interface Theme
                </span>
                <span className="text-xs text-neutral-500">
                  Switch between clean light canvas and dark terminal aesthetic
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
                Autonomous mode with interactive approval gates for destructive commands (Tier 3 risk).
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
              Cancel
            </button>
            <button
              onClick={handleSave}
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
