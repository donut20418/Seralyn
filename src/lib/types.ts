export type ProviderKind = 'claude' | 'codex' | 'gemini';
export type EventType = 
  | 'SessionStarted' 
  | 'TextDelta' 
  | 'ThinkingDelta' 
  | 'ToolStarted' 
  | 'ToolProgress' 
  | 'ToolResult' 
  | 'ApprovalRequired' 
  | 'AttachmentReceived' 
  | 'FileChange' 
  | 'UsageUpdated' 
  | 'ContextUpdated' 
  | 'Error' 
  | 'SessionFinished';

export type TokenConfidence = 'Exact' | 'Estimated';

export interface NormalizedEvent {
  id: string;
  event_type: EventType;
  provider: ProviderKind;
  conversation_id: string;
  provider_session_id?: string;
  timestamp: string;
  payload: EventPayload;
  raw_provider_event?: any;
}

export type EventPayload =
  | { kind: 'Text'; content: string }
  | { kind: 'Thinking'; content: string }
  | { kind: 'Tool'; tool_id: string; tool_name: string; input?: any; output?: any; status?: string }
  | { kind: 'Approval'; approval_id: string; tool_name: string; description: string; input?: any }
  | { 
      kind: 'Usage'; 
      input_tokens?: number; 
      output_tokens?: number; 
      cache_read_tokens?: number; 
      reasoning_tokens?: number; 
      context_tokens?: number; 
      context_window?: number; 
      confidence: TokenConfidence 
    }
  | { kind: 'Error'; code?: string; message: string }
  | { kind: 'Session'; session_id?: string; model?: string }
  | { kind: 'FileChange'; path: string; change_type: string }
  | { kind: 'Empty' };

export interface Conversation {
  id: string;
  title?: string;
  created_at: string;
  updated_at: string;
  archived: boolean;
  metadata_json?: string;
}

export interface ConversationSummary {
  id: string;
  title?: string;
  created_at: string;
  updated_at: string;
  last_message_preview?: string;
  message_count: number;
  provider?: ProviderKind;
}

export interface Message {
  id: string;
  conversation_id: string;
  parent_id?: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  provider?: string;
  model?: string;
  provider_session_id?: string;
  created_at: string;
  token_estimate?: number;
  metadata_json?: string;
}

export interface ProviderSessionRecord {
  id: string;
  conversation_id: string;
  provider: string;
  provider_session_id?: string;
  model?: string;
  created_at: string;
  last_used_at?: string;
  status: string;
  metadata_json?: string;
}

export interface ConversationWithMessages {
  conversation: Conversation;
  messages: Message[];
  provider_sessions: ProviderSessionRecord[];
}

export interface ProviderCapabilities {
  text: boolean;
  image: boolean;
  file: boolean;
  tools: boolean;
  skills: boolean;
  mcp: boolean;
  resume: boolean;
  native_compact: boolean;
  token_usage: boolean;
  context_window: boolean;
  reasoning: boolean;
  streaming: boolean;
}

export interface ProviderStatus {
  provider: ProviderKind;
  installed: boolean;
  version?: string;
  authenticated: 'Authenticated' | 'NotAuthenticated' | 'Unknown';
  capabilities: ProviderCapabilities;
  error?: string;
}
