import {
  ProviderItem,
  Session,
  Message,
  Artifact,
  ToolSchema,
  ApprovalRequest,
  SystemStatus,
  AgentEvent,
  TerminalInfo,
  TerminalExecResult,
} from './types';

export interface OAuthAccountInfo {
  provider: string;
  email: string | null;
  plan: string | null;
  account_id: string | null;
  connected: boolean;
}

export interface OAuthLoginResponse {
  auth_url?: string;
  user_code?: string;
  verification_uri?: string;
  device_code?: string;
  status: 'pending_browser' | 'pending_device' | 'completed';
  account?: OAuthAccountInfo;
}

export interface BrowserSessionInfo {
  session_id: string;
  url: string;
  created_at?: string;
  port?: number;
  pid?: number;
  live?: boolean;
}

export interface FsEntryItem {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified_at?: string;
  extension?: string;
}

export interface FsListResult {
  current_path: string;
  workspace_root: string;
  entries: FsEntryItem[];
  error?: string;
}

export interface FsReadResult {
  path: string;
  name: string;
  size: number;
  is_text: boolean;
  content?: string;
  base64?: string;
  error?: string;
}

export interface TodoItem {
  id: string;
  title: string;
  status: 'pending' | 'in_progress' | 'completed';
}

export interface WorkspaceInfo {
  default: string;
  allowed: string[];
}

export interface WorkspaceBrowseEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

export interface WorkspaceBrowseResult {
  current: string;
  parent: string | null;
  roots: string[];
  entries: WorkspaceBrowseEntry[];
  error?: string;
}

export interface TelegramRelayStatus {
  configured: boolean;
  online: boolean;
  token_masked: string;
  allowed_users: string[];
}

export interface WhatsAppBridgeStatus {
  running: boolean;
  connected: boolean;
  qr?: string | null;
  pairing_code?: string | null;
  phone?: string | null;
  jid?: string | null;
  push_name?: string | null;
  error?: string | null;
  updated_at?: string;
}

export interface SshProfile {
  id: string;
  alias: string;
  host: string;
  port: number;
  user: string;
  key_path?: string;
  certificate_filename?: string;
  certificate_data?: string;
  auth_type: string;
  description?: string;
  created_at: string;
}

export interface SkillItemInfo {
  name: string;
  title: string;
  description: string;
  path: string;
  content: string;
}

export interface GitHubUser {
  login: string;
  name: string;
  avatar_url: string;
  bio?: string;
}

export interface GitHubStatusResponse {
  authenticated: boolean;
  auth_source?: string;
  user?: GitHubUser;
  workspace_repo?: { owner: string; repo: string };
}

export interface GitHubRepoItem {
  name: string;
  owner: string;
  full_name: string;
  is_private: boolean;
  description: string;
}

/**
 * Whether an SSE event belongs to the session the UI is showing.
 *
 * `/api/events` is a single broadcast stream: every session the daemon is
 * running (including subagents and relay-driven turns) writes into it. An event
 * that names a session other than the active one must be dropped before it
 * touches any state. Events with no session id are global and always pass, so
 * daemon-wide notices are never suppressed.
 *
 * `activeSessionId` is passed per event rather than captured, which keeps a
 * long-lived subscriber from filtering against the session it opened with.
 */
export function isAgentEventForSession(
  event: { session_id?: string | null; payload?: { session_id?: string | null } | null },
  activeSessionId: string | null,
): boolean {
  const raw = event.payload?.session_id ?? event.session_id;
  const eventSessionId = typeof raw === 'string' ? raw.trim() : '';
  if (!eventSessionId) return true;
  return eventSessionId === activeSessionId;
}

