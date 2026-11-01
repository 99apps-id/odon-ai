export type SessionMode = 'general' | 'code' | 'create' | 'documents' | 'files' | 'agents' | 'automations';

export interface OutputActionChip {
  id: string;
  label: string;
  icon?: string;
  action_type: string;
  payload?: any;
}

export interface ToolCallInfo {
  call_id: string;
  tool_name: string;
  arguments: any;
  output?: any;
  is_error: boolean;
  requires_approval: boolean;
  is_approved?: boolean;
}

export interface MessageContent {
  text: string;
  thought?: string;
  tool_calls: ToolCallInfo[];
  artifact_ids: string[];
  action_chips: OutputActionChip[];
}

export interface Message {
  id: string;
  session_id: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: MessageContent;
  model?: string;
  created_at: string;
}

export interface ArtifactFile {
  path: string;
  content: string;
}

export type ArtifactPayload =
  | { type: 'code'; data: { language: string; framework?: string; entry_file?: string; files: ArtifactFile[] } }
  | { type: 'document'; data: { doc_type: string; markdown: string; size_bytes: number } }
  | { type: 'image'; data: { prompt: string; width: number; height: number; file_path?: string; preview_url?: string; size_bytes: number; model: string } }
  | { type: 'patch'; data: { git_target: string; diff: string; files_changed: number; additions: number; deletions: number } }
  | { type: 'automation'; data: { trigger: string; schedule?: string; action: string; is_active: boolean } };

export interface Artifact {
  id: string;
  session_id: string;
  title: string;
  kind: 'code' | 'document' | 'image' | 'patch' | 'automation';
  summary: string;
  payload: ArtifactPayload;
  created_at: string;
}

export interface Session {
  id: string;
  title: string;
  mode: SessionMode;
  status: 'active' | 'archived' | 'completed';
  active_model: string;
  workspace_root?: string;
  created_at: string;
  updated_at: string;
}

export interface ToolSchema {
  name: string;
  description: string;
  parameters: any;
  risk_tier: number;
}

export interface ApprovalRequest {
  id: string;
  session_id: string;
  tool_name: string;
  risk_tier: number;
  description: string;
  parameters: any;
  created_at: string;
}

export interface SystemStatus {
  status: string;
  version: string;
  workspace_root: string;
  tools_count: number;
  execution_mode: string;
  relays: {
    desktop_api: boolean;
    sse_stream: boolean;
    cli: boolean;
    telegram: boolean;
    whatsapp: boolean;
  };
}

export type AgentEvent =
  | { type: 'state_changed'; payload: { session_id: string; state: string } }
  | { type: 'text_delta'; payload: { session_id: string; delta: string } }
  | { type: 'thought_delta'; payload: { session_id: string; delta: string } }
  | { type: 'tool_call_started'; payload: { session_id: string; call_id: string; tool_name: string; arguments: any } }
  | { type: 'tool_call_finished'; payload: { session_id: string; call_id: string; output: any; is_error: boolean } }
  | { type: 'artifact_created'; payload: { session_id: string; artifact: Artifact } }
  | { type: 'approval_requested'; payload: { session_id: string; approval_id: string; tool_name: string; risk_tier: number; description: string } }
  | { type: 'approval_resolved'; payload: { session_id: string; approval_id: string; decision: 'approved' | 'rejected' | 'expired' } }
  | { type: 'error'; payload: { session_id: string; message: string } };

export interface ModelItem {
  id: string;
  name: string;
  context: number;
  recommended?: boolean;
}

export interface ProviderItem {
  id: string;
  name: string;
  category: 'oauth' | 'byok';
  auth_type: 'oauth' | 'api_key';
  description: string;
  default_base_url?: string;
  connected?: boolean;
  configured?: boolean;
  models: ModelItem[];
}

