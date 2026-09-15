import React from 'react';

interface ApprovalDialogProps {
  toolName: string;
  description: string;
  input: string;
  onApprove: () => void;
  onDeny: () => void;
}

export function ApprovalDialog({ toolName, description, input, onApprove, onDeny }: ApprovalDialogProps) {
  return (
    <div className="modal-overlay">
      <div className="approval-dialog">
        <h3>Tool Approval Required</h3>
        <p><strong>Tool:</strong> {toolName}</p>
        <p><strong>Description:</strong> {description}</p>
        <div className="tool-input">
          <pre>{input}</pre>
        </div>
        <div className="actions">
          <button className="deny" onClick={onDeny}>Deny</button>
          <button className="approve" onClick={onApprove}>Approve</button>
        </div>
      </div>
    </div>
  );
}
