import React from 'react';
import { useTranslation } from 'react-i18next';
import { Sparkles, Check, Clock, Zap, Plus, X } from 'lucide-react';
import { ScheduledWarmupConfig } from '../../types/config';

interface SmartWarmupProps {
    config: ScheduledWarmupConfig;
    onChange: (config: ScheduledWarmupConfig) => void;
}

const SmartWarmup: React.FC<SmartWarmupProps> = ({ config, onChange }) => {
    const { t } = useTranslation();

    const warmupModelsOptions = [
        { id: 'gemini-3-flash', label: 'Gemini 3 Flash' },
        { id: 'gemini-3-pro-high', label: 'Gemini 3 Pro High' },
        { id: 'claude-sonnet-4-5', label: 'Claude 4.5 Sonnet' },
        { id: 'gemini-3-pro-image', label: 'Gemini 3 Pro Image' }
    ];

    // 确保 peak_hours 有默认值
    const peakHours = config.peak_hours || ['10:00', '15:00', '20:00'];
    const warmupMode = config.warmup_mode || 'peak_based';

    const handleEnabledChange = (enabled: boolean) => {
        let newConfig = { ...config, enabled };
        // 如果开启预热且勾选列表为空，则默认勾选所有核心模型
        if (enabled && (!config.monitored_models || config.monitored_models.length === 0)) {
            newConfig.monitored_models = warmupModelsOptions.map(o => o.id);
        }
        // 确保有默认的高峰期时间和模式
        if (!newConfig.peak_hours || newConfig.peak_hours.length === 0) {
            newConfig.peak_hours = ['10:00', '15:00', '20:00'];
        }
        if (!newConfig.warmup_mode) {
            newConfig.warmup_mode = 'peak_based';
        }
        onChange(newConfig);
    };

    const toggleModel = (model: string) => {
        const currentModels = config.monitored_models || [];
        let newModels: string[];

        if (currentModels.includes(model)) {
            // 必须勾选其中一个，不能全取消
            if (currentModels.length <= 1) return;
            newModels = currentModels.filter(m => m !== model);
        } else {
            newModels = [...currentModels, model];
        }

        onChange({ ...config, monitored_models: newModels });
    };

    const handleModeChange = (mode: 'immediate' | 'peak_based') => {
        onChange({ ...config, warmup_mode: mode });
    };

    const handlePeakHourChange = (index: number, value: string) => {
        const newPeakHours = [...peakHours];
        newPeakHours[index] = value;
        onChange({ ...config, peak_hours: newPeakHours });
    };

    const addPeakHour = () => {
        if (peakHours.length >= 6) return; // 最多6个高峰期
        onChange({ ...config, peak_hours: [...peakHours, '12:00'] });
    };

    const removePeakHour = (index: number) => {
        if (peakHours.length <= 1) return; // 至少保留1个
        const newPeakHours = peakHours.filter((_, i) => i !== index);
        onChange({ ...config, peak_hours: newPeakHours });
    };

    // 计算预热时间（高峰期前5小时）
    const calculateWarmupTime = (peakTime: string): string => {
        const [h, m] = peakTime.split(':').map(Number);
        const peakMinutes = h * 60 + m;
        const warmupMinutes = peakMinutes >= 300 ? peakMinutes - 300 : 1440 + peakMinutes - 300;
        const warmupH = Math.floor(warmupMinutes / 60);
        const warmupM = warmupMinutes % 60;
        return `${warmupH.toString().padStart(2, '0')}:${warmupM.toString().padStart(2, '0')}`;
    };

    return (
        <div className="space-y-4">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <div className={`w-10 h-10 rounded-xl flex items-center justify-center transition-all duration-300 ${config.enabled
                        ? 'bg-orange-500 text-white'
                        : 'bg-orange-50 dark:bg-orange-900/20 text-orange-500'
                        }`}>
                        <Sparkles size={20} />
                    </div>
                    <div>
                        <div className="font-bold text-gray-900 dark:text-gray-100">
                            {t('settings.warmup.title', '智能预热')}
                        </div>
                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                            {t('settings.warmup.desc', '自动监控所有模型，当配额恢复到 100% 时立即触发预热')}
                        </p>
                    </div>
                </div>
                <label className="relative inline-flex items-center cursor-pointer">
                    <input
                        type="checkbox"
                        className="sr-only peer"
                        checked={config.enabled}
                        onChange={(e) => handleEnabledChange(e.target.checked)}
                    />
                    <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-orange-500 shadow-inner"></div>
                </label>
            </div>

            {config.enabled && (
                <div className="mt-4 pt-4 border-t border-gray-50 dark:border-base-300 animate-in slide-in-from-top-2 duration-300">
                    <div className="space-y-4">
                        {/* 模式切换 */}
                        <div>
                            <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-widest block mb-2">
                                {t('settings.warmup.mode_label', '预热模式')}
                            </label>
                            <div className="grid grid-cols-2 gap-2">
                                <div
                                    onClick={() => handleModeChange('immediate')}
                                    className={`
                                        flex items-center gap-2 p-3 rounded-lg border cursor-pointer transition-all duration-200
                                        ${warmupMode === 'immediate'
                                            ? 'bg-orange-50 dark:bg-orange-900/10 border-orange-200 dark:border-orange-800/50'
                                            : 'bg-gray-50/50 dark:bg-base-200/50 border-gray-100 dark:border-base-300/50 hover:border-gray-200 dark:hover:border-base-300'}
                                    `}
                                >
                                    <Zap size={16} className={warmupMode === 'immediate' ? 'text-orange-500' : 'text-gray-400'} />
                                    <div>
                                        <div className={`text-xs font-medium ${warmupMode === 'immediate' ? 'text-orange-700 dark:text-orange-400' : 'text-gray-600 dark:text-gray-400'}`}>
                                            {t('settings.warmup.mode_immediate', '即时预热')}
                                        </div>
                                        <div className="text-[10px] text-gray-400 dark:text-gray-500 mt-0.5">
                                            {t('settings.warmup.mode_immediate_desc', '100% 即预热')}
                                        </div>
                                    </div>
                                </div>
                                <div
                                    onClick={() => handleModeChange('peak_based')}
                                    className={`
                                        flex items-center gap-2 p-3 rounded-lg border cursor-pointer transition-all duration-200
                                        ${warmupMode === 'peak_based'
                                            ? 'bg-orange-50 dark:bg-orange-900/10 border-orange-200 dark:border-orange-800/50'
                                            : 'bg-gray-50/50 dark:bg-base-200/50 border-gray-100 dark:border-base-300/50 hover:border-gray-200 dark:hover:border-base-300'}
                                    `}
                                >
                                    <Clock size={16} className={warmupMode === 'peak_based' ? 'text-orange-500' : 'text-gray-400'} />
                                    <div>
                                        <div className={`text-xs font-medium ${warmupMode === 'peak_based' ? 'text-orange-700 dark:text-orange-400' : 'text-gray-600 dark:text-gray-400'}`}>
                                            {t('settings.warmup.mode_peak', '高峰期预热')}
                                        </div>
                                        <div className="text-[10px] text-gray-400 dark:text-gray-500 mt-0.5">
                                            {t('settings.warmup.mode_peak_desc', '高峰期前5小时')}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>

                        {/* 高峰期时间配置（仅在 peak_based 模式下显示） */}
                        {warmupMode === 'peak_based' && (
                            <div className="animate-in slide-in-from-top-2 duration-200">
                                <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-widest block mb-2">
                                    {t('settings.warmup.peak_hours_label', '高峰期时间')}
                                </label>
                                <div className="space-y-2">
                                    {peakHours.map((hour, index) => (
                                        <div key={index} className="flex items-center gap-2">
                                            <input
                                                type="time"
                                                value={hour}
                                                onChange={(e) => handlePeakHourChange(index, e.target.value)}
                                                className="flex-1 px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-base-300 bg-white dark:bg-base-200 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-orange-500/20 focus:border-orange-500"
                                            />
                                            <span className="text-[10px] text-gray-400 dark:text-gray-500 whitespace-nowrap">
                                                → {calculateWarmupTime(hour)} {t('settings.warmup.trigger_at', '预热')}
                                            </span>
                                            {peakHours.length > 1 && (
                                                <button
                                                    onClick={() => removePeakHour(index)}
                                                    className="p-1 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/20 text-gray-400 hover:text-red-500 transition-colors"
                                                >
                                                    <X size={14} />
                                                </button>
                                            )}
                                        </div>
                                    ))}
                                    {peakHours.length < 6 && (
                                        <button
                                            onClick={addPeakHour}
                                            className="flex items-center gap-1 px-3 py-1.5 text-xs text-orange-600 dark:text-orange-400 hover:bg-orange-50 dark:hover:bg-orange-900/20 rounded-lg transition-colors"
                                        >
                                            <Plus size={14} />
                                            {t('settings.warmup.add_peak_hour', '添加高峰期')}
                                        </button>
                                    )}
                                </div>
                                <p className="text-[10px] text-gray-400 dark:text-gray-500 mt-2 leading-relaxed">
                                    {t('settings.warmup.peak_hours_desc', '系统会在高峰期前 5 小时自动预热，确保高峰期有充足配额')}
                                </p>
                            </div>
                        )}

                        {/* 监控模型 */}
                        <div>
                            <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-widest block mb-2">
                                {t('settings.warmup.monitored_models_label', '监控模型（触发条件）')}
                            </label>
                            <div className="grid grid-cols-4 gap-2">
                                {warmupModelsOptions.map((model) => {
                                    const isSelected = config.monitored_models?.includes(model.id);
                                    return (
                                        <div
                                            key={model.id}
                                            onClick={() => toggleModel(model.id)}
                                            className={`
                                                flex items-center justify-between p-2 rounded-lg border cursor-pointer transition-all duration-200
                                                ${isSelected
                                                    ? 'bg-orange-50 dark:bg-orange-900/10 border-orange-200 dark:border-orange-800/50 text-orange-700 dark:text-orange-400'
                                                    : 'bg-gray-50/50 dark:bg-base-200/50 border-gray-100 dark:border-base-300/50 text-gray-500 hover:border-gray-200 dark:hover:border-base-300'}
                                            `}
                                        >
                                            <span className="text-[11px] font-medium truncate pr-2">
                                                {model.label}
                                            </span>
                                            <div className={`
                                                w-4 h-4 rounded-full flex items-center justify-center transition-all duration-300
                                                ${isSelected ? 'bg-orange-500 text-white scale-100' : 'bg-gray-200 dark:bg-base-300 text-transparent scale-75 opacity-0'}
                                            `}>
                                                <Check size={10} strokeWidth={4} />
                                            </div>
                                        </div>
                                    );
                                })}
                            </div>
                            <p className="text-[10px] text-gray-400 dark:text-gray-500 mt-2 leading-relaxed">
                                {t('settings.warmup.monitored_models_desc', '至少选择一个核心模型，任一选中模型配额恢复到 100% 时触发预热')}
                            </p>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
};

export default SmartWarmup;
