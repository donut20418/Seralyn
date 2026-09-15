import { useEffect } from 'react';
import { listenForConversationEvents } from '../lib/events';
import type { NormalizedEvent } from '../lib/types';

export function useEvents(callback: (event: NormalizedEvent) => void) {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    
    listenForConversationEvents(callback).then(fn => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, [callback]);
}
