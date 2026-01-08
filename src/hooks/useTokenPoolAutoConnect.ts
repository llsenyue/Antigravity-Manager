import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useConfigStore } from '../stores/useConfigStore';


export function useTokenPoolAutoConnect() {
    const { config } = useConfigStore();
    // const { t } = useTranslation();
    const isConnecting = useRef(false);
    const retryInterval = useRef<ReturnType<typeof setInterval> | null>(null);

    useEffect(() => {
        if (!config?.token_pool?.auto_connect) {
            // Remove retry interval if feature is disabled
            if (retryInterval.current) {
                clearInterval(retryInterval.current);
                retryInterval.current = null;
            }
            return;
        }

        const checkAndConnect = async () => {
            if (isConnecting.current) return;

            try {
                // Check current status
                const status = await invoke<{ status: string }>('tokenpool_status');

                // If not connected and not connecting/error state that needs manual intervention
                if (status.status === 'Disconnected' || status.status.startsWith('Error')) {
                    isConnecting.current = true;
                    console.log('[TokenPool] Auto-connecting...');

                    const serverUrl = config.token_pool?.server_url || 'ws://127.0.0.1:8046/ws/supplier';

                    try {
                        await invoke('tokenpool_connect', { server_url: serverUrl });
                        console.log('[TokenPool] Connected successfully');
                        // showToast(t('tokenpool.connected') || 'Connected to TokenPool', 'success');
                    } catch (error) {
                        console.error('[TokenPool] Auto-connect failed:', error);
                    } finally {
                        isConnecting.current = false;
                    }
                }
            } catch (error) {
                console.error('[TokenPool] Failed to check status:', error);
            }
        };

        // Initial check
        checkAndConnect();

        // Setup retry interval (every 10 seconds) for continuous monitoring/reconnecting
        retryInterval.current = setInterval(checkAndConnect, 10000);

        return () => {
            if (retryInterval.current) {
                clearInterval(retryInterval.current);
                retryInterval.current = null;
            }
        };
    }, [config?.token_pool?.auto_connect, config?.token_pool?.server_url]); // Re-run if config changes
}
