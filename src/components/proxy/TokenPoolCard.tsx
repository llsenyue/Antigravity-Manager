import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { Cloud, Wifi, WifiOff, RefreshCw, DollarSign } from 'lucide-react';
import { showToast } from '../common/ToastContainer';
import { useConfigStore } from '../../stores/useConfigStore';

interface TokenPoolStatus {
    status: string;
    supplier_id: string | null;
    enabled: boolean;
}

interface TokenPoolCardProps {
    defaultExpanded?: boolean;
}

export default function TokenPoolCard({ defaultExpanded = false }: TokenPoolCardProps) {
    const { t } = useTranslation();
    const [isExpanded, setIsExpanded] = useState(defaultExpanded);
    const [status, setStatus] = useState<TokenPoolStatus>({
        status: 'Disconnected',
        supplier_id: null,
        enabled: false,
    });
    const [loading, setLoading] = useState(false);

    // Read from global config
    const { config } = useConfigStore();
    const serverUrl = config?.token_pool?.server_url || 'ws://127.0.0.1:8046/ws/supplier';



    // 加载状态
    const loadStatus = useCallback(async () => {
        try {
            const s = await invoke<TokenPoolStatus>('tokenpool_status');
            setStatus(s);
        } catch (error) {
            console.error('Failed to load TokenPool status:', error);
        }
    }, []);

    useEffect(() => {
        loadStatus();
        const interval = setInterval(loadStatus, 5000);
        return () => clearInterval(interval);
    }, [loadStatus]);

    // 连接/断开
    const handleToggle = async () => {
        setLoading(true);
        try {
            if (status.enabled) {
                await invoke('tokenpool_disconnect');
                showToast(t('tokenpool.disconnected') || 'Disconnected from TokenPool', 'success');
            } else {
                await invoke('tokenpool_connect', { server_url: serverUrl });
                showToast(t('tokenpool.connected') || 'Connected to TokenPool', 'success');
            }
            await loadStatus();
        } catch (error: any) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setLoading(false);
        }
    };

    const isConnected = status.status === 'Connected';

    return (
        <div className="bg-gradient-to-r from-purple-50 to-indigo-50 dark:from-purple-900/20 dark:to-indigo-900/20 rounded-xl shadow-sm border border-purple-200 dark:border-purple-800 overflow-hidden transition-all duration-200 hover:shadow-md">
            <div
                className="px-5 py-4 flex items-center justify-between cursor-pointer bg-purple-100/50 dark:bg-purple-900/30 hover:bg-purple-100 dark:hover:bg-purple-900/50 transition-colors"
                onClick={(e) => {
                    if ((e.target as HTMLElement).closest('.no-expand')) return;
                    setIsExpanded(!isExpanded);
                }}
            >
                <div className="flex items-center gap-3">
                    <div className="p-2 rounded-lg bg-purple-500 text-white">
                        <Cloud size={18} />
                    </div>
                    <div>
                        <span className="font-semibold text-sm text-gray-900 dark:text-gray-100">
                            TokenPool
                        </span>
                        <p className="text-[10px] text-gray-500 dark:text-gray-400">
                            {t('tokenpool.description') || 'Share your idle quota to earn rewards'}
                        </p>
                    </div>
                    <div className={`text-xs px-2 py-0.5 rounded-full flex items-center gap-1 ${isConnected
                        ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
                        : 'bg-gray-100 text-gray-500 dark:bg-gray-700 dark:text-gray-400'
                        }`}>
                        {isConnected ? <Wifi size={12} /> : <WifiOff size={12} />}
                        {isConnected ? (t('tokenpool.status.connected') || 'Connected') : (t('tokenpool.status.disconnected') || 'Disconnected')}
                    </div>
                </div>

                <div className="flex items-center gap-4 no-expand">
                    {/* 连接开关 */}
                    <div className="flex items-center" onClick={(e) => e.stopPropagation()}>
                        <button
                            onClick={handleToggle}
                            disabled={loading}
                            className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-2 ${isConnected
                                ? 'bg-red-500 hover:bg-red-600 text-white'
                                : 'bg-purple-500 hover:bg-purple-600 text-white shadow-sm shadow-purple-500/30'
                                } ${loading ? 'opacity-50 cursor-not-allowed' : ''}`}
                        >
                            {loading ? (
                                <RefreshCw size={14} className="animate-spin" />
                            ) : isConnected ? (
                                <WifiOff size={14} />
                            ) : (
                                <Wifi size={14} />
                            )}
                            {loading
                                ? (t('common.loading') || 'Loading...')
                                : isConnected
                                    ? (t('tokenpool.action.disconnect') || 'Disconnect')
                                    : (t('tokenpool.action.connect') || 'Connect')
                            }
                        </button>
                    </div>

                    <button
                        className={`p-1 rounded-lg hover:bg-purple-200 dark:hover:bg-purple-800 transition-all duration-200 ${isExpanded ? 'rotate-180' : ''}`}
                    >
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                            <path d="m6 9 6 6 6-6" />
                        </svg>
                    </button>
                </div>
            </div>

            <div
                className={`transition-all duration-300 ease-in-out border-t border-purple-200 dark:border-purple-800 ${isExpanded ? 'max-h-[500px] opacity-100' : 'max-h-0 opacity-0 overflow-hidden'
                    }`}
            >
                <div className="p-5 space-y-4">
                    {/* 服务器地址已移至设置页面 */}
                    <div className="text-xs text-gray-500 dark:text-gray-400 font-mono bg-gray-50 dark:bg-base-300 px-3 py-2 rounded-lg border border-gray-100 dark:border-base-200 truncate">
                        {serverUrl}
                    </div>

                    {/* 状态信息 */}
                    {isConnected && status.supplier_id && (
                        <div className="bg-white dark:bg-base-200 rounded-lg p-4 border border-gray-200 dark:border-base-300">
                            <h4 className="text-xs font-semibold text-gray-700 dark:text-gray-300 mb-3 flex items-center gap-2">
                                <DollarSign size={14} />
                                {t('tokenpool.stats') || 'Statistics'}
                            </h4>
                            <div className="grid grid-cols-2 gap-4">
                                <div>
                                    <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                        {t('tokenpool.supplier_id') || 'Supplier ID'}
                                    </p>
                                    <p className="text-xs font-mono text-gray-900 dark:text-gray-100 truncate">
                                        {status.supplier_id.slice(0, 8)}...
                                    </p>
                                </div>
                                <div>
                                    <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                        {t('tokenpool.status.label') || 'Status'}
                                    </p>
                                    <p className="text-xs text-green-600 dark:text-green-400 flex items-center gap-1">
                                        <span className="w-2 h-2 bg-green-500 rounded-full animate-pulse"></span>
                                        {t('tokenpool.status.sharing') || 'Sharing quota...'}
                                    </p>
                                </div>
                            </div>
                        </div>
                    )}

                    {/* 说明 */}
                    <div className="text-[10px] text-gray-500 dark:text-gray-400 space-y-1">
                        <p>• {t('tokenpool.hint.1') || 'Your OAuth token never leaves your device'}</p>
                        <p>• {t('tokenpool.hint.2') || 'Requests are forwarded through your local proxy'}</p>
                        <p>• {t('tokenpool.hint.3') || 'Earn rewards for every API call processed'}</p>
                    </div>
                </div>
            </div>
        </div>
    );
}
