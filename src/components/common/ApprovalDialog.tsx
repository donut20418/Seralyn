import { useEffect } from 'react';
import { ShieldAlert, Check, X } from 'lucide-react';
import type { PendingApproval } from '../../lib/types';

interface ApprovalDialogProps {
  approval: PendingApproval;
  onApprove: () => void;
  onDeny: () => void;
}

export function ApprovalDialog({ approval, onApprove, onDeny }: ApprovalDialogProps) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onDeny();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onApprove, onDeny]);

  const formatInput = (input: any) => {
    if (!input) return null;
    if (typeof input === 'string') {
      try {
        const parsed = JSON.parse(input);
        return JSON.stringify(parsed, null, 2);
      } catch {
        return input;
      }
    }
    return JSON.stringify(input, null, 2);
  };

  const formattedInput = formatInput(approval.input);

  return (
    <div className="modal-overlay" onClick={onDeny}>
      <div className="approval-dialog" onClick={e => e.stopPropagation()}>
        <div className="approval-dialog-header">
          <div className="dialog-title-group">
            <ShieldAlert size={20} className="shield-icon" />
            <h3>Action Approval Required</h3>
          </div>
          <span className="safe-mode-badge">Safe Mode</span>
        </div>

        <div className="approval-dialog-content">
          <div className="approval-field">
            <span className="field-label">Tool / Operation:</span>
            <span className="field-tool-name">{approval.tool_name}</span>
          </div>

          {approval.description && (
            <div className="approval-field">
              <span className="field-label">Description:</span>
              <p className="field-desc">{approval.description}</p>
            </div>
          )}

          {formattedInput && (
            <div className="approval-field input-field">
              <span className="field-label">Requested Command / Parameters:</span>
              <pre className="approval-payload">{formattedInput}</pre>
            </div>
          )}
        </div>

        <div className="approval-dialog-actions">
          <button className="deny-btn" onClick={onDeny} title="Deny (Esc)">
            <X size={15} />
            <span>Deny</span>
          </button>
          <button className="approve-btn" onClick={onApprove} title="Approve">
            <Check size={15} />
            <span>Approve & Continue</span>
          </button>
        </div>
      </div>
    </div>
  );
}
