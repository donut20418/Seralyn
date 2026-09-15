import { listen, UnlistenFn } from '@tauri-apps/api/event';
import type { NormalizedEvent } from './types';

export function listenForConversationEvents(callback: (event: NormalizedEvent) => void): Promise<UnlistenFn> {
  return listen<NormalizedEvent>('conversation-event', (event) => {
    callback(event.payload);
  });
}
