import React, { useState } from 'react';
import {
  MessageSquare,
  Code2,
  Image as ImageIcon,
  FileText,
  Folder,
  Bot,
  Share2,
  Settings,
  Plus,
  Search,
  Bell,
  PanelRightClose,
  PanelRightOpen,
  ChevronDown,
  Paperclip,
  Globe,
  ArrowUp,
  ExternalLink,
  Radio,
  Activity,
  ChevronRight,
  Video,
  Minus,
  Square,
  X,
  Sparkles,
  Layers,
  ArrowLeft,
  CheckCircle2,
  Terminal,
} from 'lucide-react';
import { CatLogo } from './components/CatLogo';
import { CodeEditorView } from './components/CodeEditorView';
import { BrowserPreviewView } from './components/BrowserPreviewView';

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: string;
  showAppCard?: boolean;
}

export default function App() {
  const [activeNav, setActiveNav] = useState('chat');
  const [isRightPanelOpen, setIsRightPanelOpen] = useState(true);
  const [rightPanelView, setRightPanelView] = useState<'artifacts' | 'editor' | 'browser'>('artifacts');
  const [selectedModel, setSelectedModel] = useState('Auto (GPT-4o)');
  const [inputMessage, setInputMessage] = useState('');
  
  // Chat state: false = Welcome/Hero screen (matches screenshot), true = active chat
  const [inChatMode, setInChatMode] = useState(false);
  const [messages, setMessages] = useState<ChatMessage[]>([]);

  // Start a new chat (return to hero)
  const handleNewChat = () => {
    setInChatMode(false);
    setMessages([]);
    setInputMessage('');
  };

  // Trigger prompt from hero action cards
  const handleActionCard = (title: string, promptText: string) => {
    const userMsg: ChatMessage = {
      id: Date.now().toString(),
      role: 'user',
      content: promptText,
      timestamp: 'Just now',
    };
    const assistantMsg: ChatMessage = {
      id: (Date.now() + 1).toString(),
      role: 'assistant',
      content: `I've prepared the ${title.toLowerCase()} workspace for you with clean modular architecture and React 19 components.`,
      timestamp: 'Just now',
      showAppCard: title.includes('app') || title.includes('code'),
    };
    setMessages([userMsg, assistantMsg]);
    setInChatMode(true);
  };

  // Submit prompt from input box
  const handleSendMessage = () => {
    if (!inputMessage.trim()) return;
    const userMsg: ChatMessage = {
      id: Date.now().toString(),
      role: 'user',
      content: inputMessage,
      timestamp: 'Just now',
    };
    const assistantMsg: ChatMessage = {
      id: (Date.now() + 1).toString(),
      role: 'assistant',
      content: `I have analyzed your request: "${inputMessage}". The project structure has been generated with responsive layout and type-safe invariants.`,
      timestamp: 'Just now',
      showAppCard: true,
    };
    setMessages((prev) => [...prev, userMsg, assistantMsg]);
    setInputMessage('');
    setInChatMode(true);
  };

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-[#f4f5f8] text-neutral-800 font-sans select-none">
      {/* ─────────────────────────────────────────────────────────────
          1. WINDOW TITLE BAR (DESKTOP CHROME)
      ───────────────────────────────────────────────────────────── */}
      <header className="h-8 shrink-0 flex items-center justify-between px-3 text-xs text-neutral-600 bg-[#f4f5f8]">
        {/* Left: App Logo & Name */}
        <div className="flex items-center gap-1.5 font-medium text-neutral-800 tracking-tight">
          <CatLogo className="w-4 h-4 text-neutral-800" strokeWidth={2.4} />
          <span className="font-semibold text-[13px] tracking-wide">ODON</span>
        </div>

        {/* Center: Draggable Spacer */}
        <div className="flex-1 h-full mx-4" />

        {/* Right: Window Controls */}
        <div className="flex items-center gap-1">
          <button className="w-7 h-6 flex items-center justify-center rounded hover:bg-neutral-200/70 text-neutral-600 transition-colors">
            <Minus className="w-3.5 h-3.5" />
          </button>
          <button className="w-7 h-6 flex items-center justify-center rounded hover:bg-neutral-200/70 text-neutral-600 transition-colors">
            <Square className="w-3 h-3" />
          </button>
          <button className="w-7 h-6 flex items-center justify-center rounded hover:bg-red-500 hover:text-white text-neutral-600 transition-colors">
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </header>

      {/* ─────────────────────────────────────────────────────────────
          2. MAIN 3-COLUMN WORKSPACE (ROUNDED WHITE CARDS)
      ───────────────────────────────────────────────────────────── */}
      <div className="flex-1 flex overflow-hidden p-2.5 pt-0 gap-2.5">
        
        {/* ───────────────────────────────────────────────────────────
            LEFT SIDEBAR CARD
        ─────────────────────────────────────────────────────────── */}
        <aside className="w-[250px] shrink-0 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col justify-between p-3.5 overflow-hidden">
          {/* Top Section */}
          <div className="flex flex-col overflow-y-auto">
            {/* Header: Cat Logo + ODON + Subtitle */}
            <div className="flex items-center gap-2.5 px-1 py-1">
              <CatLogo className="w-8 h-8 text-neutral-900 shrink-0" strokeWidth={2.2} />
              <div className="leading-tight">
                <div className="font-bold text-base text-neutral-900 tracking-tight">ODON</div>
                <div className="text-[10px] text-neutral-500 font-normal leading-tight">
                  Open Digital Orchestration Network
                </div>
              </div>
            </div>

            {/* + New Chat Button */}
            <button
              onClick={handleNewChat}
              className="mt-3.5 w-full flex items-center justify-between px-3 py-2 rounded-xl border border-neutral-200 hover:border-neutral-300 hover:bg-neutral-50 transition-all text-xs font-medium text-neutral-800 shadow-2xs group"
            >
              <div className="flex items-center gap-1.5 font-semibold">
                <Plus className="w-3.5 h-3.5 text-neutral-700 group-hover:scale-110 transition-transform" />
                <span>New Chat</span>
              </div>
              <span className="text-[11px] font-mono text-neutral-400 bg-neutral-100 px-1.5 py-0.5 rounded">
                Ctrl N
              </span>
            </button>

            {/* Navigation Menu */}
            <nav className="mt-3.5 space-y-0.5">
              <button
                onClick={() => { setActiveNav('chat'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${
                  activeNav === 'chat'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                }`}
              >
                <MessageSquare className={`w-4 h-4 ${activeNav === 'chat' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>AI Chat</span>
              </button>

              <button
                onClick={() => { setActiveNav('code'); setRightPanelView('editor'); setIsRightPanelOpen(true); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${
                  activeNav === 'code'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                }`}
              >
                <Code2 className="w-4 h-4 text-neutral-600" />
                <span>Code</span>
              </button>

              <button
                onClick={() => setActiveNav('create')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${
                  activeNav === 'create'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                }`}
              >
                <ImageIcon className="w-4 h-4 text-neutral-600" />
                <span>Create</span>
              </button>

              <button
                onClick={() => setActiveNav('documents')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${
                  activeNav === 'documents'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                }`}
              >
                <FileText className="w-4 h-4 text-neutral-600" />
                <span>Documents</span>
              </button>

              <button
                onClick={() => setActiveNav('files')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${
                  activeNav === 'files'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                }`}
              >
                <Folder className="w-4 h-4 text-neutral-600" />
                <span>Files</span>
              </button>

              <button
                onClick={() => setActiveNav('agents')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${
                  activeNav === 'agents'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                }`}
              >
                <Bot className="w-4 h-4 text-neutral-600" />
                <span>Agents</span>
              </button>

              <button
                onClick={() => setActiveNav('automations')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${
                  activeNav === 'automations'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                }`}
              >
                <Share2 className="w-4 h-4 text-neutral-600" />
                <span>Automations</span>
              </button>
            </nav>

            {/* Recent Section */}
            <div className="mt-5 pt-3 border-t border-neutral-100">
              <div className="text-[11px] font-medium text-neutral-400 px-1 mb-2">Recent</div>
              <div className="space-y-1">
                {[
                  { title: 'Rencana arsitektur ODON', time: '2h ago' },
                  { title: 'Refactor modul agent (Rust)', time: '4h ago' },
                  { title: 'Buat gambar ilustrasi produk', time: 'Yesterday' },
                  { title: 'Konversi PDF ke DOCX', time: 'Yesterday' },
                ].map((item, idx) => (
                  <button
                    key={idx}
                    onClick={() => {
                      setInChatMode(true);
                      setMessages([
                        {
                          id: 'hist-1',
                          role: 'user',
                          content: item.title,
                          timestamp: item.time,
                        },
                        {
                          id: 'hist-2',
                          role: 'assistant',
                          content: `Here is the progress and artifacts generated for "${item.title}". All components are compiled and verified.`,
                          timestamp: item.time,
                          showAppCard: true,
                        },
                      ]);
                    }}
                    className="w-full flex items-center justify-between px-2 py-1.5 rounded-lg text-xs text-neutral-700 hover:bg-neutral-100/80 transition-colors group text-left"
                  >
                    <div className="flex items-center gap-2 truncate">
                      <MessageSquare className="w-3.5 h-3.5 text-neutral-400 group-hover:text-neutral-600 shrink-0" />
                      <span className="truncate">{item.title}</span>
                    </div>
                    <span className="text-[10px] text-neutral-400 shrink-0 ml-1">{item.time}</span>
                  </button>
                ))}
              </div>
              <button className="text-[11px] text-neutral-400 hover:text-neutral-700 px-2 py-1 mt-1 transition-colors">
                Show more...
              </button>
            </div>
          </div>

          {/* User Profile at Bottom */}
          <div className="pt-3 border-t border-neutral-100 flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div className="w-7 h-7 rounded-full bg-blue-600 text-white flex items-center justify-center font-semibold text-xs tracking-tight shadow-2xs">
                OD
              </div>
              <div className="leading-tight">
                <div className="font-semibold text-xs text-neutral-900">Odon User</div>
                <div className="text-[10px] text-neutral-400">Pro • Local</div>
              </div>
            </div>
            <button className="p-1.5 text-neutral-500 hover:text-neutral-800 hover:bg-neutral-100 rounded-lg transition-colors">
              <Settings className="w-4 h-4" />
            </button>
          </div>
        </aside>

        {/* ───────────────────────────────────────────────────────────
            CENTER MAIN CHAT / HERO CARD
        ─────────────────────────────────────────────────────────── */}
        <main className="flex-1 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col justify-between overflow-hidden relative">
          
          {/* Top Bar inside Center Card */}
          <div className="h-12 border-b border-neutral-100 px-4 flex items-center justify-between shrink-0">
            {/* Model Selector Pill */}
            <div className="flex items-center gap-2">
              <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-full border border-neutral-200/90 text-xs font-medium text-neutral-800 hover:bg-neutral-50 transition-colors shadow-2xs cursor-pointer">
                <Sparkles className="w-3.5 h-3.5 text-neutral-700" />
                <span>{selectedModel}</span>
                <ChevronDown className="w-3 h-3 text-neutral-400 ml-0.5" />
              </div>

              {inChatMode && (
                <button
                  onClick={handleNewChat}
                  className="text-xs text-blue-600 hover:text-blue-800 font-medium px-2 py-1 rounded-md hover:bg-blue-50 transition-colors"
                >
                  ← Back to Home
                </button>
              )}
            </div>

            {/* Action Buttons on Right */}
            <div className="flex items-center gap-1">
              <button className="p-2 text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-lg transition-colors">
                <Search className="w-4 h-4" />
              </button>
              <button className="p-2 text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-lg transition-colors">
                <Bell className="w-4 h-4" />
              </button>
              <button
                onClick={() => setIsRightPanelOpen(!isRightPanelOpen)}
                className={`p-2 rounded-lg transition-colors ${
                  isRightPanelOpen
                    ? 'text-neutral-700 hover:bg-neutral-100'
                    : 'text-neutral-400 hover:text-neutral-800 hover:bg-neutral-100'
                }`}
                title={isRightPanelOpen ? 'Collapse panel' : 'Open panel'}
              >
                {isRightPanelOpen ? <PanelRightClose className="w-4 h-4" /> : <PanelRightOpen className="w-4 h-4" />}
              </button>
            </div>
          </div>

          {/* Center Content Area */}
          <div className="flex-1 overflow-y-auto px-6 py-6 flex flex-col justify-center items-center">
            
            {/* VIEW A: WELCOME HERO (EXACTLY MATCHING USER'S SCREENSHOT) */}
            {!inChatMode && (
              <div className="w-full max-w-2xl flex flex-col items-center my-auto">
                {/* Cute Cat Logo Illustration */}
                <div className="mb-3 text-neutral-900 hover:scale-105 transition-transform">
                  <CatLogo className="w-20 h-20" strokeWidth={2} />
                </div>

                {/* Main Heading */}
                <h1 className="text-2xl font-bold text-neutral-900 tracking-tight text-center">
                  What can ODON do for you?
                </h1>

                {/* Subheading */}
                <p className="text-xs text-neutral-500 text-center mt-1.5 mb-8">
                  Your universal AI assistant for coding, creating, documents, and more.
                </p>

                {/* 6 Feature Action Cards (3x2 Grid) */}
                <div className="grid grid-cols-3 gap-3.5 w-full">
                  {/* Card 1: Build an app */}
                  <button
                    onClick={() => handleActionCard('Build an app', 'Build a personal notes web application with clean modular React 19 and SQLite storage.')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Code2 className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Build an app</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Create, refactor, and debug code
                    </div>
                  </button>

                  {/* Card 2: Create an image */}
                  <button
                    onClick={() => handleActionCard('Create an image', 'Generate a modern minimalist product illustration for ODON desktop UI.')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <ImageIcon className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Create an image</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Generate or edit images
                    </div>
                  </button>

                  {/* Card 3: Make a document */}
                  <button
                    onClick={() => handleActionCard('Make a document', 'Draft a comprehensive product requirements document (PRD) for local AI orchestration.')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <FileText className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Make a document</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Write, convert, or analyze files
                    </div>
                  </button>

                  {/* Card 4: Run an agent */}
                  <button
                    onClick={() => handleActionCard('Run an agent', 'Run autonomous security audit agent on the workspace tools and policy engine.')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Bot className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Run an agent</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Let AI work on complex tasks
                    </div>
                  </button>

                  {/* Card 5: Automate a workflow */}
                  <button
                    onClick={() => handleActionCard('Automate a workflow', 'Set up automated background scheduler for Telegram and WhatsApp relay sync.')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Share2 className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Automate a workflow</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Schedule and orchestrate tasks
                    </div>
                  </button>

                  {/* Card 6: Create a video */}
                  <button
                    onClick={() => handleActionCard('Create a video', 'Generate a product showcase walkthrough storyboard and video script.')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Video className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Create a video</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Generate video from ideas
                    </div>
                  </button>
                </div>
              </div>
            )}

            {/* VIEW B: CONVERSATION STREAM */}
            {inChatMode && (
              <div className="w-full max-w-2xl flex flex-col space-y-6 my-auto">
                {messages.map((msg) => (
                  <div key={msg.id} className="flex flex-col space-y-2">
                    <div className="flex items-center gap-2">
                      <div className={`w-5 h-5 rounded-full flex items-center justify-center text-[10px] font-bold ${
                        msg.role === 'user'
                          ? 'bg-blue-600 text-white'
                          : 'bg-neutral-900 text-white'
                      }`}>
                        {msg.role === 'user' ? 'U' : <CatLogo className="w-3.5 h-3.5 text-white" />}
                      </div>
                      <span className="text-xs font-semibold text-neutral-900">
                        {msg.role === 'user' ? 'You' : 'ODON'}
                      </span>
                      <span className="text-[10px] text-neutral-400">{msg.timestamp}</span>
                    </div>

                    <div className="text-xs text-neutral-800 leading-relaxed pl-7">
                      {msg.content}
                    </div>

                    {/* Interactive App Card */}
                    {msg.showAppCard && (
                      <div className="ml-7 mt-2 p-3.5 rounded-xl border border-neutral-200/90 bg-neutral-50/60 shadow-2xs max-w-lg">
                        <div className="flex items-center justify-between mb-2">
                          <div className="flex items-center gap-2">
                            <div className="p-1.5 rounded-lg bg-blue-50 text-blue-600 border border-blue-100">
                              <Code2 className="w-4 h-4" />
                            </div>
                            <div>
                              <div className="font-semibold text-xs text-neutral-900">personal-notes</div>
                              <div className="text-[10px] text-neutral-500">React 19 + TypeScript + Local Storage</div>
                            </div>
                          </div>
                          <span className="inline-flex items-center gap-1 text-[10px] font-medium text-emerald-700 bg-emerald-50 px-2 py-0.5 rounded-full border border-emerald-200">
                            <CheckCircle2 className="w-3 h-3 text-emerald-600" />
                            Verified
                          </span>
                        </div>

                        <div className="flex items-center gap-2 mt-3">
                          <button
                            onClick={() => {
                              setRightPanelView('browser');
                              setIsRightPanelOpen(true);
                            }}
                            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-neutral-900 text-white hover:bg-neutral-800 text-xs font-medium transition-colors shadow-2xs"
                          >
                            <ExternalLink className="w-3 h-3" />
                            Preview in Browser
                          </button>
                          <button
                            onClick={() => {
                              setRightPanelView('editor');
                              setIsRightPanelOpen(true);
                            }}
                            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 bg-white hover:bg-neutral-50 text-neutral-700 text-xs font-medium transition-colors"
                          >
                            <Code2 className="w-3 h-3 text-neutral-500" />
                            Inspect Source
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Bottom Prompt Input Box */}
          <div className="p-4 pt-0 shrink-0">
            <div className="w-full max-w-2xl mx-auto rounded-3xl border border-neutral-200/90 bg-white shadow-2xs p-3 focus-within:border-neutral-300 focus-within:shadow-xs transition-all">
              {/* Input Area */}
              <input
                type="text"
                value={inputMessage}
                onChange={(e) => setInputMessage(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleSendMessage()}
                placeholder="Ask ODON anything..."
                className="w-full text-xs text-neutral-800 placeholder-neutral-400 outline-none px-1 py-1 bg-transparent"
              />

              {/* Bottom Row Tools inside Input Box */}
              <div className="flex items-center justify-between mt-2 pt-2 border-t border-neutral-100">
                {/* Left Tools */}
                <div className="flex items-center gap-1">
                  <button className="p-1.5 text-neutral-500 hover:text-neutral-800 hover:bg-neutral-100 rounded-full transition-colors">
                    <Plus className="w-4 h-4" />
                  </button>
                  <button className="p-1.5 text-neutral-500 hover:text-neutral-800 hover:bg-neutral-100 rounded-full transition-colors">
                    <Paperclip className="w-4 h-4" />
                  </button>
                  
                  {/* Tools Dropdown */}
                  <button className="flex items-center gap-1 px-2.5 py-1 text-xs text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-full transition-colors ml-1 border border-neutral-200/60">
                    <Globe className="w-3.5 h-3.5 text-neutral-500" />
                    <span>Tools</span>
                    <ChevronDown className="w-3 h-3 text-neutral-400" />
                  </button>
                </div>

                {/* Right Tools */}
                <div className="flex items-center gap-2">
                  {/* Model Selector Pill */}
                  <button className="flex items-center gap-1 px-2.5 py-1 text-xs text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-full transition-colors">
                    <span>Auto (GPT-4o)</span>
                    <ChevronDown className="w-3 h-3 text-neutral-400" />
                  </button>

                  {/* Send Button */}
                  <button
                    onClick={handleSendMessage}
                    className="w-7 h-7 rounded-full bg-neutral-900 hover:bg-neutral-800 text-white flex items-center justify-center transition-transform active:scale-95 shadow-2xs"
                  >
                    <ArrowUp className="w-3.5 h-3.5 stroke-[2.5]" />
                  </button>
                </div>
              </div>
            </div>
          </div>
        </main>

        {/* ───────────────────────────────────────────────────────────
            RIGHT PANEL: ARTIFACTS / RELAYS / CODE / BROWSER
        ─────────────────────────────────────────────────────────── */}
        {isRightPanelOpen && (
          <aside className="w-[300px] shrink-0 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col p-4 overflow-hidden">
            
            {/* Right Panel Sub-View: Code Editor */}
            {rightPanelView === 'editor' && (
              <div className="h-full flex flex-col">
                <div className="flex items-center justify-between pb-3 border-b border-neutral-100 mb-3">
                  <div className="flex items-center gap-1.5">
                    <button
                      onClick={() => setRightPanelView('artifacts')}
                      className="p-1 hover:bg-neutral-100 rounded-md text-neutral-500 hover:text-neutral-900 transition-colors"
                      title="Back to Artifacts"
                    >
                      <ArrowLeft className="w-3.5 h-3.5" />
                    </button>
                    <span className="font-bold text-xs text-neutral-900">Code Editor</span>
                  </div>
                  <button
                    onClick={() => setRightPanelView('browser')}
                    className="text-[11px] text-blue-600 hover:text-blue-800 font-medium"
                  >
                    Open Browser →
                  </button>
                </div>
                <div className="flex-1 overflow-hidden">
                  <CodeEditorView onRunPreview={() => setRightPanelView('browser')} />
                </div>
              </div>
            )}

            {/* Right Panel Sub-View: Browser Preview */}
            {rightPanelView === 'browser' && (
              <div className="h-full flex flex-col">
                <div className="flex items-center justify-between pb-3 border-b border-neutral-100 mb-3">
                  <div className="flex items-center gap-1.5">
                    <button
                      onClick={() => setRightPanelView('artifacts')}
                      className="p-1 hover:bg-neutral-100 rounded-md text-neutral-500 hover:text-neutral-900 transition-colors"
                      title="Back to Artifacts"
                    >
                      <ArrowLeft className="w-3.5 h-3.5" />
                    </button>
                    <span className="font-bold text-xs text-neutral-900">Browser Preview</span>
                  </div>
                  <button
                    onClick={() => setRightPanelView('editor')}
                    className="text-[11px] text-blue-600 hover:text-blue-800 font-medium"
                  >
                    View Code →
                  </button>
                </div>
                <div className="flex-1 overflow-hidden">
                  <BrowserPreviewView />
                </div>
              </div>
            )}

            {/* Right Panel Default View: Artifacts & Relays (EXACT MATCH TO USER'S SCREENSHOT) */}
            {rightPanelView === 'artifacts' && (
              <div className="flex-1 flex flex-col justify-between overflow-y-auto">
                <div className="space-y-4">
                  {/* 1. Artifacts Section */}
                  <div>
                    <div className="flex items-center gap-2 mb-0.5">
                      <Layers className="w-4 h-4 text-neutral-900" />
                      <h2 className="font-bold text-sm text-neutral-900">Artifacts</h2>
                    </div>
                    <p className="text-[11px] text-neutral-500 mb-3">
                      View and manage results from your tasks.
                    </p>

                    {/* Artifacts List */}
                    <div className="space-y-2">
                      {/* Item 1: odon-architecture.png */}
                      <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group">
                        <div className="flex items-center gap-2.5">
                          <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
                            <ImageIcon className="w-4 h-4" />
                          </div>
                          <div>
                            <div className="text-xs font-medium text-neutral-900 leading-tight">
                              odon-architecture.png
                            </div>
                            <div className="text-[10px] text-neutral-400 mt-0.5">
                              Image • 2.4 MB • 2h ago
                            </div>
                          </div>
                        </div>
                        <button className="text-neutral-400 hover:text-neutral-700 p-1">
                          <ExternalLink className="w-3.5 h-3.5" />
                        </button>
                      </div>

                      {/* Item 2: agent.rs */}
                      <div
                        onClick={() => setRightPanelView('editor')}
                        className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group cursor-pointer"
                      >
                        <div className="flex items-center gap-2.5">
                          <div className="p-2 rounded-xl bg-neutral-100 text-neutral-700">
                            <Code2 className="w-4 h-4" />
                          </div>
                          <div>
                            <div className="text-xs font-medium text-neutral-900 leading-tight">
                              agent.rs
                            </div>
                            <div className="text-[10px] text-neutral-400 mt-0.5">
                              Code • 12 KB • 4h ago
                            </div>
                          </div>
                        </div>
                        <button className="text-neutral-400 hover:text-neutral-700 p-1">
                          <ExternalLink className="w-3.5 h-3.5" />
                        </button>
                      </div>

                      {/* Item 3: rencana-produk.pdf */}
                      <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group">
                        <div className="flex items-center gap-2.5">
                          <div className="p-2 rounded-xl bg-red-50 text-red-500">
                            <FileText className="w-4 h-4" />
                          </div>
                          <div>
                            <div className="text-xs font-medium text-neutral-900 leading-tight">
                              rencana-produk.pdf
                            </div>
                            <div className="text-[10px] text-neutral-400 mt-0.5">
                              PDF • 1.1 MB • Yesterday
                            </div>
                          </div>
                        </div>
                        <button className="text-neutral-400 hover:text-neutral-700 p-1">
                          <ExternalLink className="w-3.5 h-3.5" />
                        </button>
                      </div>

                      {/* Item 4: demo-odon.mp4 */}
                      <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group">
                        <div className="flex items-center gap-2.5">
                          <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
                            <Video className="w-4 h-4" />
                          </div>
                          <div>
                            <div className="text-xs font-medium text-neutral-900 leading-tight">
                              demo-odon.mp4
                            </div>
                            <div className="text-[10px] text-neutral-400 mt-0.5">
                              Video • 24.8 MB • Yesterday
                            </div>
                          </div>
                        </div>
                        <button className="text-neutral-400 hover:text-neutral-700 p-1">
                          <ExternalLink className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>

                    {/* View all artifacts link */}
                    <button
                      onClick={() => setRightPanelView('browser')}
                      className="text-xs font-medium text-neutral-700 hover:text-neutral-900 flex items-center gap-1 mt-3 px-1"
                    >
                      <span>View all artifacts</span>
                      <span>→</span>
                    </button>
                  </div>

                  {/* Divider */}
                  <div className="border-t border-neutral-100 my-4" />

                  {/* 2. Relays Section */}
                  <div>
                    <div className="flex items-center gap-2 mb-0.5">
                      <Radio className="w-4 h-4 text-neutral-900" />
                      <h2 className="font-bold text-sm text-neutral-900">Relays</h2>
                    </div>
                    <p className="text-[11px] text-neutral-500 mb-3">
                      Connect and control ODON from your chat apps.
                    </p>

                    <div className="space-y-2">
                      {/* Telegram */}
                      <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors cursor-pointer">
                        <div className="flex items-center gap-2.5">
                          <div className="w-7 h-7 rounded-full bg-[#0088cc] text-white flex items-center justify-center">
                            <svg viewBox="0 0 24 24" fill="currentColor" className="w-3.5 h-3.5">
                              <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm4.64 6.8c-.15 1.58-.8 5.42-1.13 7.19-.14.75-.42 1-.68 1.03-.58.05-1.02-.38-1.58-.75-.88-.58-1.38-.94-2.23-1.5-.99-.65-.35-1.01.22-1.59.15-.15 2.71-2.48 2.76-2.69a.2.2 0 00-.05-.18c-.06-.05-.14-.03-.21-.02-.09.02-1.49.95-4.22 2.79-.4.27-.76.41-1.08.4-.36-.01-1.04-.2-1.55-.37-.63-.2-1.12-.31-1.08-.66.02-.18.27-.36.75-.55 2.92-1.27 4.86-2.11 5.83-2.52 2.77-1.16 3.35-1.36 3.73-1.37.08 0 .27.02.39.12.1.08.13.19.14.27-.01.06.01.24 0 .38z" />
                            </svg>
                          </div>
                          <span className="text-xs font-semibold text-neutral-900">Telegram</span>
                        </div>
                        <div className="flex items-center gap-2">
                          <div className="flex items-center gap-1.5 text-[11px] text-neutral-600">
                            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
                            <span>Connected</span>
                          </div>
                          <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                        </div>
                      </div>

                      {/* WhatsApp */}
                      <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors cursor-pointer">
                        <div className="flex items-center gap-2.5">
                          <div className="w-7 h-7 rounded-full bg-[#25d366] text-white flex items-center justify-center">
                            <svg viewBox="0 0 24 24" fill="currentColor" className="w-3.5 h-3.5">
                              <path d="M12.04 2c-5.46 0-9.91 4.45-9.91 9.91 0 1.75.46 3.45 1.32 4.95L2.05 22l5.25-1.38c1.45.79 3.08 1.21 4.74 1.21 5.46 0 9.91-4.45 9.91-9.91 0-2.65-1.03-5.14-2.9-7.01A9.816 9.816 0 0012.04 2zm0 18.15c-1.49 0-2.94-.4-4.22-1.16l-.3-.18-3.13.82.83-3.05-.2-.31a8.17 8.17 0 01-1.25-4.36c0-4.52 3.68-8.2 8.2-8.2 2.19 0 4.25.85 5.8 2.4 1.55 1.55 2.4 3.61 2.4 5.8 0 4.52-3.68 8.2-8.13 8.2zm4.49-6.14c-.25-.12-1.47-.72-1.7-.81-.23-.08-.39-.12-.56.12-.17.25-.64.81-.79.97-.14.17-.29.19-.54.06-.25-.12-1.05-.39-2-1.23-.74-.66-1.23-1.47-1.38-1.72-.14-.25-.02-.38.11-.5.11-.11.25-.29.37-.43.12-.14.17-.25.25-.41.08-.17.04-.31-.02-.43s-.56-1.34-.76-1.84c-.2-.48-.41-.42-.56-.43h-.48c-.17 0-.43.06-.66.31-.22.25-.86.84-.86 2.05s.88 2.38 1 2.55c.12.17 1.73 2.65 4.2 3.71.59.25 1.05.41 1.41.52.59.19 1.13.16 1.56.1.48-.07 1.47-.6 1.68-1.18.21-.58.21-1.07.14-1.18-.06-.11-.23-.17-.48-.3z" />
                            </svg>
                          </div>
                          <span className="text-xs font-semibold text-neutral-900">WhatsApp</span>
                        </div>
                        <div className="flex items-center gap-2">
                          <div className="flex items-center gap-1.5 text-[11px] text-neutral-600">
                            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
                            <span>Connected</span>
                          </div>
                          <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                        </div>
                      </div>
                    </div>
                  </div>
                </div>

                {/* 3. Agent Activity Section at Bottom */}
                <div className="pt-3 border-t border-neutral-100">
                  <div className="flex items-center gap-2 mb-1">
                    <Activity className="w-4 h-4 text-neutral-900" />
                    <h3 className="font-bold text-xs text-neutral-900">Agent Activity</h3>
                  </div>
                  <div className="flex items-center gap-1.5 text-xs text-neutral-500 mt-1">
                    <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                    <span>Idle • Ready for new tasks</span>
                  </div>
                </div>
              </div>
            )}
          </aside>
        )}
      </div>
    </div>
  );
}
