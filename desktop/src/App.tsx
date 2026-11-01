import React, { useState } from 'react';
import {
  MessageSquare,
  Code2,
  FileText,
  FolderOpen,
  Bot,
  Zap,
  Radio,
  SlidersHorizontal,
  ChevronDown,
  Settings,
  Paperclip,
  Image as ImageIcon,
  Globe,
  ArrowUp,
  Plus,
  Play,
  GitBranch,
  CheckCircle2,
  MoreHorizontal,
  Layers,
  FileCode,
  ExternalLink,
  PanelRightClose,
  PanelRightOpen,
  PanelLeftClose,
  PanelLeftOpen,
  Maximize2,
  Minimize2,
  X,
  Share2,
} from 'lucide-react';
import { CodeEditorView } from './components/CodeEditorView';
import { BrowserPreviewView } from './components/BrowserPreviewView';

export default function App() {
  const [activeRightTab, setActiveRightTab] = useState<'artifacts' | 'editor' | 'browser' | 'relays'>('artifacts');
  const [isRightPanelOpen, setIsRightPanelOpen] = useState(true);
  const [isRightPanelWide, setIsRightPanelWide] = useState(false);
  const [isLeftSidebarOpen, setIsLeftSidebarOpen] = useState(true);
  const [selectedModel, setSelectedModel] = useState('ODON Auto');
  const [inputMessage, setInputMessage] = useState('');
  const [hasTurn, setHasTurn] = useState(true);

  // Helper to open right panel to a specific view
  const openRightView = (tab: 'artifacts' | 'editor' | 'browser' | 'relays') => {
    setActiveRightTab(tab);
    setIsRightPanelOpen(true);
  };

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-white text-neutral-900 font-sans antialiased">
      {/* ─────────────────────────────────────────────────────────────
          LEFT SIDEBAR: MINIMALIST CHATGPT STYLE
      ───────────────────────────────────────────────────────────── */}
      {isLeftSidebarOpen && (
        <aside className="w-[260px] shrink-0 border-r border-neutral-200 bg-[#f9f9f9] flex flex-col justify-between select-none">
          {/* Top Actions */}
          <div className="p-3 border-b border-neutral-200/80 flex items-center justify-between">
            <button
              onClick={() => setHasTurn(false)}
              className="flex items-center gap-2 px-3 py-1.5 rounded-lg text-xs font-medium text-neutral-800 hover:bg-neutral-200/60 transition-colors"
            >
              <div className="w-5 h-5 rounded-md bg-neutral-900 text-white flex items-center justify-center font-bold text-[11px]">
                O
              </div>
              <span className="font-semibold text-neutral-900">New Chat</span>
            </button>

            <button
              onClick={() => setIsLeftSidebarOpen(false)}
              className="p-1.5 text-neutral-500 hover:text-neutral-900 hover:bg-neutral-200/60 rounded-md transition-colors"
              title="Close sidebar"
            >
              <PanelLeftClose className="w-4 h-4" />
            </button>
          </div>

          {/* History List */}
          <div className="flex-1 overflow-y-auto px-3 py-3 space-y-4">
            <div>
              <div className="text-[11px] font-medium text-neutral-400 px-2 mb-1.5">Today</div>
              <div className="space-y-0.5">
                {[
                  { title: 'Personal knowledge hub app', active: true },
                  { title: 'Refactor SQLite storage engine', active: false },
                ].map((c, i) => (
                  <button
                    key={i}
                    onClick={() => setHasTurn(true)}
                    className={`w-full text-left px-2.5 py-1.5 rounded-md text-xs truncate transition-colors ${
                      c.active
                        ? 'bg-neutral-200/70 text-neutral-900 font-medium'
                        : 'text-neutral-600 hover:bg-neutral-200/40 hover:text-neutral-900'
                    }`}
                  >
                    {c.title}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <div className="text-[11px] font-medium text-neutral-400 px-2 mb-1.5">Previous 7 Days</div>
              <div className="space-y-0.5">
                {[
                  { title: 'Telegram relay integration', active: false },
                  { title: 'Security policy risk tiers', active: false },
                  { title: 'Cloud Code OAuth PKCE test', active: false },
                  { title: 'React 19 Server Actions audit', active: false },
                ].map((c, i) => (
                  <button
                    key={i}
                    onClick={() => setHasTurn(true)}
                    className="w-full text-left px-2.5 py-1.5 rounded-md text-xs text-neutral-600 hover:bg-neutral-200/40 hover:text-neutral-900 truncate transition-colors"
                  >
                    {c.title}
                  </button>
                ))}
              </div>
            </div>
          </div>

          {/* Bottom Account Row */}
          <div className="p-3 border-t border-neutral-200/80">
            <div className="flex items-center justify-between p-1.5 rounded-lg hover:bg-neutral-200/60 cursor-pointer transition-colors">
              <div className="flex items-center gap-2.5">
                <div className="w-7 h-7 rounded-full bg-neutral-800 text-white flex items-center justify-center text-xs font-semibold">
                  AC
                </div>
                <div>
                  <div className="text-xs font-medium text-neutral-900 leading-none">Alex Chen</div>
                  <span className="text-[11px] text-neutral-500">Pro plan</span>
                </div>
              </div>
              <Settings className="w-3.5 h-3.5 text-neutral-400" />
            </div>
          </div>
        </aside>
      )}

      {/* ─────────────────────────────────────────────────────────────
          CENTER COLUMN: CHATGPT-GRADE CONVERSATIONAL WORKSPACE
      ───────────────────────────────────────────────────────────── */}
      <main className="flex-1 flex flex-col bg-white overflow-hidden min-w-0">
        {/* Top Header */}
        <header className="h-12 border-b border-neutral-200/80 px-4 flex items-center justify-between shrink-0 bg-white">
          <div className="flex items-center gap-2">
            {!isLeftSidebarOpen && (
              <button
                onClick={() => setIsLeftSidebarOpen(true)}
                className="p-1.5 text-neutral-500 hover:text-neutral-900 hover:bg-neutral-100 rounded-md transition-colors mr-1"
                title="Open sidebar"
              >
                <PanelLeftOpen className="w-4 h-4" />
              </button>
            )}

            {/* Model Selector Pill */}
            <button className="flex items-center gap-1 px-2.5 py-1 rounded-md hover:bg-neutral-100 text-xs font-semibold text-neutral-900 transition-colors">
              <span>{selectedModel}</span>
              <ChevronDown className="w-3.5 h-3.5 text-neutral-500" />
            </button>
          </div>

          <div className="flex items-center gap-1.5">
            <button className="p-1.5 text-neutral-500 hover:text-neutral-900 hover:bg-neutral-100 rounded-md transition-colors" title="Share chat">
              <Share2 className="w-4 h-4" />
            </button>

            {/* Toggle Canvas / Right Panel */}
            <button
              onClick={() => setIsRightPanelOpen(!isRightPanelOpen)}
              className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${
                isRightPanelOpen
                  ? 'bg-neutral-100 text-neutral-900'
                  : 'text-neutral-600 hover:bg-neutral-100 hover:text-neutral-900'
              }`}
              title={isRightPanelOpen ? 'Hide right panel' : 'Open right panel'}
            >
              {isRightPanelOpen ? (
                <>
                  <PanelRightClose className="w-4 h-4 text-neutral-600" />
                  <span className="hidden sm:inline">Close Canvas</span>
                </>
              ) : (
                <>
                  <PanelRightOpen className="w-4 h-4 text-neutral-600" />
                  <span className="hidden sm:inline">Open Canvas</span>
                </>
              )}
            </button>
          </div>
        </header>

        {/* Chat Feed */}
        <div className="flex-1 overflow-y-auto px-6 py-6 space-y-6">
          {!hasTurn ? (
            /* Empty State */
            <div className="h-full flex flex-col items-center justify-center max-w-xl mx-auto text-center space-y-6 my-auto">
              <div className="w-10 h-10 rounded-full bg-neutral-900 text-white flex items-center justify-center text-sm font-bold mx-auto">
                O
              </div>
              <h2 className="text-xl font-semibold text-neutral-900 tracking-tight">What do you want to build today?</h2>

              <div className="grid grid-cols-2 gap-2.5 w-full text-left">
                {[
                  { title: 'Build React 19 app', desc: 'Minimal note manager with Vite', prompt: 'Build a minimal web app with React 19' },
                  { title: 'Generate project assets', desc: 'Hero illustration & UI preview cards', prompt: 'Generate high quality project landing assets' },
                  { title: 'Write product PRD', desc: 'Requirements, scope, and technical design', prompt: 'Write a comprehensive PRD for our product' },
                  { title: 'Orchestrate agents', desc: 'Autonomous research and sync pipelines', prompt: 'Run a research agent on knowledge graphs' },
                ].map((card, i) => (
                  <button
                    key={i}
                    onClick={() => {
                      setInputMessage(card.prompt);
                      setHasTurn(true);
                    }}
                    className="p-3.5 rounded-xl border border-neutral-200 bg-white hover:border-neutral-400 hover:bg-neutral-50/50 transition-all text-left"
                  >
                    <div className="text-xs font-semibold text-neutral-900">{card.title}</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5">{card.desc}</div>
                  </button>
                ))}
              </div>
            </div>
          ) : (
            /* Active Conversational Turn */
            <div className="max-w-2xl mx-auto space-y-6">
              {/* User Message */}
              <div className="flex justify-end">
                <div className="bg-[#f4f4f4] text-neutral-900 rounded-2xl px-4 py-2.5 text-[14px] leading-relaxed max-w-[85%] select-text">
                  I want to build a small "personal knowledge hub" project. Please:
                  <br />1) Write the core code (a minimal web app with React 19) for managing notes.
                  <br />2) Create a product requirement document (PRD) for it.
                  <br />3) Generate a few high-quality images for the project (hero illustration and UI preview).
                  <br />Organize the outputs clearly and let me know what you've done.
                </div>
              </div>

              {/* Assistant Message */}
              <div className="flex items-start gap-3 select-text">
                <div className="w-7 h-7 rounded-full bg-neutral-900 text-white flex items-center justify-center text-xs font-bold shrink-0 mt-0.5">
                  O
                </div>
                <div className="flex-1 space-y-3.5">
                  <div className="text-[14px] text-neutral-900 leading-relaxed space-y-3">
                    <p>
                      I have structured the "Personal Knowledge Hub" solution across code, product requirements, and visuals. Here is a summary of what has been created:
                    </p>

                    <ul className="space-y-1.5 pl-1">
                      <li className="flex items-start gap-2">
                        <span className="text-neutral-400 select-none">•</span>
                        <span><strong>Code</strong>: React 19 application with Vite and TypeScript, featuring note creation, local persistence, and filtering.</span>
                      </li>
                      <li className="flex items-start gap-2">
                        <span className="text-neutral-400 select-none">•</span>
                        <span><strong>PRD</strong>: Detailed product requirements document covering specifications, data models, and milestones.</span>
                      </li>
                      <li className="flex items-start gap-2">
                        <span className="text-neutral-400 select-none">•</span>
                        <span><strong>Visual Assets</strong>: Concept illustration and UI layout preview ready for production.</span>
                      </li>
                    </ul>

                    {/* Clean Action Cards (Canvas Trigger) */}
                    <div className="pt-2">
                      <div className="text-xs font-semibold text-neutral-500 mb-2">Project Artifacts</div>
                      <div className="grid grid-cols-3 gap-2">
                        {/* Card 1: Code -> Editor */}
                        <div
                          onClick={() => openRightView('editor')}
                          className="p-3 rounded-lg border border-neutral-200 bg-white hover:border-neutral-900 hover:shadow-2xs transition-all cursor-pointer group"
                        >
                          <div className="flex items-center justify-between mb-1.5">
                            <Code2 className="w-4 h-4 text-neutral-700" />
                            <span className="text-[10px] font-mono text-neutral-400 group-hover:text-neutral-900">Code</span>
                          </div>
                          <div className="text-xs font-semibold text-neutral-900 truncate">personal-notes</div>
                          <div className="text-[11px] text-neutral-500">React 19 • TypeScript</div>
                        </div>

                        {/* Card 2: Document -> Artifacts */}
                        <div
                          onClick={() => openRightView('artifacts')}
                          className="p-3 rounded-lg border border-neutral-200 bg-white hover:border-neutral-900 hover:shadow-2xs transition-all cursor-pointer group"
                        >
                          <div className="flex items-center justify-between mb-1.5">
                            <FileText className="w-4 h-4 text-neutral-700" />
                            <span className="text-[10px] font-mono text-neutral-400 group-hover:text-neutral-900">Doc</span>
                          </div>
                          <div className="text-xs font-semibold text-neutral-900 truncate">PRD.md</div>
                          <div className="text-[11px] text-neutral-500">12.4 KB • Markdown</div>
                        </div>

                        {/* Card 3: Images -> Artifacts */}
                        <div
                          onClick={() => openRightView('artifacts')}
                          className="p-3 rounded-lg border border-neutral-200 bg-white hover:border-neutral-900 hover:shadow-2xs transition-all cursor-pointer group"
                        >
                          <div className="flex items-center justify-between mb-1.5">
                            <ImageIcon className="w-4 h-4 text-neutral-700" />
                            <span className="text-[10px] font-mono text-neutral-400 group-hover:text-neutral-900">Image</span>
                          </div>
                          <div className="text-xs font-semibold text-neutral-900 truncate">hero-concept.png</div>
                          <div className="text-[11px] text-neutral-500">1920 × 1080 • PNG</div>
                        </div>
                      </div>
                    </div>

                    <p className="text-xs text-neutral-600 pt-1">
                      Choose an action to inspect or preview:
                    </p>

                    {/* Action Buttons */}
                    <div className="flex flex-wrap gap-2 pt-0.5">
                      <button
                        onClick={() => openRightView('browser')}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-full border border-neutral-900 bg-neutral-900 hover:bg-neutral-800 text-white text-xs font-medium transition-colors shadow-2xs"
                      >
                        <Play className="w-3 h-3 fill-current" />
                        Preview in Browser
                      </button>

                      <button
                        onClick={() => openRightView('editor')}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-full border border-neutral-300 hover:border-neutral-900 text-neutral-800 text-xs font-medium transition-colors"
                      >
                        <Code2 className="w-3 h-3" />
                        Open Code Editor
                      </button>

                      <button
                        onClick={() => openRightView('artifacts')}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-full border border-neutral-300 hover:border-neutral-900 text-neutral-800 text-xs font-medium transition-colors"
                      >
                        <GitBranch className="w-3 h-3 text-neutral-600" />
                        View Git Patch
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Floating Input Area (ChatGPT Style) */}
        <div className="p-4 bg-white border-t border-neutral-100">
          <div className="max-w-2xl mx-auto">
            <div className="rounded-3xl border border-neutral-200 bg-[#f4f4f4] focus-within:border-neutral-400 focus-within:bg-white transition-all px-4 py-2.5 shadow-2xs">
              <textarea
                value={inputMessage}
                onChange={(e) => setInputMessage(e.target.value)}
                placeholder="Ask ODON anything or write code..."
                className="w-full resize-none outline-none text-[14px] text-neutral-900 placeholder-neutral-400 bg-transparent min-h-[44px]"
                rows={1}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    if (inputMessage.trim()) {
                      setHasTurn(true);
                      setInputMessage('');
                    }
                  }
                }}
              />

              <div className="flex items-center justify-between pt-1">
                <div className="flex items-center gap-1 text-neutral-500">
                  <button className="p-1.5 hover:text-neutral-900 hover:bg-neutral-200/60 rounded-full transition-colors" title="Attach file">
                    <Paperclip className="w-4 h-4" />
                  </button>
                  <button className="p-1.5 hover:text-neutral-900 hover:bg-neutral-200/60 rounded-full transition-colors" title="Generate image">
                    <ImageIcon className="w-4 h-4" />
                  </button>
                  <button className="p-1.5 hover:text-neutral-900 hover:bg-neutral-200/60 rounded-full transition-colors" title="Web search">
                    <Globe className="w-4 h-4" />
                  </button>
                  <button
                    onClick={() => openRightView('editor')}
                    className="p-1.5 hover:text-neutral-900 hover:bg-neutral-200/60 rounded-full transition-colors"
                    title="Code editor"
                  >
                    <Code2 className="w-4 h-4" />
                  </button>
                </div>

                <button
                  onClick={() => {
                    if (inputMessage.trim()) {
                      setHasTurn(true);
                      setInputMessage('');
                    }
                  }}
                  disabled={!inputMessage.trim()}
                  className="w-7 h-7 rounded-full bg-neutral-900 disabled:opacity-20 text-white flex items-center justify-center hover:bg-neutral-800 transition-colors"
                >
                  <ArrowUp className="w-4 h-4" />
                </button>
              </div>
            </div>
            <div className="text-center text-[11px] text-neutral-400 mt-2">
              ODON can make mistakes. Verify important information.
            </div>
          </div>
        </div>
      </main>

      {/* ─────────────────────────────────────────────────────────────
          RIGHT DRAWER (CANVAS): CODE EDITOR, BROWSER, ARTIFACTS
      ───────────────────────────────────────────────────────────── */}
      {isRightPanelOpen && (
        <aside
          className={`shrink-0 border-l border-neutral-200 bg-white flex flex-col justify-between overflow-hidden transition-all duration-200 ${
            isRightPanelWide ? 'w-[580px]' : 'w-[440px]'
          }`}
        >
          {/* Top Canvas Bar */}
          <div className="p-2.5 border-b border-neutral-200 bg-white flex items-center justify-between gap-2 shrink-0">
            {/* Segmented Tab Control */}
            <div className="flex-1 flex items-center p-0.5 bg-neutral-100 rounded-lg">
              {[
                { id: 'browser', label: 'Browser', icon: Globe },
                { id: 'editor', label: 'Code', icon: Code2 },
                { id: 'artifacts', label: 'Artifacts', icon: Layers, badge: 3 },
                { id: 'relays', label: 'Relays', icon: Radio },
              ].map((tab) => {
                const Icon = tab.icon;
                const isActive = activeRightTab === tab.id;
                return (
                  <button
                    key={tab.id}
                    onClick={() => setActiveRightTab(tab.id as any)}
                    className={`flex-1 py-1 px-2 rounded-md text-xs font-medium transition-all text-center flex items-center justify-center gap-1.5 ${
                      isActive
                        ? 'bg-white text-neutral-900 shadow-2xs'
                        : 'text-neutral-500 hover:text-neutral-800'
                    }`}
                  >
                    <Icon className="w-3.5 h-3.5" />
                    <span>{tab.label}</span>
                    {tab.badge && (
                      <span className="w-3.5 h-3.5 rounded-full bg-neutral-900 text-white text-[9px] flex items-center justify-center font-bold">
                        {tab.badge}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>

            {/* Resize & Close */}
            <div className="flex items-center gap-1 shrink-0 pl-1">
              <button
                onClick={() => setIsRightPanelWide(!isRightPanelWide)}
                className="p-1.5 text-neutral-400 hover:text-neutral-900 hover:bg-neutral-100 rounded-md transition-colors"
                title={isRightPanelWide ? 'Standard width (440px)' : 'Wider width (580px)'}
              >
                {isRightPanelWide ? <Minimize2 className="w-3.5 h-3.5" /> : <Maximize2 className="w-3.5 h-3.5" />}
              </button>
              <button
                onClick={() => setIsRightPanelOpen(false)}
                className="p-1.5 text-neutral-400 hover:text-neutral-900 hover:bg-neutral-100 rounded-md transition-colors"
                title="Close canvas"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>

          {/* Canvas Content */}
          <div className="flex-1 overflow-y-auto p-4">
            {/* VIEW 1: IN-APP BROWSER PREVIEW */}
            {activeRightTab === 'browser' && (
              <div className="h-full">
                <BrowserPreviewView />
              </div>
            )}

            {/* VIEW 2: CODE EDITOR */}
            {activeRightTab === 'editor' && (
              <div className="h-full">
                <CodeEditorView onRunPreview={() => setActiveRightTab('browser')} />
              </div>
            )}

            {/* VIEW 3: ARTIFACTS OVERVIEW */}
            {activeRightTab === 'artifacts' && (
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-semibold text-neutral-900">Generated Artifacts (3)</span>
                  <button
                    onClick={() => setActiveRightTab('browser')}
                    className="text-xs text-neutral-600 hover:text-neutral-900 font-medium underline"
                  >
                    Open Live Preview &rarr;
                  </button>
                </div>

                {/* Artifact 1: Hero Concept */}
                <div className="rounded-lg border border-neutral-200 bg-white overflow-hidden shadow-2xs">
                  <div className="h-28 bg-neutral-900 relative flex items-center justify-center overflow-hidden">
                    <div className="text-neutral-200 text-center px-4">
                      <span className="text-lg font-semibold tracking-tight">Personal Notes Hub</span>
                      <p className="text-[11px] text-neutral-400">Concept Architecture</p>
                    </div>
                  </div>
                  <div className="p-3 flex items-center justify-between">
                    <div className="flex items-center gap-2.5">
                      <ImageIcon className="w-4 h-4 text-neutral-600" />
                      <div>
                        <div className="text-xs font-medium text-neutral-900">hero-concept.png</div>
                        <div className="text-[11px] text-neutral-400">1920 × 1080 • PNG • 1.8 MB</div>
                      </div>
                    </div>
                    <button className="p-1 text-neutral-400 hover:text-neutral-900">
                      <ExternalLink className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>

                {/* Artifact 2: Document */}
                <div className="p-3.5 rounded-lg border border-neutral-200 bg-white hover:border-neutral-400 transition-colors shadow-2xs flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <div className="w-7 h-7 rounded bg-neutral-100 text-neutral-700 flex items-center justify-center shrink-0">
                      <FileText className="w-3.5 h-3.5" />
                    </div>
                    <div>
                      <div className="text-xs font-medium text-neutral-900">Personal Knowledge Hub - PRD.md</div>
                      <div className="text-[11px] text-neutral-400">Product Requirements • 12.4 KB</div>
                    </div>
                  </div>
                  <ExternalLink className="w-3.5 h-3.5 text-neutral-400 hover:text-neutral-900" />
                </div>

                {/* Artifact 3: Git Patch */}
                <div
                  onClick={() => setActiveRightTab('editor')}
                  className="p-3.5 rounded-lg border border-neutral-200 bg-white hover:border-neutral-400 transition-colors shadow-2xs space-y-2 cursor-pointer"
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <GitBranch className="w-3.5 h-3.5 text-neutral-700" />
                      <span className="text-xs font-medium text-neutral-900">feat: personal knowledge hub</span>
                    </div>
                    <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-neutral-100 text-neutral-700">
                      +212 -8 lines
                    </span>
                  </div>
                  <div className="bg-neutral-900 text-neutral-300 font-mono text-[11px] p-2.5 rounded space-y-0.5 overflow-x-auto">
                    <div className="text-emerald-400">+ src/App.tsx</div>
                    <div className="text-emerald-400">+ src/features/notes/NoteList.tsx</div>
                    <div className="text-rose-400">- src/utils/mock.ts</div>
                  </div>
                </div>
              </div>
            )}

            {/* VIEW 4: RELAYS OVERVIEW */}
            {activeRightTab === 'relays' && (
              <div className="space-y-3">
                <div className="p-4 rounded-lg border border-neutral-200 bg-white shadow-2xs space-y-3">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Radio className="w-3.5 h-3.5 text-emerald-600 animate-pulse" />
                      <span className="text-xs font-semibold text-neutral-900">Relay Channels</span>
                    </div>
                    <span className="text-[10px] font-mono text-emerald-700 bg-emerald-50 px-2 py-0.5 rounded">
                      ONLINE
                    </span>
                  </div>

                  <div className="space-y-2 pt-1 text-xs">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="w-1.5 h-1.5 rounded-full bg-emerald-500"></span>
                        <span className="font-medium text-neutral-900">Telegram</span>
                        <span className="text-neutral-400">@odon_bot</span>
                      </div>
                      <span className="text-neutral-500">Connected</span>
                    </div>

                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="w-1.5 h-1.5 rounded-full bg-emerald-500"></span>
                        <span className="font-medium text-neutral-900">WhatsApp</span>
                        <span className="text-neutral-400">+1 415-xxx-7218</span>
                      </div>
                      <span className="text-neutral-500">Connected</span>
                    </div>
                  </div>

                  <div className="pt-2 border-t border-neutral-100 text-[10.5px] font-mono text-neutral-400 space-y-1">
                    <div>[10:21:08] Telegram relay: message sent (OK)</div>
                    <div>[10:21:09] WhatsApp relay: message sent (OK)</div>
                    <div>[10:24:12] Artifact created: hero-concept.png</div>
                  </div>
                </div>
              </div>
            )}
          </div>
        </aside>
      )}
    </div>
  );
}