export const api = {
  getApiToken(): string {
    if (typeof window === 'undefined') return '';
    return localStorage.getItem('odon_api_token') || '';
  },

  setApiToken(token: string) {
    if (typeof window === 'undefined') return;
    const trimmed = token.trim();
    if (trimmed) localStorage.setItem('odon_api_token', trimmed);
    else localStorage.removeItem('odon_api_token');
  },

  /**
   * Asks the daemon, without sending any token, whether it requires one.
   * Uses /api/health, which the daemon exempts from token auth on purpose, so
   * the reply is readable even when the configured token is wrong or missing.
   * `tokenRequired: null` means the daemon answered but did not report the flag.
   */
  async probeAuthMode(): Promise<{ reachable: boolean; tokenRequired: boolean | null }> {
    try {
      const res = await window.fetch(`${this.getBaseUrl()}/api/health`);
      if (!res.ok) return { reachable: true, tokenRequired: null };
      const data = await res.json().catch(() => null);
      if (!data || typeof data.auth_required !== 'boolean') {
        return { reachable: true, tokenRequired: null };
      }
      return { reachable: true, tokenRequired: data.auth_required };
    } catch {
      return { reachable: false, tokenRequired: null };
    }
  },

  isTrustedApiUrl(input: string | URL): boolean {
    if (typeof window === 'undefined') return false;
    try {
      const pageOrigin = window.location.origin;
      const baseUrl = this.getBaseUrl();
      const trustedOrigin = new URL(baseUrl || pageOrigin, pageOrigin).origin;
      return new URL(input.toString(), pageOrigin).origin === trustedOrigin;
    } catch {
      return false;
    }
  },

  /**
   * Turns a raw browser network failure into a message that names the cause.
   * `window.fetch` rejects with a bare `TypeError: Failed to fetch` when the
   * daemon is not listening, which reads like an application error and hides
   * the only action that helps: start odond.
   */
  describeError(error: unknown, fallback: string): string {
    const raw =
      error instanceof Error ? error.message : typeof error === 'string' ? error : '';
    const text = raw.trim();
    const looksLikeNetworkFailure =
      error instanceof TypeError ||
      /failed to fetch|networkerror|load failed|err_connection_refused|connection refused/i.test(
        text,
      );
    if (!looksLikeNetworkFailure) return text || fallback;
    const base = this.getBaseUrl() || 'this origin';
    return `Cannot reach the ODON daemon at ${base}. Start it with \`odond serve\` (the desktop app launches it automatically), then retry.`;
  },

  /** fetch wrapper that only attaches ODON_API_TOKEN to the configured daemon origin. */
  async fetchApi(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
    const token = this.getApiToken();
    const url = input instanceof Request ? input.url : input;
    if (!token || !this.isTrustedApiUrl(url)) return window.fetch(input, init);
    const headers = new Headers(init?.headers || (input instanceof Request ? input.headers : undefined));
    headers.set('Authorization', `Bearer ${token}`);
    const res = await window.fetch(input, { ...init, headers });
    if (res.status !== 401) return res;
    // The daemon minted a new loopback token (it was restarted, or a second
    // instance overwrote the token file). Adopt the current token once and
    // replay the request, so a stale token does not surface as "model failed"
    // or "daemon offline" while the daemon is actually healthy.
    const refreshed = await this.refreshTokenFromShell();
    if (!refreshed || refreshed === token) return res;
    const retryHeaders = new Headers(init?.headers || (input instanceof Request ? input.headers : undefined));
    retryHeaders.set('Authorization', `Bearer ${refreshed}`);
    return window.fetch(input, { ...init, headers: retryHeaders });
  },

  /** Ask the desktop shell for the daemon's current token. No-op in a browser. */
  async refreshTokenFromShell(): Promise<string | null> {
    try {
      const { daemonToken } = await import('./lib/desktop');
      const token = await daemonToken();
      if (token && token !== this.getApiToken()) {
        this.setApiToken(token);
      }
      return token;
    } catch {
      return null;
    }
  },

  authorizeUrl(url: string): string {
    if (!url.includes('/api/') || !this.isTrustedApiUrl(url)) return url;
    const token = this.getApiToken();
    if (!token) return url;
    return `${url}${url.includes('?') ? '&' : '?'}token=${encodeURIComponent(token)}`;
  },

  getBaseUrl(): string {
    if (typeof window !== 'undefined') {
      const explicit = localStorage.getItem('odon_api_base');
      if (explicit) return explicit;
      // In browser development (Vite on port 3000) or served from daemon (port 4096),
      // empty string uses the current host/proxy, avoiding CORS or 127.0.0.1 mismatch.
      return '';
    }
    return 'http://127.0.0.1:4096';
  },

  setBaseUrl(url: string) {
    if (typeof window !== 'undefined') {
      localStorage.setItem('odon_api_base', url.replace(/\/+$/, ''));
    }
  },

  async getStatus(): Promise<SystemStatus> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/status`);
    if (!res.ok) throw new Error(`HTTP error ${res.status}`);
    return res.json();
  },

  async browserRead(url: string): Promise<{
    url: string;
    status: number;
    title: string;
    content_type: string;
    html: string;
    text: string;
    links: string[];
  }> {
    const res = await this.fetchApi(
      `${this.getBaseUrl()}/api/browser/read?url=${encodeURIComponent(url)}`,
    );
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `HTTP error ${res.status}`);
    return data;
  },

  // ── Live browser bridge ────────────────────────────────────────────────
  // These hit the daemon routes that drive the same Chromium session the agent
  // tools use, so the panel mirrors the agent and can be clicked by the user.
  async browserSessions(): Promise<{ sessions: BrowserSessionInfo[]; browser: boolean }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/browser/sessions`);
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `HTTP error ${res.status}`);
    return data;
  },

  async browserOpenSession(url: string): Promise<BrowserSessionInfo> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/browser/sessions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ url }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `HTTP error ${res.status}`);
    return data;
  },

  async browserNavigate(id: string, url: string): Promise<{ url: string; title: string; ready: boolean }> {
    const res = await this.fetchApi(
      `${this.getBaseUrl()}/api/browser/sessions/${encodeURIComponent(id)}/navigate`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url }),
      },
    );
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `HTTP error ${res.status}`);
    return data;
  },

  async browserPageState(id: string): Promise<{ url: string; title: string; text: string; links: string[] }> {
    const res = await this.fetchApi(
      `${this.getBaseUrl()}/api/browser/sessions/${encodeURIComponent(id)}/state`,
    );
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `HTTP error ${res.status}`);
    return data;
  },

  async browserScreenshot(id: string): Promise<string> {
    const res = await this.fetchApi(
      `${this.getBaseUrl()}/api/browser/sessions/${encodeURIComponent(id)}/screenshot`,
    );
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `HTTP error ${res.status}`);
    return data.png_base64 || '';
  },

  async browserAct(
    id: string,
    action: { action: 'click' | 'type'; x?: number; y?: number; text?: string },
  ): Promise<void> {
    const res = await this.fetchApi(
      `${this.getBaseUrl()}/api/browser/sessions/${encodeURIComponent(id)}/act`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(action),
      },
    );
    if (!res.ok) {
      const data = await res.json().catch(() => ({}));
      throw new Error(data.error || `HTTP error ${res.status}`);
    }
  },

  async browserCloseSession(id: string): Promise<void> {
    await this.fetchApi(`${this.getBaseUrl()}/api/browser/sessions/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }).catch(() => undefined);
  },

  async getTools(): Promise<any[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/tools`);
    if (!res.ok) return [];
    return res.json();
  },

  async getMemories(): Promise<any[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/memory`);
    if (!res.ok) return [];
    return res.json();
  },

  async deleteMemory(key: string): Promise<boolean> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/memory/${encodeURIComponent(key)}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async listSessions(): Promise<Session[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions`);
    if (!res.ok) throw new Error(`Failed to list sessions: ${res.statusText}`);
    return res.json();
  },

  async createSession(
    title: string,
    mode: string = 'general',
    model: string = 'odon-auto',
    workspaceRoot?: string,
  ): Promise<Session> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title, mode, model, workspace_root: workspaceRoot }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Failed to create session: ${res.statusText}`);
    }
    return res.json();
  },

  async getWorkspaces(): Promise<WorkspaceInfo> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/workspaces`);
    if (!res.ok) return { default: '', allowed: [] };
    return res.json();
  },

  async addWorkspace(path: string): Promise<WorkspaceInfo & { added?: string; error?: string }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/workspaces`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) {
      if (res.status === 404 || res.status === 405) {
        throw new Error(
          `The odond daemon does not support the Workspace API yet (HTTP ${res.status}). Rebuild and restart odond, then try again.`,
        );
      }
      throw new Error(data.error || `Failed to add workspace: ${res.status}`);
    }
    return data;
  },

  async removeWorkspace(path: string): Promise<WorkspaceInfo> {
    const res = await this.fetchApi(
      `${this.getBaseUrl()}/api/workspaces?path=${encodeURIComponent(path)}`,
      { method: 'DELETE' },
    );
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Failed to remove workspace: ${res.status}`);
    }
    return res.json();
  },

  async browseWorkspaces(path?: string): Promise<WorkspaceBrowseResult> {
    const suffix = path ? `?path=${encodeURIComponent(path)}` : '';
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/workspaces/browse${suffix}`);
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      const error =
        res.status === 404 || res.status === 405
          ? `The odond daemon does not support the Workspace API yet (HTTP ${res.status}). Rebuild and restart odond.`
          : err.error || `Failed to browse folders: ${res.status}`;
      return { current: path || '', parent: null, roots: [], entries: [], error };
    }
    return res.json();
  },

  async setSessionWorkspace(sessionId: string, path: string | null): Promise<Session> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${sessionId}/workspace`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Failed to set workspace: ${res.status}`);
    }
    return res.json();
  },

  async getSession(id: string): Promise<{ session: Session; messages: Message[]; artifacts: Artifact[] }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${id}`);
    if (!res.ok) throw new Error(`Failed to get session: ${res.statusText}`);
    return res.json();
  },

  async deleteSession(id: string): Promise<boolean> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async listMessages(sessionId: string): Promise<Message[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${sessionId}/messages`);
    if (!res.ok) return [];
    return res.json();
  },

  async sendMessage(
    sessionId: string,
    text: string,
    model?: string,
    effort?: string,
    attachments?: Array<{ name: string; mime_type: string; data_base64: string }>,
    customInstructions?: string,
    planMode?: boolean,
    mode?: string,
  ): Promise<Message> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${sessionId}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        text,
        model,
        effort,
        custom_instructions: customInstructions,
        plan_mode: planMode,
        mode,
        attachments: attachments || [],
      }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Send failed with status ${res.status}`);
    }
    const data = await res.json();
    return data.message;
  },

  async cancelTurn(sessionId: string): Promise<{ cancelled: boolean }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${sessionId}/cancel`, {
      method: 'POST',
    });
    if (!res.ok) return { cancelled: false };
    return res.json().catch(() => ({ cancelled: false }));
  },

  // A gateway provider is saved with a key but no model ids until something
  // asks it what it serves. Without this the picker stays empty and every send
  // fails with "no model selected".
  async discoverModels(
    providerId: string,
  ): Promise<{ provider_id: string; models: string[] }> {
    const res = await this.fetchApi(
      `${this.getBaseUrl()}/api/providers/${encodeURIComponent(providerId)}/discover-models`,
      { method: 'POST' },
    );
    const data = await res.json().catch(() => ({}));
    if (!res.ok) {
      throw new Error(data?.error || `Model discovery failed with status ${res.status}`);
    }
    return { provider_id: data.provider_id ?? providerId, models: data.models ?? [] };
  },

  async steerMessage(
    sessionId: string,
    text: string,
    attachments?: Array<{ name: string; mime_type: string; data_base64: string }>,
    priority: boolean = false,
    steerId?: string,
  ): Promise<{ queued: boolean; steer_id?: string; priority?: boolean }> {    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${sessionId}/steer`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text, attachments: attachments || [], priority, steer_id: steerId }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Steer failed with status ${res.status}`);
    }
    return res.json();
  },

  async testProvider(providerId: string, model?: string): Promise<{
    ok: boolean;
    provider: string;
    model?: string;
    latency_ms?: number;
    sample?: string;
    error?: string;
  }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/providers/test`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider_id: providerId, model }),
    });
    if (!res.ok) {
      let detail = '';
      try {
        const body = await res.json();
        detail = body.error || JSON.stringify(body);
      } catch {
        detail = await res.text().catch(() => '');
      }
      return { ok: false, provider: providerId, error: detail || `HTTP ${res.status}` };
    }
    return res.json();
  },

  async listArtifacts(sessionId: string): Promise<Artifact[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${sessionId}/artifacts`);
    if (!res.ok) return [];
    return res.json();
  },

  async getTodos(sessionId: string): Promise<TodoItem[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/sessions/${sessionId}/todos`);
    if (!res.ok) return [];
    const data = await res.json();
    return Array.isArray(data?.items) ? data.items : [];
  },

  async getArtifact(id: string): Promise<Artifact> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/artifacts/${id}`);
    if (!res.ok) throw new Error(`Failed to fetch artifact ${id}`);
    return res.json();
  },

  async listArtifactVersions(id: string): Promise<Array<{
    artifact_id: string;
    version: number;
    title: string;
    created_at: string;
  }>> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/artifacts/${id}/versions`);
    if (!res.ok) return [];
    return res.json();
  },

  async restoreArtifactVersion(id: string, version: number): Promise<Artifact> {
    const res = await this.fetchApi(
      `${this.getBaseUrl()}/api/artifacts/${id}/versions/${version}/restore`,
      { method: 'POST' },
    );
    if (!res.ok) throw new Error(`Failed to restore artifact version ${version}`);
    return res.json();
  },

  artifactExportUrl(id: string, format: 'json' | 'md' | 'html'): string {
    return this.authorizeUrl(
      `${this.getBaseUrl()}/api/artifacts/${id}/export?format=${format}`,
    );
  },

  async listTools(): Promise<ToolSchema[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/tools`);
    if (!res.ok) return [];
    return res.json();
  },

  async listApprovals(): Promise<ApprovalRequest[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/approvals`);
    if (!res.ok) return [];
    return res.json();
  },

  async resolveApproval(id: string, decision: 'approve_once' | 'approve_always' | 'approve_all' | 'deny', reason?: string): Promise<boolean> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/approvals/${id}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ decision, reason }),
    });
    return res.ok;
  },

  async getModels(): Promise<{ providers: ProviderItem[]; catalog_as_of?: string }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/models`);
    if (!res.ok) throw new Error(`Failed to fetch models: ${res.statusText}`);
    return res.json();
  },

  async getTerminalInfo(): Promise<TerminalInfo> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/terminal/info`);
    if (!res.ok) throw new Error(`Failed to fetch terminal info: ${res.statusText}`);
    return res.json();
  },

  async executeTerminal(command: string, cwd?: string): Promise<TerminalExecResult> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/terminal/exec`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command, cwd }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Execution failed with status ${res.status}`);
    }
    return res.json();
  },

  async loginOAuth(provider: string): Promise<OAuthLoginResponse> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/oauth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `OAuth sign-in failed with status ${res.status}`);
    }
    return res.json();
  },

  async getOAuthAccounts(): Promise<OAuthAccountInfo[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/oauth/accounts`);
    if (!res.ok) return [];
    return res.json();
  },

  async getProviderConfigs(): Promise<Array<{
    provider_id: string;
    configured: boolean;
    masked_key: string;
    base_url?: string;
    is_default: boolean;
    name?: string;
    models?: string[];
  }>> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/providers/config`);
    if (!res.ok) return [];
    return res.json();
  },

  async saveProviderConfig(
    provider_id: string,
    api_key: string,
    base_url?: string,
    is_default?: boolean,
    name?: string,
    models?: string[],
  ): Promise<any> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/providers/config`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider_id, api_key, base_url, is_default, name, models }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Save failed with status ${res.status}`);
    }
    return res.json();
  },

  async deleteProviderConfig(provider_id: string): Promise<boolean> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/providers/config/${provider_id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async disconnectOAuth(provider: string): Promise<boolean> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/oauth/disconnect`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider }),
    });
    return res.ok;
  },

  // Approvals & Audit Trail
  async getApprovalHistory(): Promise<any[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/approvals/history`);
    if (!res.ok) return [];
    return res.json();
  },

  async getApprovalPolicies(): Promise<string[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/approvals/policies`);
    if (!res.ok) return [];
    return res.json();
  },

  async deleteApprovalPolicy(key: string): Promise<boolean> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/approvals/policies/${encodeURIComponent(key)}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async getSettings(): Promise<{ execution_mode: string }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/status`);
    if (!res.ok) return { execution_mode: 'Autonomous' };
    const data = await res.json();
    return { execution_mode: data.execution_mode || 'Autonomous' };
  },

  async updateSettings(settings: Record<string, unknown>): Promise<{ updated: boolean }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/settings`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(settings),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to update settings: ${res.status}`);
    return data;
  },

  // MCP Servers
  async getMcpServers(): Promise<any[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/mcp/servers`);
    if (!res.ok) return [];
    return res.json();
  },

  async saveMcpServer(server: any): Promise<any> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/mcp/servers`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(server),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to save MCP server: ${res.status}`);
    return data;
  },

  async deleteMcpServer(id: string): Promise<boolean> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/mcp/servers/${id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async toggleMcpServer(id: string): Promise<any> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/mcp/servers/${id}/toggle`, {
      method: 'POST',
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to toggle MCP server: ${res.status}`);
    return data;
  },

  // GitHub
  async getGitHubStatus(): Promise<GitHubStatusResponse> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/github/status`);
    if (!res.ok) return { authenticated: false };
    return res.json();
  },

  async getGitHubUserRepos(): Promise<{ repos: GitHubRepoItem[] }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/github/user-repos`);
    if (!res.ok) return { repos: [] };
    return res.json();
  },

  async getGitHubPrs(owner?: string, repo?: string): Promise<any[]> {
    const params = new URLSearchParams();
    if (owner) params.append('owner', owner);
    if (repo) params.append('repo', repo);
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/github/prs?${params.toString()}`);
    if (!res.ok) return [];
    return res.json();
  },

  async getGitHubIssues(owner?: string, repo?: string, state: string = 'open'): Promise<any[]> {
    const params = new URLSearchParams();
    if (owner) params.append('owner', owner);
    if (repo) params.append('repo', repo);
    params.append('state', state);
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/github/issues?${params.toString()}`);
    if (!res.ok) return [];
    return res.json();
  },

  async createGitHubIssue(owner: string, repo: string, title: string, body?: string): Promise<any> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/github/issues`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ owner, repo, title, body }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to create issue: ${res.status}`);
    return data;
  },

  // Automations
  async getAutomations(): Promise<any[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/automations`);
    if (!res.ok) return [];
    return res.json();
  },

  async saveAutomation(job: any): Promise<any> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/automations`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(job),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to save automation: ${res.status}`);
    return data;
  },

  async deleteAutomation(id: string): Promise<boolean> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/automations/${id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async triggerAutomation(id: string): Promise<any> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/automations/${id}/trigger`, {
      method: 'POST',
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to trigger automation: ${res.status}`);
    return data;
  },

  async getAutomationLogs(): Promise<any[]> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/automations/logs`);
    if (!res.ok) return [];
    return res.json();
  },

  // Relay configuration (runtime, no daemon restart required)
  async getTelegramRelay(): Promise<TelegramRelayStatus> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/relays/telegram`);
    if (!res.ok) return { configured: false, online: false, token_masked: '', allowed_users: [] };
    return res.json();
  },

  async saveTelegramRelay(token: string, allowedUsers: string[]): Promise<TelegramRelayStatus> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/relays/telegram`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token, allowed_users: allowedUsers }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `Failed to connect Telegram: ${res.status}`);
    return data;
  },

  async disconnectTelegramRelay(): Promise<void> {
    await this.fetchApi(`${this.getBaseUrl()}/api/relays/telegram`, { method: 'DELETE' });
  },

  async getWhatsAppBridgeStatus(): Promise<WhatsAppBridgeStatus> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/relays/whatsapp/bridge/status`);
    if (!res.ok) return { running: false, connected: false };
    return res.json();
  },

  async startWhatsAppBridge(
    phone: string | null,
    allowedNumbers: string[],
    allowAnyone: boolean,
  ): Promise<WhatsAppBridgeStatus> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/relays/whatsapp/bridge/start`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ phone, allowed_numbers: allowedNumbers, allow_anyone: allowAnyone }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `Failed to start WhatsApp: ${res.status}`);
    return data;
  },

  async logoutWhatsAppBridge(): Promise<void> {
    await this.fetchApi(`${this.getBaseUrl()}/api/relays/whatsapp/bridge/logout`, {
      method: 'POST',
    });
  },

  // File Explorer APIs
  async fsList(path?: string): Promise<FsListResult> {
    const url = path ? `${this.getBaseUrl()}/api/fs/list?path=${encodeURIComponent(path)}` : `${this.getBaseUrl()}/api/fs/list`;
    const res = await this.fetchApi(url);
    if (!res.ok) return { current_path: '', workspace_root: '', entries: [], error: 'Failed to list directory' };
    return res.json();
  },

  async fsRead(path: string): Promise<FsReadResult> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/fs/read?path=${encodeURIComponent(path)}`);
    if (!res.ok) return { path, name: '', size: 0, is_text: false, error: 'Failed to read file' };
    return res.json();
  },

  async fsWrite(path: string, content: string): Promise<{ success: boolean; error?: string }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/fs/write`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path, content }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to write file: ${res.status}`);
    return data;
  },

  async fsMkdir(path: string): Promise<{ success: boolean; error?: string }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/fs/mkdir`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to create directory: ${res.status}`);
    return data;
  },

  async fsDelete(path: string): Promise<{ success: boolean; error?: string }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/fs/delete?path=${encodeURIComponent(path)}`, {
      method: 'DELETE',
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error((data as { error?: string }).error || `Failed to delete: ${res.status}`);
    return data;
  },

  // SSH Remote APIs
  async getSshProfiles(): Promise<{ profiles: SshProfile[] }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/ssh/profiles`);
    if (!res.ok) return { profiles: [] };
    return res.json();
  },

  async saveSshProfile(profile: SshProfile): Promise<{ success: boolean; profiles: SshProfile[] }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/ssh/profiles`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(profile),
    });
    return res.json();
  },

  async deleteSshProfile(id: string): Promise<{ success: boolean; profiles: SshProfile[] }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/ssh/profiles/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
    return res.json();
  },

  async execSsh(payload: { profile_id?: string; host?: string; port?: number; user?: string; key_path?: string; command: string }): Promise<TerminalExecResult> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/ssh/exec`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    if (!res.ok) {
      return { exit_code: 1, stdout: '', stderr: 'SSH execution request failed', success: false, cwd: '' };
    }
    return res.json();
  },

  async importSshCertificate(filename: string, content: string, profile_id?: string): Promise<{ success: boolean; saved_path: string; filename: string }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/ssh/certificates/import`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ filename, content, profile_id }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Import certificate failed with status ${res.status}`);
    }
    return res.json();
  },

  getWsBaseUrl(): string {
    const explicit = this.getBaseUrl();
    if (explicit) return explicit.replace(/^http/, 'ws');
    if (typeof window !== 'undefined') {
      const scheme = window.location.protocol === 'https:' ? 'wss' : 'ws';
      return `${scheme}://${window.location.host}`;
    }
    return 'ws://127.0.0.1:4096';
  },

  getTerminalWsUrl(params?: { shell?: string; cwd?: string; ssh_profile_id?: string; cols?: number; rows?: number }): string {
    const base = this.getWsBaseUrl();
    const query = new URLSearchParams();
    if (params?.shell) query.set('shell', params.shell);
    if (params?.cwd) query.set('cwd', params.cwd);
    if (params?.ssh_profile_id) query.set('ssh_profile_id', params.ssh_profile_id);
    if (params?.cols) query.set('cols', params.cols.toString());
    if (params?.rows) query.set('rows', params.rows.toString());
    const token = this.getApiToken();
    if (token) query.set('token', token);
    const qs = query.toString();
    return `${base}/api/terminal/ws${qs ? `?${qs}` : ''}`;
  },

  // Skills APIs
  async getSkills(): Promise<{ skills: SkillItemInfo[] }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/skills`);
    if (!res.ok) return { skills: [] };
    return res.json();
  },

  async saveSkill(name: string, content: string): Promise<{ success: boolean; path?: string; error?: string }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/skills`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, content }),
    });
    return res.json();
  },

  async deleteSkill(name: string): Promise<{ success: boolean }> {
    const res = await this.fetchApi(`${this.getBaseUrl()}/api/skills/${encodeURIComponent(name)}`, {
      method: 'DELETE',
    });
    return res.json();
  },

  createTerminalWs(): WebSocket {
    return new WebSocket(this.getTerminalWsUrl());
  },

  subscribeEvents(
    onEvent: (event: AgentEvent) => void,
    onReconnect?: () => void,
    onDisconnect?: () => void,
    activeSessionId?: string | null,
  ): () => void {
    let es: EventSource | null = null;
    let closed = false;
    let timer: number | undefined;
    let retryDelay = 1000;
    let connected = false;

    const scheduleOpen = () => {
      if (closed || timer !== undefined) return;
      timer = window.setTimeout(() => {
        timer = undefined;
        open();
      }, retryDelay);
      retryDelay = Math.min(retryDelay * 2, 15000);
    };

    const open = () => {
      if (closed) return;
      const token = this.getApiToken();
      const params = new URLSearchParams();
      if (token) params.set('token', token);
      if (activeSessionId) params.set('session_id', activeSessionId);
      const next = new EventSource(
        `${this.getBaseUrl()}/api/events${params.toString() ? '?' + params.toString() : ''}`,
      );
      es = next;
      next.onopen = () => {
        retryDelay = 1000;
        const wasConnected = connected;
        connected = true;
        if (wasConnected) onReconnect?.();
      };
      // Events for every session arrive here; callers gate them with
      // isAgentEventForSession before applying anything.
      next.onmessage = (e) => {
        try {
          const parsed: AgentEvent = JSON.parse(e.data);
          onEvent(parsed);
        } catch (err) {
          console.error('Failed to parse SSE event:', err);
        }
      };
      next.onerror = () => {
        if (closed || es !== next) return;
        connected = false;
        next.close();
        es = null;
        onDisconnect?.();
        scheduleOpen();
      };
    };

    // The desktop shell hands the daemon token over after boot. Opening the
    // stream before that hands it a token-less URL, and EventSource never
    // re-reads the URL, so the session would stay silent for its lifetime.
    const shellPresent =
      typeof (window as unknown as { __TAURI__?: { core?: { invoke?: unknown } } }).__TAURI__
        ?.core?.invoke === 'function';
    if (!shellPresent || this.getApiToken()) {
      open();
    } else {
      let attempts = 0;
      const waitForToken = () => {
        if (closed) return;
        if (this.getApiToken() || attempts >= 100) {
          open();
          return;
        }
        attempts += 1;
        timer = window.setTimeout(waitForToken, 100);
      };
      waitForToken();
    }

    return () => {
      closed = true;
      if (timer !== undefined) window.clearTimeout(timer);
      es?.close();
      es = null;
    };
  },
};
