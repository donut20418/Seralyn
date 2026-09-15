import React from 'react';
import { ProviderKind } from '../../lib/types';

interface StreamingTextProps {
  content: string;
  provider: ProviderKind;
}

export function StreamingText({ content, provider }: StreamingTextProps) {
  return (
    <div className="message-bubble assistant streaming">
      <div className={`provider-badge ${provider}`}>
        [{provider}]
      </div>
      <div className="message-content">
        <pre>{content}<span className="cursor" /></pre>
      </div>
    </div>
  );
}
