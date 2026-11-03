import React, { useState } from 'react';
import {
  Terminal as TerminalIcon,
  RotateCcw,
  Copy,
  Check,
  Plus,
  X,
  ChevronDown,
  Sparkles,
  Command,
} from 'lucide-react';
import { NativeTerminal } from './NativeTerminal';

interface TerminalTab {
  id: string;
  title: string;
  shell: string;
  cwd?: string;
}

interface TerminalViewProps {
  onClose?: () => void;
  onSendToAgent?: (text: string) => void;
}

/**
 * Shells offered for new tabs, per host platform. The daemon spawns the
 * tab's shell name as a program, so offering Windows shells on Linux or
 * macOS makes every tab fail to start; the default tab is the first
 * entry, which is the platform's normal shell.
 */
const IS_WINDOWS =
  typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);

interface ShellOption {
  id: string;
  name: string;
  hint: string;
}

const SHELL_OPTIONS: ShellOption[] = IS_WINDOWS
  ? [
      { id: 'powershell', name: 'PowerShell', hint: 'Default' },
      { id: 'cmd', name: 'Command Prompt', hint: 'CMD' },
      { id: 'wsl', name: 'WSL / Linux', hint: 'Bash' },
      { id: 'bash', name: 'Git Bash', hint: 'sh' },
    ]
  : [
      { id: 'bash', name: 'Bash', hint: 'Default' },
      { id: 'zsh', name: 'Zsh', hint: 'zsh' },
      { id: 'sh', name: 'POSIX sh', hint: 'sh' },
    ];

export const TerminalView: React.FC<TerminalViewProps> = ({ onClose, onSendToAgent }) => {
  const [tabs, setTabs] = useState<TerminalTab[]>([
    {
      id: `tab-${SHELL_OPTIONS[0].id}-1`,
      title: `${SHELL_OPTIONS[0].name} #1`,
      shell: SHELL_OPTIONS[0].id,
    },
  ]);
  const [activeTabId, setActiveTabId] = useState<string>(
    `tab-${SHELL_OPTIONS[0].id}-1`,
  );
  const [isAddMenuOpen, setIsAddMenuOpen] = useState<boolean>(false);

  const activeTab = tabs.find((t) => t.id === activeTabId) || tabs[0];

  const handleAddTab = (shell: string, name: string) => {
    const newId = `tab-${shell}-${Date.now()}`;
    const count = tabs.filter((t) => t.shell === shell).length + 1;
    const newTab: TerminalTab = {
      id: newId,
      title: `${name} #${count}`,
      shell,
    };
    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(newId);
    setIsAddMenuOpen(false);
  };

  const handleCloseTab = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (tabs.length === 1) return;
    const nextTabs = tabs.filter((t) => t.id !== id);
    setTabs(nextTabs);
    if (activeTabId === id) {
      setActiveTabId(nextTabs[0].id);
    }
  };

  return (
    <div className="flex-1 flex flex-col h-full bg-[#0d1117] rounded-2xl overflow-hidden border border-neutral-800 shadow-2xs font-sans">
      {/* ───────────────────────────────────────────────────────────
          TOP TAB BAR & CONTROLS
      ─────────────────────────────────────────────────────────── */}
      <div className="h-10 bg-[#161b22] border-b border-neutral-800 flex items-center justify-between px-2 shrink-0 select-none">
        {/* Tab list */}
        <div className="flex items-center gap-1 overflow-x-auto scrollbar-none h-full">
          {tabs.map((tab) => {
            const isActive = tab.id === activeTabId;
            return (
              <div
                key={tab.id}
                onClick={() => setActiveTabId(tab.id)}
                className={`flex items-center gap-2 px-3 py-1.5 rounded-lg text-xs font-mono font-medium transition-all cursor-pointer border ${
                  isActive
                    ? 'bg-[#0d1117] text-white border-neutral-700 shadow-xs'
                    : 'text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800/60 border-transparent'
                }`}
              >
                <TerminalIcon className={`w-3.5 h-3.5 ${isActive ? 'text-blue-400' : 'text-neutral-500'}`} />
                <span className="truncate max-w-[120px]">{tab.title}</span>

                {tabs.length > 1 && (
                  <button
                    onClick={(e) => handleCloseTab(tab.id, e)}
                    className="p-0.5 rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-700/60 cursor-pointer"
                    title="Close Tab"
                  >
                    <X className="w-3 h-3" />
                  </button>
                )}
              </div>
            );
          })}

          {/* Add Tab Button with Dropdown */}
          <div className="relative">
            <button
              onClick={() => setIsAddMenuOpen(!isAddMenuOpen)}
              className="flex items-center gap-1 p-1.5 rounded-lg text-neutral-400 hover:text-white hover:bg-neutral-800 transition-colors cursor-pointer"
              title="Open New Terminal Shell"
            >
              <Plus className="w-3.5 h-3.5" />
              <ChevronDown className="w-3 h-3" />
            </button>

            {isAddMenuOpen && (
              <div className="absolute left-0 top-full mt-1 w-44 bg-[#1f242c] border border-neutral-700 rounded-xl shadow-xl p-1 z-30 text-xs font-mono text-neutral-200 space-y-0.5">
                {SHELL_OPTIONS.map((option) => (
                  <button
                    key={option.id}
                    onClick={() => handleAddTab(option.id, option.name)}
                    className="w-full text-left px-2.5 py-1.5 rounded-lg hover:bg-blue-600 hover:text-white transition-colors cursor-pointer flex items-center justify-between"
                  >
                    <span>{option.name}</span>
                    <span className="text-[10px] text-neutral-400 font-sans">{option.hint}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Right Info Controls */}
        <div className="flex items-center gap-2">
          <span className="text-[11px] font-mono text-neutral-400 bg-neutral-800/80 px-2 py-0.5 rounded-md hidden sm:inline">
            {IS_WINDOWS ? 'Native ConPTY Engine' : 'Native PTY Engine'}
          </span>

          {onClose && (
            <button
              onClick={onClose}
              className="p-1 rounded-md text-neutral-400 hover:text-white hover:bg-neutral-800 transition-colors cursor-pointer"
              title="Close Terminal Pane"
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      {/* ───────────────────────────────────────────────────────────
          TERMINAL BODIES (Cached per tab for persistent sessions)
      ─────────────────────────────────────────────────────────── */}
      <div className="flex-1 relative overflow-hidden">
        {tabs.map((tab) => {
          const isVisible = tab.id === activeTabId;
          return (
            <div
              key={tab.id}
              className={`w-full h-full absolute inset-0 ${isVisible ? 'block z-10' : 'hidden z-0'}`}
            >
              <NativeTerminal
                shell={tab.shell}
                cwd={tab.cwd}
                title={tab.title}
                className="w-full h-full rounded-none border-none"
              />
            </div>
          );
        })}
      </div>
    </div>
  );
};
