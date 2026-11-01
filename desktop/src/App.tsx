import React, { useState } from 'react';
import {
  MessageSquare,
  Code2,
  Sparkles,
  FileText,
  FolderOpen,
  Bot,
  Zap,
  Radio,
  Search,
  SlidersHorizontal,
  ChevronDown,
  Bell,
  Settings,
  Paperclip,
  Image as ImageIcon,
  Globe,
  Mic,
  Send,
  Plus,
  Play,
  GitBranch,
  CheckCircle2,
  Clock,
  MoreVertical,
  Layers,
  FileCode,
  FileCheck,
  ExternalLink,
  ShieldCheck,
  Terminal,
} from 'lucide-react';

export default function App() {
  const [activeWorkspace, setActiveWorkspace] = useState<'chat' | 'code' | 'create' | 'documents' | 'files' | 'agents' | 'automations'>('chat');
  const [activeRightTab, setActiveRightTab] = useState<'artifacts' | 'relays' | 'agents' | 'automations'>('artifacts');
  const [selectedModel, setSelectedModel] = useState('ODON Auto');
  const [inputMessage, setInputMessage] = useState('');
  const [hasTurn, setHasTurn] = useState(true);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#fbfbfc] text-[#1c1d22] font-sans antialiased">
      {/* ─────────────────────────────────────────────────────────────
          LEFT SIDEBAR: NAVIGATION & RECENT CHATS
      ───────────────────────────────────────────────────────────── */}
      <aside className="w-[280px] shrink-0 border-r border-[#eef0f3] bg-[#ffffff] flex flex-col justify-between select-none">
        {/* Top Header */}
        <div className="p-4 border-b border-[#f1f3f6]">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-[#6366f1] to-[#8b5cf6] flex items-center justify-center text-white shadow-sm shadow-indigo-100">
                <svg className="w-5 h-5 fill-current" viewBox="0 0 24 24">
                  <path d="M12 2C6.48 2 2 6.48 2 12c0 1.82.49 3.53 1.35 5L2 22l5.2-1.3c1.42.8 3.07 1.3 4.8 1.3 5.52 0 10-4.48 10-10S17.52 2 12 2zm1 14h-2v-2h2v2zm0-4h-2V7h2v5z"/>
                </svg>
              </div>
              <div>
                <h1 className="font-bold text-[15px] tracking-tight text-[#111217]">ODON</h1>
                <p className="text-[10px] text-[#8a8f9d] font-medium leading-none">Open Digital Orchestration</p>
              </div>
            </div>
            <button className="p-1.5 text-[#8a8f9d] hover:text-[#111217] hover:bg-[#f3f4f6] rounded-md transition-colors">
              <SlidersHorizontal className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Scrollable Workspaces & History */}
        <div className="flex-1 overflow-y-auto px-3 py-4 space-y-6">
          {/* New Chat Button */}
          <button
            onClick={() => setHasTurn(false)}
            className="w-full flex items-center justify-center gap-2 py-2 px-3 rounded-lg border border-[#e4e7ec] bg-[#ffffff] hover:bg-[#f8f9fb] text-[13px] font-semibold text-[#272932] shadow-xs transition-all active:scale-[0.99]"
          >
            <Plus className="w-4 h-4 text-[#6366f1]" />
            New Chat
          </button>

          {/* Workspaces Menu */}
          <div>
            <div className="text-[11px] font-bold tracking-wider text-[#9aa0ac] uppercase px-2 mb-2">Workspaces</div>
            <nav className="space-y-0.5">
              {[
                { id: 'chat', label: 'AI Chat', icon: MessageSquare },
                { id: 'code', label: 'Code', icon: Code2 },
                { id: 'create', label: 'Create', icon: Sparkles },
                { id: 'documents', label: 'Documents', icon: FileText },
                { id: 'files', label: 'Files', icon: FolderOpen },
                { id: 'agents', label: 'Agents', icon: Bot },
                { id: 'automations', label: 'Automations', icon: Zap },
              ].map((item) => {
                const Icon = item.icon;
                const isActive = activeWorkspace === item.id;
                return (
                  <button
                    key={item.id}
                    onClick={() => {
                      setActiveWorkspace(item.id as any);
                      if (item.id === 'chat') setHasTurn(true);
                    }}
                    className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg text-[13px] font-medium transition-colors ${
                      isActive
                        ? 'bg-[#eff2fe] text-[#4f46e5] font-semibold'
                        : 'text-[#4b5162] hover:bg-[#f5f6f9] hover:text-[#111217]'
                    }`}
                  >
                    <Icon className={`w-4 h-4 ${isActive ? 'text-[#4f46e5]' : 'text-[#848a9b]'}`} />
                    {item.label}
                  </button>
                );
              })}
            </nav>
          </div>

          {/* Recent Chats */}
          <div>
            <div className="flex items-center justify-between px-2 mb-2">
              <span className="text-[11px] font-bold tracking-wider text-[#9aa0ac] uppercase">Recent Chats</span>
              <button className="text-[#848a9b] hover:text-[#111217]">
                <Plus className="w-3.5 h-3.5" />
              </button>
            </div>
            <div className="space-y-1">
              {[
                { title: 'Build a project with code, doc...', time: 'Now', active: true },
                { title: 'Design product landing images', time: '2h', active: false },
                { title: 'Automate daily report', time: '5h', active: false },
                { title: 'Integrate Telegram & WhatsApp', time: '1d', active: false },
                { title: 'ODON product brief', time: '2d', active: false },
              ].map((c, i) => (
                <button
                  key={i}
                  onClick={() => setHasTurn(true)}
                  className={`w-full flex items-center justify-between px-2.5 py-2 rounded-md text-[12.5px] transition-colors text-left ${
                    c.active
                      ? 'bg-[#eff2fe]/60 text-[#4f46e5] font-medium'
                      : 'text-[#4b5162] hover:bg-[#f7f8fa] hover:text-[#111217]'
                  }`}
                >
                  <span className="truncate flex-1 mr-2">{c.title}</span>
                  <span className="text-[11px] text-[#9ca3af] shrink-0">{c.time}</span>
                </button>
              ))}
              <button className="w-full text-left px-2.5 py-1.5 text-[11.5px] text-[#6366f1] font-semibold hover:underline">
                View All Chats &rarr;
              </button>
            </div>
          </div>
        </div>

        {/* User Profile */}
        <div className="p-3 border-t border-[#f1f3f6]">
          <div className="flex items-center justify-between p-1.5 rounded-lg hover:bg-[#f7f8fa] cursor-pointer transition-colors">
            <div className="flex items-center gap-2.5">
              <div className="relative">
                <img
                  src="https://images.unsplash.com/photo-1534528741775-53994a69daeb?w=100&auto=format&fit=crop&q=80"
                  alt="Avatar"
                  className="w-8 h-8 rounded-full object-cover"
                />
                <span className="absolute bottom-0 right-0 w-2.5 h-2.5 rounded-full bg-emerald-500 ring-2 ring-white"></span>
              </div>
              <div>
                <div className="flex items-center gap-1.5">
                  <span className="text-[13px] font-semibold text-[#1c1d22]">Alex Chen</span>
                  <span className="px-1.5 py-0.2 rounded text-[10px] font-bold bg-[#eff2fe] text-[#4f46e5]">Pro</span>
                </div>
                <span className="text-[11px] text-[#8a8f9d]">alex@odon.network</span>
              </div>
            </div>
            <ChevronDown className="w-4 h-4 text-[#8a8f9d]" />
          </div>
        </div>
      </aside>

      {/* ─────────────────────────────────────────────────────────────
          CENTER COLUMN: AI CONVERSATION & WORKSPACE
      ───────────────────────────────────────────────────────────── */}
      <main className="flex-1 flex flex-col bg-[#ffffff] overflow-hidden">
        {/* Header */}
        <header className="h-14 border-b border-[#eef0f3] px-6 flex items-center justify-between shrink-0 bg-[#ffffff]">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2 text-[#1c1d22]">
              <MessageSquare className="w-4 h-4 text-[#6366f1]" />
              <h2 className="text-[14px] font-bold text-[#111217]">AI Chat</h2>
            </div>
            <span className="text-[#d0d5dd]">|</span>
            <p className="text-[12px] text-[#6b7280]">Collaborate with ODON to code, create, write, and automate — all in one place.</p>
          </div>

          <div className="flex items-center gap-2.5">
            {/* Model Selector */}
            <div className="relative flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[#e4e7ec] bg-[#ffffff] hover:bg-[#f9fafb] text-[12.5px] font-semibold text-[#344054] cursor-pointer shadow-2xs">
              <Sparkles className="w-3.5 h-3.5 text-[#6366f1]" />
              <span>Model: {selectedModel}</span>
              <ChevronDown className="w-3.5 h-3.5 text-[#98a2b3] ml-1" />
            </div>

            <button className="p-2 text-[#667085] hover:text-[#111217] hover:bg-[#f2f4f7] rounded-lg transition-colors">
              <Bell className="w-4 h-4" />
            </button>
            <button className="p-2 text-[#667085] hover:text-[#111217] hover:bg-[#f2f4f7] rounded-lg transition-colors">
              <Settings className="w-4 h-4" />
            </button>
          </div>
        </header>

        {/* Chat Feed */}
        <div className="flex-1 overflow-y-auto px-8 py-6 space-y-6">
          {!hasTurn ? (
            /* Empty State / What can ODON do for you */
            <div className="h-full flex flex-col items-center justify-center max-w-2xl mx-auto text-center space-y-8 my-auto">
              <div>
                <div className="w-12 h-12 rounded-2xl bg-indigo-50 flex items-center justify-center text-[#6366f1] mx-auto mb-4">
                  <Sparkles className="w-6 h-6" />
                </div>
                <h2 className="text-2xl font-bold text-[#111217] mb-2 tracking-tight">What can ODON do for you?</h2>
                <p className="text-[14px] text-[#6b7280]">Code, create, automate, and more with autonomous orchestration.</p>
              </div>

              <div className="grid grid-cols-2 gap-3.5 w-full">
                {[
                  { title: 'Build an app', desc: 'React 19, Rust, TypeScript fullstack', icon: Code2, prompt: 'Build a minimal web app with React 19' },
                  { title: 'Create an image', desc: 'Prompts to 4K Flux / Midjourney cards', icon: ImageIcon, prompt: 'Generate high quality project landing assets' },
                  { title: 'Make a report', desc: 'PRD, competitive audit, executive brief', icon: FileText, prompt: 'Write a comprehensive PRD for our product' },
                  { title: 'Run an agent', desc: 'Autonomous multi-step research pipeline', icon: Bot, prompt: 'Run a research agent on knowledge graphs' },
                ].map((card, i) => {
                  const Icon = card.icon;
                  return (
                    <button
                      key={i}
                      onClick={() => {
                        setInputMessage(card.prompt);
                        setHasTurn(true);
                      }}
                      className="p-4 rounded-xl border border-[#e4e7ec] bg-[#ffffff] hover:border-[#6366f1]/40 hover:shadow-sm text-left transition-all group"
                    >
                      <Icon className="w-5 h-5 text-[#6366f1] mb-2.5 group-hover:scale-105 transition-transform" />
                      <div className="text-[13.5px] font-bold text-[#111217]">{card.title}</div>
                      <div className="text-[12px] text-[#6b7280] mt-0.5">{card.desc}</div>
                    </button>
                  );
                })}
              </div>
            </div>
          ) : (
            /* Active Conversational Turn (as in screenshot) */
            <div className="max-w-3xl mx-auto space-y-6">
              {/* User Bubble */}
              <div className="flex items-start gap-3.5 justify-end">
                <div className="max-w-xl bg-[#f4f5f8] text-[#1f242e] rounded-2xl rounded-tr-xs px-4 py-3 text-[13.5px] leading-relaxed shadow-2xs">
                  <div className="font-semibold text-[11.5px] text-[#6b7280] mb-1">Alex Chen • 10:24 AM</div>
                  <p>
                    I want to build a small "personal knowledge hub" project. Please:
                    <br />1) Write the core code (a minimal web app with React 19) for managing notes.
                    <br />2) Create a product requirement document (PRD) for it.
                    <br />3) Generate a few high-quality images for the project (e.g., a hero illustration and a UI preview).
                    <br />Organize the outputs clearly and let me know what you've done.
                  </p>
                </div>
                <div className="w-8 h-8 rounded-full overflow-hidden shrink-0 mt-1">
                  <img src="https://images.unsplash.com/photo-1534528741775-53994a69daeb?w=100&auto=format&fit=crop&q=80" alt="Alex" />
                </div>
              </div>

              {/* ODON Assistant Response */}
              <div className="flex items-start gap-3.5">
                <div className="w-8 h-8 rounded-xl bg-gradient-to-br from-[#6366f1] to-[#8b5cf6] flex items-center justify-center text-white shrink-0 mt-1 shadow-sm shadow-indigo-100">
                  <Bot className="w-4 h-4" />
                </div>
                <div className="flex-1 space-y-4">
                  <div className="flex items-center gap-2">
                    <span className="text-[13px] font-bold text-[#111217]">ODON</span>
                    <span className="px-1.5 py-0.2 rounded text-[10px] font-bold bg-[#eff2fe] text-[#4f46e5]">AI</span>
                    <span className="text-[11px] text-[#9ca3af]">10:24 AM</span>
                  </div>

                  {/* Flowing conversational text */}
                  <div className="text-[13.5px] text-[#2c3240] leading-relaxed space-y-3">
                    <p>
                      Got it! I'll help you build the "Personal Knowledge Hub" end-to-end — including the code (React 19), a product requirements document (PRD), and high-quality project images. Here's what I've done:
                    </p>

                    <div className="space-y-2 py-1">
                      <div className="flex items-start gap-2.5">
                        <CheckCircle2 className="w-4 h-4 text-emerald-500 mt-0.5 shrink-0" />
                        <div><strong className="text-[#111217]">Code</strong>: A minimal React 19 app (Vite + TypeScript) for managing notes, with a clean UI, search, tags, and local storage.</div>
                      </div>
                      <div className="flex items-start gap-2.5">
                        <CheckCircle2 className="w-4 h-4 text-emerald-500 mt-0.5 shrink-0" />
                        <div><strong className="text-[#111217]">Document</strong>: A complete PRD covering product goals, features, user flows, and technical scope.</div>
                      </div>
                      <div className="flex items-start gap-2.5">
                        <CheckCircle2 className="w-4 h-4 text-emerald-500 mt-0.5 shrink-0" />
                        <div><strong className="text-[#111217]">Images</strong>: Generated a hero illustration and a UI preview for the project.</div>
                      </div>
                    </div>

                    <div className="pt-2">
                      <div className="flex items-center gap-1.5 text-[12px] font-bold text-[#6b7280] uppercase tracking-wider mb-2">
                        <Layers className="w-3.5 h-3.5 text-[#6366f1]" />
                        Outputs
                      </div>

                      {/* Interactive Output Cards */}
                      <div className="grid grid-cols-3 gap-2.5">
                        <div className="p-3 rounded-xl border border-[#e4e7ec] bg-[#fafbfc] hover:border-[#6366f1]/40 transition-all cursor-pointer">
                          <div className="flex items-center justify-between mb-1.5">
                            <Code2 className="w-4 h-4 text-[#6366f1]" />
                            <span className="w-2 h-2 rounded-full bg-emerald-500"></span>
                          </div>
                          <div className="text-[12.5px] font-bold text-[#111217] truncate">Code: personal-knowledge-hub</div>
                          <div className="text-[11px] text-[#6b7280] mt-0.5">React 19 • TypeScript • Vite</div>
                        </div>

                        <div className="p-3 rounded-xl border border-[#e4e7ec] bg-[#fafbfc] hover:border-[#6366f1]/40 transition-all cursor-pointer">
                          <div className="flex items-center justify-between mb-1.5">
                            <FileText className="w-4 h-4 text-blue-500" />
                            <span className="w-2 h-2 rounded-full bg-emerald-500"></span>
                          </div>
                          <div className="text-[12.5px] font-bold text-[#111217] truncate">PRD: Personal Knowledge Hub</div>
                          <div className="text-[11px] text-[#6b7280] mt-0.5">Product Requirements Document</div>
                        </div>

                        <div className="p-3 rounded-xl border border-[#e4e7ec] bg-[#fafbfc] hover:border-[#6366f1]/40 transition-all cursor-pointer">
                          <div className="flex items-center justify-between mb-1.5">
                            <ImageIcon className="w-4 h-4 text-purple-500" />
                            <span className="w-2 h-2 rounded-full bg-emerald-500"></span>
                          </div>
                          <div className="text-[12.5px] font-bold text-[#111217] truncate">Images (2)</div>
                          <div className="text-[11px] text-[#6b7280] mt-0.5">Hero • UI Preview</div>
                        </div>
                      </div>
                    </div>

                    <p className="text-[13px] text-[#4b5563] pt-2">
                      You can view, download, or continue iterating on any of these. Would you like me to:
                    </p>

                    {/* Action Chips */}
                    <div className="flex flex-wrap gap-2 pt-1">
                      <button className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[#e4e7ec] bg-white hover:bg-[#f9fafb] text-[12.5px] font-semibold text-[#344054] shadow-2xs transition-colors">
                        <Play className="w-3.5 h-3.5 text-[#6366f1]" />
                        Run the project and preview it
                      </button>
                      <button className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[#e4e7ec] bg-white hover:bg-[#f9fafb] text-[12.5px] font-semibold text-[#344054] shadow-2xs transition-colors">
                        <GitBranch className="w-3.5 h-3.5 text-[#111217]" />
                        Create a Git repository
                      </button>
                      <button className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[#e4e7ec] bg-white hover:bg-[#f9fafb] text-[12.5px] font-semibold text-[#344054] shadow-2xs transition-colors">
                        <Zap className="w-3.5 h-3.5 text-amber-500" />
                        Turn this into an automation
                      </button>
                      <button className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[#e4e7ec] bg-white hover:bg-[#f9fafb] text-[12.5px] font-semibold text-[#344054] shadow-2xs transition-colors">
                        <FileCode className="w-3.5 h-3.5 text-[#6366f1]" />
                        Refine the PRD
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Bottom Input Area */}
        <div className="p-4 border-t border-[#eef0f3] bg-[#ffffff]">
          <div className="max-w-3xl mx-auto rounded-2xl border border-[#e4e7ec] bg-[#ffffff] shadow-sm focus-within:border-[#6366f1] focus-within:ring-2 focus-within:ring-indigo-100 transition-all p-3">
            {/* Top Toolbar in Input */}
            <div className="flex items-center justify-between pb-2 mb-2 border-b border-[#f3f4f6]">
              <div className="flex items-center gap-2">
                <button className="flex items-center gap-1 px-2 py-1 rounded text-[11.5px] font-semibold text-[#4b5563] hover:bg-[#f3f4f6]">
                  <SlidersHorizontal className="w-3 h-3 text-[#6366f1]" />
                  Tools
                  <ChevronDown className="w-3 h-3 text-[#9ca3af]" />
                </button>
                <button className="flex items-center gap-1 px-2 py-1 rounded text-[11.5px] font-semibold text-[#4b5563] hover:bg-[#f3f4f6]">
                  <Sparkles className="w-3 h-3 text-purple-500" />
                  Mode: General
                  <ChevronDown className="w-3 h-3 text-[#9ca3af]" />
                </button>
              </div>

              <span className="flex items-center gap-1 text-[11px] font-semibold text-[#6366f1] bg-indigo-50/80 px-2 py-0.5 rounded-full">
                React 19 Ready
              </span>
            </div>

            {/* Textarea */}
            <textarea
              value={inputMessage}
              onChange={(e) => setInputMessage(e.target.value)}
              placeholder="Message ODON... (e.g., build, create, analyze, automate...)"
              className="w-full resize-none outline-none text-[13.5px] text-[#111217] placeholder-[#9ca3af] bg-transparent min-h-[50px]"
              rows={2}
            />

            {/* Bottom Actions Bar */}
            <div className="flex items-center justify-between pt-2">
              <div className="flex items-center gap-1 text-[#667085]">
                <button className="p-1.5 hover:text-[#111217] hover:bg-[#f2f4f7] rounded-md transition-colors" title="Attach file">
                  <Paperclip className="w-4 h-4" />
                </button>
                <button className="p-1.5 hover:text-[#111217] hover:bg-[#f2f4f7] rounded-md transition-colors" title="Generate or upload image">
                  <ImageIcon className="w-4 h-4" />
                </button>
                <button className="p-1.5 hover:text-[#111217] hover:bg-[#f2f4f7] rounded-md transition-colors" title="Search web">
                  <Globe className="w-4 h-4" />
                </button>
                <button className="p-1.5 hover:text-[#111217] hover:bg-[#f2f4f7] rounded-md transition-colors" title="Code snippet">
                  <Code2 className="w-4 h-4" />
                </button>
              </div>

              <div className="flex items-center gap-2">
                <button className="p-1.5 text-[#667085] hover:text-[#111217] hover:bg-[#f2f4f7] rounded-md transition-colors">
                  <Mic className="w-4 h-4" />
                </button>
                <button
                  onClick={() => {
                    if (inputMessage.trim()) {
                      setHasTurn(true);
                      setInputMessage('');
                    }
                  }}
                  className="w-8 h-8 rounded-lg bg-[#4f46e5] hover:bg-[#4338ca] text-white flex items-center justify-center transition-all shadow-xs"
                >
                  <Send className="w-4 h-4" />
                </button>
              </div>
            </div>
          </div>
        </div>
      </main>

      {/* ─────────────────────────────────────────────────────────────
          RIGHT DRAWER: ARTIFACTS, RELAYS & AGENTS
      ───────────────────────────────────────────────────────────── */}
      <aside className="w-[360px] shrink-0 border-l border-[#eef0f3] bg-[#fcfdfe] flex flex-col justify-between overflow-hidden">
        {/* Top Tab Bar */}
        <div className="p-3 border-b border-[#eef0f3] bg-[#ffffff]">
          <div className="flex items-center justify-between p-1 bg-[#f2f4f7] rounded-lg">
            {[
              { id: 'artifacts', label: 'Artifacts', badge: 3 },
              { id: 'relays', label: 'Relays' },
              { id: 'agents', label: 'Agents' },
              { id: 'automations', label: 'Automations' },
            ].map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveRightTab(tab.id as any)}
                className={`flex-1 py-1 px-2 rounded-md text-[11.5px] font-semibold transition-all text-center flex items-center justify-center gap-1 ${
                  activeRightTab === tab.id
                    ? 'bg-white text-[#111217] shadow-xs'
                    : 'text-[#667085] hover:text-[#111217]'
                }`}
              >
                {tab.label}
                {tab.badge && (
                  <span className="w-4 h-4 rounded-full bg-[#6366f1] text-white text-[9.5px] flex items-center justify-center">
                    {tab.badge}
                  </span>
                )}
              </button>
            ))}
          </div>
        </div>

        {/* Drawer Content */}
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          {activeRightTab === 'artifacts' && (
            <>
              <div className="flex items-center justify-between">
                <span className="text-[12px] font-bold text-[#111217]">Artifacts (Outputs from turn)</span>
                <button className="text-[11px] text-[#6366f1] font-semibold hover:underline">Download All</button>
              </div>

              {/* Artifact 1: Image Card */}
              <div className="rounded-xl border border-[#e4e7ec] bg-white overflow-hidden shadow-2xs group hover:border-[#6366f1]/40 transition-all">
                <div className="h-32 bg-gradient-to-tr from-[#6366f1] via-[#8b5cf6] to-[#ec4899] relative flex items-center justify-center overflow-hidden">
                  <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_top,_var(--tw-gradient-stops))] from-indigo-400/30 via-transparent to-black/40"></div>
                  <div className="text-white text-center z-10 px-4">
                    <span className="text-2xl font-bold tracking-tight">Personal Hub</span>
                    <p className="text-[11px] text-white/80">Hero Concept Illustration</p>
                  </div>
                </div>
                <div className="p-3 flex items-center justify-between">
                  <div className="flex items-center gap-2.5">
                    <ImageIcon className="w-4 h-4 text-purple-500" />
                    <div>
                      <div className="text-[12.5px] font-bold text-[#111217]">hero-illustration.png</div>
                      <div className="text-[11px] text-[#8a8f9d]">1920 × 1080 • PNG • 1.8 MB</div>
                    </div>
                  </div>
                  <button className="p-1 text-[#8a8f9d] hover:text-[#111217]">
                    <MoreVertical className="w-4 h-4" />
                  </button>
                </div>
              </div>

              {/* Artifact 2: Document Card */}
              <div className="p-3.5 rounded-xl border border-[#e4e7ec] bg-white hover:border-[#6366f1]/40 transition-all shadow-2xs flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-8 h-8 rounded-lg bg-blue-50 text-blue-600 flex items-center justify-center shrink-0">
                    <FileText className="w-4 h-4" />
                  </div>
                  <div>
                    <div className="text-[12.5px] font-bold text-[#111217]">Personal Knowledge Hub - PRD.md</div>
                    <div className="text-[11px] text-[#8a8f9d]">Product Requirements Document • 12.4 KB</div>
                  </div>
                </div>
                <ExternalLink className="w-3.5 h-3.5 text-[#9ca3af] hover:text-[#111217]" />
              </div>

              {/* Artifact 3: Git Patch / Code Card */}
              <div className="p-3.5 rounded-xl border border-[#e4e7ec] bg-white hover:border-[#6366f1]/40 transition-all shadow-2xs space-y-2">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <GitBranch className="w-4 h-4 text-emerald-600" />
                    <span className="text-[12.5px] font-bold text-[#111217]">feat: personal knowledge hub</span>
                  </div>
                  <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-emerald-50 text-emerald-700 font-semibold">
                    +212 -8 lines • 6 files
                  </span>
                </div>
                <div className="bg-[#18191f] text-[#d4d4d8] font-mono text-[11px] p-2.5 rounded-lg space-y-0.5 overflow-x-auto">
                  <div className="text-emerald-400">+ src/App.tsx</div>
                  <div className="text-emerald-400">+ src/features/notes/NoteList.tsx</div>
                  <div className="text-rose-400">- src/utils/mock.ts</div>
                </div>
              </div>
            </>
          )}

          {/* Relay Status Panel */}
          <div className="p-4 rounded-xl border border-[#e4e7ec] bg-white shadow-2xs space-y-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Radio className="w-4 h-4 text-emerald-500 animate-pulse" />
                <span className="text-[12px] font-bold text-[#111217]">Relay Status</span>
              </div>
              <span className="text-[10.5px] font-bold text-emerald-600 bg-emerald-50 px-2 py-0.5 rounded-full">
                All Online
              </span>
            </div>

            <div className="space-y-2 pt-1">
              <div className="flex items-center justify-between text-[12px]">
                <div className="flex items-center gap-2">
                  <div className="w-2 h-2 rounded-full bg-emerald-500"></div>
                  <span className="font-semibold text-[#111217]">Telegram</span>
                  <span className="text-[11px] text-[#6b7280]">@odon_bot</span>
                </div>
                <span className="text-[11px] text-emerald-600 font-medium">Online</span>
              </div>

              <div className="flex items-center justify-between text-[12px]">
                <div className="flex items-center gap-2">
                  <div className="w-2 h-2 rounded-full bg-emerald-500"></div>
                  <span className="font-semibold text-[#111217]">WhatsApp</span>
                  <span className="text-[11px] text-[#6b7280]">+1 415-xxx-7218</span>
                </div>
                <span className="text-[11px] text-emerald-600 font-medium">Online</span>
              </div>
            </div>

            {/* Live activity log */}
            <div className="pt-2 border-t border-[#f3f4f6] text-[10.5px] font-mono text-[#6b7280] space-y-1">
              <div>[10:21:08] Telegram relay: message sent (OK)</div>
              <div>[10:21:09] WhatsApp relay: message sent (OK)</div>
            </div>
          </div>

          {/* Active Agents Mini-card */}
          <div className="p-3.5 rounded-xl border border-[#e4e7ec] bg-white shadow-2xs space-y-2.5">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Bot className="w-4 h-4 text-[#6366f1]" />
                <span className="text-[12px] font-bold text-[#111217]">Active Agents</span>
              </div>
              <span className="text-[10.5px] font-semibold text-[#6366f1]">2 Active</span>
            </div>
            <div className="space-y-1.5 text-[12px]">
              <div className="flex items-center justify-between">
                <span className="text-[#374151]">Research Agent</span>
                <span className="text-[10px] font-semibold text-emerald-600 bg-emerald-50 px-1.5 py-0.2 rounded">Running</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-[#374151]">Content Agent</span>
                <span className="text-[10px] font-semibold text-[#6b7280] bg-gray-50 px-1.5 py-0.2 rounded">Idle</span>
              </div>
            </div>
          </div>

          {/* Automations Mini-card */}
          <div className="p-3.5 rounded-xl border border-[#e4e7ec] bg-white shadow-2xs space-y-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Zap className="w-4 h-4 text-amber-500" />
                <span className="text-[12px] font-bold text-[#111217]">Daily Knowledge Sync</span>
              </div>
              <div className="w-7 h-4 bg-[#6366f1] rounded-full relative cursor-pointer">
                <div className="w-3 h-3 bg-white rounded-full absolute right-0.5 top-0.5"></div>
              </div>
            </div>
            <p className="text-[11px] text-[#6b7280]">Next run: Today 20:00 (Cron)</p>
          </div>
        </div>
      </aside>
    </div>
  );
}
