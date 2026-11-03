import { useState } from "react";
import { ImageIcon, Video, FileText, Terminal, Shield, GitPullRequest, Share2 } from "lucide-react";

type ExecutionModeKey = "chat" | "auto" | "agent" | "plan";

// Mirrors the backend SessionMode labels. Kept separate from
// ExecutionModeKey: a starter has a composer mode and a session label,
// and the two vocabularies are not interchangeable.
type SessionModeKey =
  | "general"
  | "code"
  | "create"
  | "documents"
  | "agents"
  | "automations";

type PromptStartersProps = {
  onSelect: (
    title: string,
    promptText: string,
    mode: ExecutionModeKey,
    sessionMode: SessionModeKey,
  ) => void;
};

type Starter = {
  id: string;
  title: string;
  description: string;
  prompt: string;
  mode: ExecutionModeKey;
  sessionMode: SessionModeKey;
  icon: typeof ImageIcon;
};

const STARTERS: Starter[] = [
  {
    id: "app",
    title: "Build an app",
    description: "Scaffold a modular app scaffold",
    prompt:
      "Build a personal notes web application with clean modular React 19 and SQLite storage.",
    mode: "agent",
    sessionMode: "code",
    icon: Terminal,
  },
  {
    id: "review",
    title: "Review an interface",
    description: "Analyze a screenshot or mockup",
    prompt:
      "Review the attached interface screenshot for usability, accessibility, and visual hierarchy. Prioritize actionable fixes.",
    mode: "chat",
    sessionMode: "general",
    icon: ImageIcon,
  },
  {
    id: "document",
    title: "Make a document",
    description: "Draft a product requirements document",
    prompt:
      "Draft a comprehensive product requirements document (PRD) for local AI orchestration.",
    mode: "chat",
    sessionMode: "documents",
    icon: FileText,
  },
  {
    id: "agent",
    title: "Run an agent",
    description: "Run an autonomous audit",
    prompt:
      "Run an autonomous security audit agent on the workspace tools and policy engine.",
    mode: "agent",
    sessionMode: "agents",
    icon: Shield,
  },
  {
    id: "automation",
    title: "Automate a workflow",
    description: "Set up an automated relay sync",
    prompt:
      "Set up automated background scheduler for Telegram and WhatsApp relay sync.",
    mode: "auto",
    sessionMode: "automations",
    icon: Share2,
  },
  {
    id: "walkthrough",
    title: "Plan a walkthrough",
    description: "Draft a storyboard and script",
    prompt:
      "Create a concise product walkthrough storyboard and narration script, with one scene per key user outcome.",
    mode: "plan",
    sessionMode: "create",
    icon: Video,
  },
];

export function PromptStartersGrid({ onSelect }: PromptStartersProps) {
  const [hovered, setHovered] = useState<string | null>(null);

  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3.5 w-full">
      {STARTERS.map((starter) => {
        const Icon = starter.icon;
        const isHovered = hovered === starter.id;
        return (
          <button
            key={starter.id}
            onMouseEnter={() => setHovered(starter.id)}
            onMouseLeave={() => setHovered(null)}
            onClick={() =>
              onSelect(
                starter.title,
                starter.prompt,
                starter.mode,
                starter.sessionMode,
              )
            }
            className={`p-3.5 rounded-2xl border text-left transition-all cursor-pointer ${
              isHovered
                ? "border-neutral-300 shadow-xs bg-neutral-50"
                : "border-neutral-200/90 bg-white hover:border-neutral-300 hover:shadow-xs"
            }`}
          >
            <div className="text-neutral-800 mb-2">
              <Icon
                className={`w-5 h-5 transition-transform ${
                  isHovered ? "scale-110" : ""
                }`}
              />
            </div>
            <div className="font-semibold text-xs text-neutral-900">{starter.title}</div>
            <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
              {starter.description}
            </div>
          </button>
        );
      })}
    </div>
  );
}
