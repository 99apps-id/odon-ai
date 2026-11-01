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
