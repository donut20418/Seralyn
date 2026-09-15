import { useState, useCallback } from 'react';
import type { ProviderStatus, ProviderKind } from '../lib/types';
import * as api from '../lib/api';

export function useProviders() {
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
  const [activeProvider, setActiveProvider] = useState<ProviderKind>('claude');
  const [isDetecting, setIsDetecting] = useState(false);

  const detectProviders = useCallback(async () => {
    setIsDetecting(true);
    try {
      const status = await api.detectProviders();
      setProviders(status);
    } catch (e) {
      console.error(e);
    } finally {
      setIsDetecting(false);
    }
  }, []);

  return {
    providers,
    activeProvider,
    isDetecting,
    detectProviders,
    setActiveProvider
  };
}
