import { useState } from 'react';
import { X, Copy, Check, Server, Layers, ListOrdered } from 'lucide-react';
import type { Conversation, Message, ProviderSessionRecord } from '../../lib/types';

interface ContextInspectorProps {
  conversation: Conversation | null;
  messages: Message[];
  providerSessions: ProviderSessionRecord[];
  onClose: () => void;
}

export function ContextInspector({
  conversation,
  messages,
  providerSessions,
  onClose,
}: ContextInspectorProps) {
  const [activeTab, setActiveTab] = useState<'sessions' | 'cursor' | 'turns'>('sessions');
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const maxSeq = messages.reduce((max, m) => Math.max(max, m.seq || 0), 0);

  const handleCopy = async (text: string, id: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      setTimeout(() => setCopiedId(null), 2000);
    } catch (e) {
      console.error('Failed to copy text:', e);
    }
  };

  return (
    <div className="inspector-overlay" onClick={onClose}>
      <div className="inspector-panel" onClick={e => e.stopPropagation()}>
        <div className="inspector-header">
          <div className="inspector-title">
            <Layers size={18} />
            <span>Context Inspector</span>
          </div>
          <button className="inspector-close-btn" onClick={onClose} title="Close">
            <X size={18} />
          </button>
        </div>

        <div className="inspector-tabs">
          <button
            className={`inspector-tab ${activeTab === 'sessions' ? 'active' : ''}`}
            onClick={() => setActiveTab('sessions')}
          >
            <Server size={14} />
            <span>Native Sessions ({providerSessions.length})</span>
          </button>
          <button
            className={`inspector-tab ${activeTab === 'cursor' ? 'active' : ''}`}
            onClick={() => setActiveTab('cursor')}
          >
            <Layers size={14} />
            <span>Sync Cursor</span>
          </button>
          <button
            className={`inspector-tab ${activeTab === 'turns' ? 'active' : ''}`}
            onClick={() => setActiveTab('turns')}
          >
            <ListOrdered size={14} />
            <span>Turn Tree ({messages.length})</span>
          </button>
        </div>

        <div className="inspector-body">
          {activeTab === 'sessions' && (
            <div className="sessions-view">
              <p className="inspector-description">
                CLI provider backend sessions tied to conversation <code>{conversation?.id?.slice(0, 8)}</code>.
              </p>
              {providerSessions.length === 0 ? (
                <div className="empty-notice">No native provider sessions started yet.</div>
              ) : (
                providerSessions.map(ps => (
                  <div key={ps.id} className="session-card">
                    <div className="session-card-header">
                      <span className={`provider-badge ${ps.provider}`}>
                        {ps.provider.toUpperCase()}
                      </span>
                      <span className={`session-status ${ps.status}`}>{ps.status}</span>
                    </div>
                    <div className="session-detail-row">
                      <span className="detail-label">Native Session ID:</span>
                      <div className="detail-value-copy">
                        <code title={ps.provider_session_id || 'Not assigned'}>
                          {ps.provider_session_id || 'None (in-memory)'}
                        </code>
                        {ps.provider_session_id && (
                          <button
                            className="copy-btn-small"
                            onClick={() => handleCopy(ps.provider_session_id!, ps.id)}
                            title="Copy native session ID"
                          >
                            {copiedId === ps.id ? <Check size={12} /> : <Copy size={12} />}
                          </button>
                        )}
                      </div>
                    </div>
                    {ps.model && (
                      <div className="session-detail-row">
                        <span className="detail-label">Model:</span>
                        <span className="detail-val">{ps.model}</span>
                      </div>
                    )}
                    <div className="session-detail-row">
                      <span className="detail-label">Synced Through Seq:</span>
                      <span className="detail-val font-mono">{ps.synced_through_seq ?? 0}</span>
                    </div>
                    {ps.last_used_at && (
                      <div className="session-detail-row">
                        <span className="detail-label">Last Active:</span>
                        <span className="detail-val">{new Date(ps.last_used_at).toLocaleTimeString()}</span>
                      </div>
                    )}
                  </div>
                ))
              )}
            </div>
          )}

          {activeTab === 'cursor' && (
            <div className="cursor-view">
              <p className="inspector-description">
                SQLite Sync Cursor tracks which turns each provider backend has processed. When switching providers (e.g. Claude → Codex → Gemini → Claude), only the missing delta messages are injected.
              </p>
              <div className="cursor-summary-box">
                <div className="cursor-max-seq">
                  <span className="label">Current Conversation Max Seq:</span>
                  <span className="val">{maxSeq}</span>
                </div>
              </div>

              <div className="cursor-providers-list">
                {['claude', 'codex', 'gemini'].map(pKind => {
                  const sess = providerSessions.find(s => s.provider === pKind);
                  const syncedSeq = sess?.synced_through_seq ?? 0;
                  const deltaCount = Math.max(0, maxSeq - syncedSeq);
                  const isUpToDate = maxSeq > 0 && deltaCount === 0;

                  return (
                    <div key={pKind} className="cursor-provider-row">
                      <div className="provider-info">
                        <span className={`provider-badge ${pKind}`}>{pKind}</span>
                        <span className="synced-text">Synced: seq {syncedSeq}</span>
                      </div>
                      <div className="delta-status">
                        {isUpToDate ? (
                          <span className="delta-badge uptodate">Up to date</span>
                        ) : deltaCount > 0 ? (
                          <span className="delta-badge pending">
                            +{deltaCount} delta {deltaCount === 1 ? 'turn' : 'turns'} needed
                          </span>
                        ) : (
                          <span className="delta-badge idle">Uninitialized</span>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          {activeTab === 'turns' && (
            <div className="turns-view">
              <div className="turn-list">
                {messages.map(m => (
                  <div key={m.id} className={`turn-item turn-${m.role}`}>
                    <div className="turn-meta">
                      <span className="turn-seq">#{m.seq || '-'}</span>
                      <span className="turn-role">{m.role}</span>
                      {m.provider && (
                        <span className={`turn-provider ${m.provider}`}>[{m.provider}]</span>
                      )}
                      <span className="turn-time">
                        {new Date(m.created_at).toLocaleTimeString()}
                      </span>
                    </div>
                    <div className="turn-preview">
                      {m.content.slice(0, 140)}
                      {m.content.length > 140 ? '...' : ''}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
