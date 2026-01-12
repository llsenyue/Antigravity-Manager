import React, { useEffect, useState } from 'react';
import { X, Download } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

interface UpdateInfo {
  has_update: boolean;
  latest_version: string;
  current_version: string;
  download_url: string;
}

interface UpdateNotificationProps {
  onClose: () => void;
}

export const UpdateNotification: React.FC<UpdateNotificationProps> = ({ onClose }) => {
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isVisible, setIsVisible] = useState(false);

  useEffect(() => {
    checkForUpdates();
  }, []);

  const checkForUpdates = async () => {
    try {
      const info = await invoke<UpdateInfo>('check_for_updates');
      if (info.has_update) {
        setUpdateInfo(info);
        setIsVisible(true);
      } else {
        onClose();
      }
    } catch (error) {
      console.error('Failed to check for updates:', error);
      onClose();
    }
  };

  const handleDownload = () => {
    if (updateInfo?.download_url) {
      window.open(updateInfo.download_url, '_blank');
    }
  };

  const handleClose = () => {
    setIsVisible(false);
    setTimeout(onClose, 300);
  };

  if (!updateInfo || !isVisible) {
    return null;
  }

  return (
    <div className="fixed top-4 right-4 z-50 animate-in slide-in-from-top-2 duration-300">
      <div className="bg-gradient-to-r from-blue-500 to-purple-600 text-white rounded-lg shadow-2xl p-4 max-w-md">
        <div className="flex items-start justify-between mb-2">
          <div className="flex items-center gap-2">
            <Download className="w-5 h-5" />
            <h3 className="font-semibold text-lg">新版本可用</h3>
          </div>
          <button
            onClick={handleClose}
            className="text-white/80 hover:text-white transition-colors"
            aria-label="关闭"
          >
            <X className="w-5 h-5" />
          </button>
        </div>
        
        <div className="space-y-2">
          <p className="text-sm text-white/90">
            发现新版本 <span className="font-bold">{updateInfo.latest_version}</span>
          </p>
          <p className="text-xs text-white/70">
            当前版本: {updateInfo.current_version}
          </p>
          
          <button
            onClick={handleDownload}
            className="w-full mt-3 bg-white text-blue-600 hover:bg-blue-50 font-medium py-2 px-4 rounded-md transition-colors duration-200 flex items-center justify-center gap-2"
          >
            <Download className="w-4 h-4" />
            立即下载
          </button>
        </div>
      </div>
    </div>
  );
};
