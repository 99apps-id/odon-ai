import React, { useState } from 'react';
import { X, Shield, Server, Key, Sliders, Check } from 'lucide-react';
import { api } from '../api';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  isDark: boolean;
  onToggleTheme: () => void;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  isDark,
  onToggleTheme,
}) => {
  const [daemonUrl, setDaemonUrl] = useState(api.getBaseUrl());
  const [apiKey, setApiKey] = useState(
    (typeof window !== 'undefined' && localStorage.getItem('odon_openai_key')) || ''
  );
  const [baseUrl, setBaseUrl] = useState(
    (typeof window !== 'undefined' && localStorage.getItem('odon_openai_base')) || 'http://localhost:11434/v1'
  );
  const [saved, setSaved] = useState(false);

  if (!isOpen) return null;

  const handleSave = () => {
    api.setBaseUrl(daemonUrl);
    if (typeof window !== 'undefined') {
      localStorage.setItem('odon_openai_key', apiKey);
      localStorage.setItem('odon_openai_base', baseUrl);
    }
    setSaved(true);
    setTimeout(() => {
      setSaved(false);
      onClose();
    }, 800);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs select-none">
      <div className="w-[520px] max-w-[90vw] rounded-2xl bg-white dark:bg-[#1f1f1f] border border-neutral-200 dark:border-neutral-800 shadow-xl overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800">
          <div className="flex items-center gap-2">
            <Sliders className="w-4 h-4 text-neutral-900 dark:text-neutral-100" />
            <h3 className="font-semibold text-[15px] text-neutral-900 dark:text-neutral-100">
              Settings & Preferences
            </h3>
          </div>
          <button
            onClick={onClose}
            className="text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 p-1 rounded-md"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Form Body */}
        <div className="p-6 space-y-5 text-[13px]">
          {/* Daemon URL */}
          <div className="space-y-1.5">
            <label className="flex items-center gap-1.5 font-medium text-neutral-800 dark:text-neutral-200">
              <Server className="w-3.5 h-3.5 text-neutral-500" />
              ODON Daemon Address
            </label>
            <input
              type="text"
              value={daemonUrl}
              onChange={(e) => setDaemonUrl(e.target.value)}
              placeholder="http://127.0.0.1:4096"
              className="w-full px-3 py-2 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 outline-none focus:border-neutral-900 dark:focus:border-neutral-100 font-mono text-[12.5px]"
            />
            <p className="text-[11.5px] text-neutral-500">
              Address of the running `odond serve` background service.
            </p>
          </div>

          {/* Model Provider / BYOK */}
          <div className="space-y-1.5">
            <label className="flex items-center gap-1.5 font-medium text-neutral-800 dark:text-neutral-200">
              <Key className="w-3.5 h-3.5 text-neutral-500" />
              OpenAI / Anthropic / Ollama Base URL
            </label>
            <input
              type="text"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="http://localhost:11434/v1 or https://api.openai.com/v1"
              className="w-full px-3 py-2 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 outline-none focus:border-neutral-900 dark:focus:border-neutral-100 font-mono text-[12.5px]"
            />
          </div>

          <div className="space-y-1.5">
            <label className="font-medium text-neutral-800 dark:text-neutral-200">
              API Key (Optional / BYOK)
            </label>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="sk-... (Leave empty to use local Ollama or simulated provider)"
              className="w-full px-3 py-2 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 outline-none focus:border-neutral-900 dark:focus:border-neutral-100 font-mono text-[12.5px]"
            />
          </div>

          {/* Theme & Security Policy */}
          <div className="pt-2 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-between">
            <span className="font-medium text-neutral-800 dark:text-neutral-200">Interface Theme</span>
            <button
              onClick={onToggleTheme}
              className="px-3 py-1.5 rounded-lg border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-800 text-neutral-800 dark:text-neutral-200 text-[12px] font-medium hover:bg-neutral-100 dark:hover:bg-neutral-700 transition-colors"
            >
              {isDark ? 'Dark Mode' : 'Light Mode'}
            </button>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 px-6 py-3.5 bg-neutral-50 dark:bg-neutral-900 border-t border-neutral-200 dark:border-neutral-800">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-lg text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-200 text-[12.5px] font-medium"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-neutral-900 hover:bg-neutral-800 text-white dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-100 text-[12.5px] font-semibold transition-colors shadow-xs"
          >
            {saved ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : null}
            <span>{saved ? 'Saved!' : 'Save Changes'}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
