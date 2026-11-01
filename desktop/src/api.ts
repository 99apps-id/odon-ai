import {
  Session,
  Message,
  Artifact,
  ToolSchema,
  ApprovalRequest,
  SystemStatus,
  AgentEvent,
  ProviderItem,
  TerminalInfo,
  TerminalExecResult,
} from './types';

const API_BASE = (typeof window !== 'undefined' && localStorage.getItem('odon_api_base')) || 'http://127.0.0.1:4096';

export const api = {
  getBaseUrl(): string {
    return (typeof window !== 'undefined' && localStorage.getItem('odon_api_base')) || API_BASE;
  },

  setBaseUrl(url: string) {
    if (typeof window !== 'undefined') {
      localStorage.setItem('odon_api_base', url.replace(/\/+$/, ''));
    }
  },

  async getStatus(): Promise<SystemStatus> {
    const res = await fetch(`${this.getBaseUrl()}/api/status`);
    if (!res.ok) throw new Error(`HTTP error ${res.status}`);
    return res.json();
  },

  async listSessions(): Promise<Session[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions`);
    if (!res.ok) throw new Error(`Failed to list sessions: ${res.statusText}`);
    return res.json();
  },

  async createSession(title: string, mode: string = 'general', model: string = 'odon-auto'): Promise<Session> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title, mode, model }),
    });
    if (!res.ok) throw new Error(`Failed to create session: ${res.statusText}`);
    return res.json();
  },

  async getSession(id: string): Promise<{ session: Session; messages: Message[]; artifacts: Artifact[] }> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${id}`);
    if (!res.ok) throw new Error(`Failed to get session: ${res.statusText}`);
    return res.json();
  },

  async deleteSession(id: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async listMessages(sessionId: string): Promise<Message[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${sessionId}/messages`);
    if (!res.ok) return [];
    return res.json();
  },

  async sendMessage(sessionId: string, text: string, model?: string): Promise<Message> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${sessionId}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text, model }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Send failed with status ${res.status}`);
    }
    const data = await res.json();
    return data.message;
  },

  async listArtifacts(sessionId: string): Promise<Artifact[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${sessionId}/artifacts`);
    if (!res.ok) return [];
    return res.json();
  },

  async getArtifact(id: string): Promise<Artifact> {
    const res = await fetch(`${this.getBaseUrl()}/api/artifacts/${id}`);
    if (!res.ok) throw new Error(`Failed to fetch artifact ${id}`);
    return res.json();
  },

  async listTools(): Promise<ToolSchema[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/tools`);
    if (!res.ok) return [];
    return res.json();
  },

  async listApprovals(): Promise<ApprovalRequest[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/approvals`);
    if (!res.ok) return [];
    return res.json();
  },

  async resolveApproval(id: string, decision: 'approved' | 'rejected', reason?: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/approvals/${id}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ decision, reason }),
    });
    return res.ok;
  },

  async getModels(): Promise<{ providers: ProviderItem[] }> {
    const res = await fetch(`${this.getBaseUrl()}/api/models`);
    if (!res.ok) throw new Error(`Failed to fetch models: ${res.statusText}`);
    return res.json();
  },

  async getTerminalInfo(): Promise<TerminalInfo> {
    const res = await fetch(`${this.getBaseUrl()}/api/terminal/info`);
    if (!res.ok) throw new Error(`Failed to fetch terminal info: ${res.statusText}`);
    return res.json();
  },

  async executeTerminal(command: string, cwd?: string): Promise<TerminalExecResult> {
    const res = await fetch(`${this.getBaseUrl()}/api/terminal/exec`, {
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

  createTerminalWs(): WebSocket {
    const baseUrl = this.getBaseUrl();
    const wsUrl = baseUrl.replace(/^http/, 'ws') + '/api/terminal/ws';
    return new WebSocket(wsUrl);
  },

  subscribeEvents(onEvent: (event: AgentEvent) => void): () => void {
    const es = new EventSource(`${this.getBaseUrl()}/api/events`);

    es.onmessage = (e) => {
      try {
        const parsed = JSON.parse(e.data);
        onEvent(parsed);
      } catch (err) {
        console.error('Failed to parse SSE event:', err);
      }
    };

    es.onerror = () => {
      // EventSource reconnects automatically
    };

    return () => {
      es.close();
    };
  },
};