export const FALLBACK_PROVIDERS: ProviderItem[] = [
  {
    id: 'github-copilot',
    name: 'GitHub Copilot',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Active GitHub Copilot session with frontier GPT-6 Astra and Claude 5.5 models',
    models: [
      { id: 'gpt-6-astra', name: 'GPT-6 Astra (GitHub Copilot)', context: 1050000, recommended: true },
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5 (GitHub Copilot)', context: 1000000, recommended: true },
      { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol (GitHub Copilot)', context: 1050000 },
      { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5 (GitHub Copilot)', context: 1000000 },
    ],
  },
  {
    id: 'antigravity',
    name: 'Google Antigravity',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Google Cloud Code Assist & Antigravity OAuth with Gemini 4 and Claude 5.5 reasoning',
    models: [
      { id: 'gemini-4-argon', name: 'Gemini 4 Argon (Flagship)', context: 1000000, recommended: true },
      { id: 'gemini-3.8-flash', name: 'Gemini 3.8 Flash (Agentic & Coding)', context: 1000000, recommended: true },
      { id: 'gemini-3.7-flash', name: 'Gemini 3.7 Flash', context: 1000000 },
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5 (Antigravity)', context: 1000000, recommended: true },
      { id: 'gemini-3.8-live', name: 'Gemini 3.8 Live (Real-Time)', context: 128000 },
    ],
  },
  {
    id: 'chatgpt',
    name: 'ChatGPT / OpenAI Codex',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Official OpenAI OAuth authorization with GPT-6 frontier reasoning models',
    models: [
      { id: 'gpt-6-astra', name: 'GPT-6 Astra (Flagship Frontier)', context: 1050000, recommended: true },
      { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol (Production Reasoning)', context: 1050000, recommended: true },
      { id: 'gpt-6.1-terra', name: 'GPT-6.1 Terra (Visual & Agentic)', context: 1050000 },
      { id: 'gpt-6-luna', name: 'GPT-6 Luna (Fast & High Throughput)', context: 1050000 },
    ],
  },
  {
    id: 'claude-oauth',
    name: 'Claude (Anthropic OAuth)',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Direct Anthropic Claude account authorization with Claude 5.5 architecture',
    models: [
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5 (Flagship Balance)', context: 1000000, recommended: true },
      { id: 'claude-opus-5-5', name: 'Claude Opus 5.5 (Deep Agentic Coding)', context: 1000000, recommended: true },
      { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5 (Low Latency)', context: 1000000 },
      { id: 'claude-fable-5-1', name: 'Claude Fable 5.1 (Complex Reasoning)', context: 1000000 },
    ],
  },
  {
    id: 'xai-oauth',
    name: 'xAI Grok',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'xAI Grok device authorization with Grok 4.7 frontier reasoning',
    models: [
      { id: 'grok-4.7', name: 'Grok 4.7 (Flagship Frontier)', context: 500000, recommended: true },
      { id: 'grok-4.6', name: 'Grok 4.6 (High Context)', context: 500000 },
      { id: 'grok-4.20-reasoning', name: 'Grok 4.20 Reasoning (Deep Math & Logic)', context: 256000, recommended: true },
    ],
  },
  {
    id: 'muse',
    name: 'Meta Muse Code',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Meta Muse Code developer device authorization and SSE streaming',
    models: [
      { id: 'muse-spark-2.0', name: 'Muse Spark 2.0 (Flagship Coding)', context: 256000, recommended: true },
      { id: 'muse-code-v3', name: 'Muse Code v3', context: 128000 },
    ],
  },
  {
    id: 'openai',
    name: 'OpenAI (Official BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Official OpenAI platform with GPT-6 Astra, Sol, Terra, and Luna models',
    default_base_url: 'https://api.openai.com/v1',
    models: [
      { id: 'gpt-6-astra', name: 'GPT-6 Astra (Flagship Frontier)', context: 1050000, recommended: true },
      { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol (Production Reasoning)', context: 1050000, recommended: true },
      { id: 'gpt-6.1-terra', name: 'GPT-6.1 Terra (Visual & Agentic)', context: 1050000 },
      { id: 'gpt-6-luna', name: 'GPT-6 Luna (Fast & High Throughput)', context: 1050000 },
    ],
  },
  {
    id: 'anthropic',
    name: 'Anthropic (Official BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Official Anthropic API with Claude 5.5 adaptive thinking models',
    default_base_url: 'https://api.anthropic.com/v1',
    models: [
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5 (Flagship Balance)', context: 1000000, recommended: true },
      { id: 'claude-opus-5-5', name: 'Claude Opus 5.5 (Deep Agentic Coding)', context: 1000000, recommended: true },
      { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5 (Low Latency)', context: 1000000 },
      { id: 'claude-fable-5-1', name: 'Claude Fable 5.1 (Complex Reasoning)', context: 1000000 },
    ],
  },
  {
    id: 'google',
    name: 'Google Gemini (Official BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Google Gemini API with 1M context and native multimodal reasoning',
    default_base_url: 'https://generativelanguage.googleapis.com/v1beta/openai',
    models: [
      { id: 'gemini-4-argon', name: 'Gemini 4 Argon (Flagship)', context: 1000000, recommended: true },
      { id: 'gemini-3.8-flash', name: 'Gemini 3.8 Flash (Agentic & Coding)', context: 1000000, recommended: true },
      { id: 'gemini-3.7-flash', name: 'Gemini 3.7 Flash', context: 1000000 },
      { id: 'gemini-3.8-live', name: 'Gemini 3.8 Live (Real-Time)', context: 128000 },
    ],
  },
  {
    id: 'deepseek',
    name: 'DeepSeek (BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'DeepSeek V4.1 Flash and Pro reasoning models with 1M context',
    default_base_url: 'https://api.deepseek.com/v1',
    models: [
      { id: 'deepseek-flash', name: 'DeepSeek-V4.1-Flash (General & Agent)', context: 1000000, recommended: true },
      { id: 'deepseek-v4-pro', name: 'DeepSeek-V4-Pro (Deep Reasoning & Coding)', context: 1000000, recommended: true },
    ],
  },
  {
    id: 'qwen',
    name: 'Qwen Plan / DashScope Global',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Alibaba Cloud international AI gateway: Qwen 3.5, Qwen Max, and Qwen 2.5 Coder',
    default_base_url: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
    models: [
      { id: 'qwen3.5-plus', name: 'Qwen 3.5 Plus (Flagship)', context: 1000000, recommended: true },
      { id: 'qwen-max', name: 'Qwen Max (Complex Reasoning)', context: 131072, recommended: true },
      { id: 'qwen-2.5-coder-32b-instruct', name: 'Qwen 2.5 Coder 32B', context: 131072, recommended: true },
      { id: 'qwq-32b-preview', name: 'QwQ 32B Reasoning Preview', context: 32768 },
    ],
  },
  {
    id: 'mistral',
    name: 'Mistral AI',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Mistral AI models including Mistral Large 4, Medium 3.5, and Codestral',
    default_base_url: 'https://api.mistral.ai/v1',
    models: [
      { id: 'mistral-large-4', name: 'Mistral Large 4 (Multimodal Flagship)', context: 128000, recommended: true },
      { id: 'mistral-medium-3.5', name: 'Mistral Medium 3.5 (Agentic & Coding)', context: 128000, recommended: true },
      { id: 'mistral-small-4', name: 'Mistral Small 4', context: 128000 },
      { id: 'codestral-latest', name: 'Codestral Latest (Code Specialist)', context: 256000, recommended: true },
      { id: 'ministral-8b', name: 'Ministral 8B (Fast & Light)', context: 128000 },
    ],
  },
  {
    id: 'groq',
    name: 'Groq (Fast Inference)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Ultra high-speed LPU inference engine for open-weights Llama 4 models',
    default_base_url: 'https://api.groq.com/openai/v1',
    models: [
      { id: 'llama-4-scout', name: 'Llama 4 Scout (10M Context)', context: 10000000, recommended: true },
      { id: 'llama-3.3-70b-versatile', name: 'Llama 3.3 70B Versatile', context: 131072, recommended: true },
      { id: 'llama-3.1-8b-instant', name: 'Llama 3.1 8B Instant', context: 131072 },
    ],
  },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Unified gateway across all frontier models and providers',
    default_base_url: 'https://openrouter.ai/api/v1',
    models: [
      { id: 'openai/gpt-6-astra', name: 'GPT-6 Astra (OpenRouter)', context: 1050000, recommended: true },
      { id: 'anthropic/claude-sonnet-5-5', name: 'Claude Sonnet 5.5 (OpenRouter)', context: 1000000, recommended: true },
      { id: 'deepseek/deepseek-flash', name: 'DeepSeek Flash (OpenRouter)', context: 1000000, recommended: true },
      { id: 'google/gemini-4-argon', name: 'Gemini 4 Argon (OpenRouter)', context: 1000000 },
    ],
  },
  {
    id: 'ollama',
    name: 'Ollama (Local Models)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Run open models entirely locally on your hardware without internet',
    default_base_url: 'http://localhost:11434/v1',
    models: [
      { id: 'llama4:scout', name: 'Llama 4 Scout Local (10M Context)', context: 10000000, recommended: true },
      { id: 'llama4:maverick', name: 'Llama 4 Maverick Local', context: 1000000, recommended: true },
      { id: 'llama3.3', name: 'Llama 3.3 70B Local', context: 32768 },
      { id: 'deepseek-flash', name: 'DeepSeek-V4.1 Local', context: 64000 },
    ],
  },
  {
    id: 'openai-compatible',
    name: 'OpenAI Compatible (Custom / Gateway)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Any OpenAI-compatible gateway: vLLM, LM Studio, LiteLLM, or custom proxies',
    default_base_url: 'http://localhost:11434/v1',
    models: [],
  },
];

export const CUSTOM_OPENAI_MODELS_STORAGE_KEY = 'odon_custom_models_openai_compatible';

export function getCustomOpenAiModels(): ModelItem[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = localStorage.getItem(CUSTOM_OPENAI_MODELS_STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed) && parsed.length > 0) {
        return parsed.filter((m) => m && typeof m.id === 'string' && m.id.trim().length > 0);
      }
    }
  } catch {}
  return [];
}

export function saveCustomOpenAiModels(models: ModelItem[]): void {
  if (typeof window === 'undefined') return;
  const valid = models.filter((m) => m && typeof m.id === 'string' && m.id.trim().length > 0);
  localStorage.setItem(CUSTOM_OPENAI_MODELS_STORAGE_KEY, JSON.stringify(valid));
}

export interface TerminalInfo {
  shell: string;
  os: string;
  arch: string;
  default_cwd: string;
  user: string;
}

export interface TerminalExecResult {
  exit_code: number;
  stdout: string;
  stderr: string;
  cwd: string;
  success: boolean;
}
