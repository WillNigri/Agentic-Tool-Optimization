/**
 * Skill Sync Component
 * Manages skill synchronization across devices
 */

import React, { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Cloud,
  CloudOff,
  RefreshCw,
  Monitor,
  Laptop,
  Server,
  HardDrive,
  Trash2,
  AlertTriangle,
  Check,
  Upload,
  Download,
  GitMerge,
  Clock,
  Loader2,
} from 'lucide-react';
import { useCloudStore } from '../../stores/useCloudStore';
import {
  getSyncStatus,
  getSyncDevices,
  registerSyncDevice,
  removeSyncDevice,
  syncSkills,
  resolveSyncConflict,
  SyncDevice,
  SyncStatus,
  SyncConflict,
  CloudSkill,
  LocalSkillForSync,
} from '../../lib/cloud-api';

// Device type icons
const deviceIcons: Record<string, React.ReactNode> = {
  desktop: <Monitor size={20} />,
  laptop: <Laptop size={20} />,
  server: <Server size={20} />,
  other: <HardDrive size={20} />,
};

// Generate a unique device ID
function generateDeviceId(): string {
  const stored = localStorage.getItem('ato_device_id');
  if (stored) return stored;

  const id = `device_${Date.now()}_${Math.random().toString(36).substring(2, 9)}`;
  localStorage.setItem('ato_device_id', id);
  return id;
}

// Get device info
function getDeviceInfo(): { name: string; type: 'desktop' | 'laptop' | 'server' | 'other'; os: string } {
  const userAgent = navigator.userAgent.toLowerCase();
  let os = 'Unknown';
  let type: 'desktop' | 'laptop' | 'server' | 'other' = 'desktop';

  if (userAgent.includes('mac')) {
    os = 'macOS';
    type = userAgent.includes('macbook') ? 'laptop' : 'desktop';
  } else if (userAgent.includes('win')) {
    os = 'Windows';
  } else if (userAgent.includes('linux')) {
    os = 'Linux';
    type = 'server'; // Assume server for Linux
  }

  return {
    name: `${os} Device`,
    type,
    os,
  };
}

interface ConflictResolutionModalProps {
  conflict: SyncConflict;
  onResolve: (resolution: 'keep_local' | 'keep_cloud' | 'merge', content?: string) => void;
  onCancel: () => void;
}

function ConflictResolutionModal({ conflict, onResolve, onCancel }: ConflictResolutionModalProps) {
  const { t } = useTranslation();

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-neutral-800 rounded-lg p-6 max-w-lg w-full mx-4">
        <div className="flex items-center gap-3 mb-4">
          <AlertTriangle className="text-yellow-500" size={24} />
          <h3 className="text-lg font-semibold">{t('sync.conflictDetected', 'Sync Conflict Detected')}</h3>
        </div>

        <p className="text-neutral-300 mb-4">
          {t('sync.conflictDescription', 'The skill "{{name}}" has been modified both locally and in the cloud.', { name: conflict.skill_name })}
        </p>

        <div className="grid grid-cols-2 gap-4 mb-6">
          <div className="bg-neutral-700 rounded p-3">
            <div className="text-sm text-neutral-400 mb-1">{t('sync.localVersion', 'Local Version')}</div>
            <div className="text-xs text-neutral-500">
              {new Date(conflict.local_updated_at).toLocaleString()}
            </div>
          </div>
          <div className="bg-neutral-700 rounded p-3">
            <div className="text-sm text-neutral-400 mb-1">{t('sync.cloudVersion', 'Cloud Version')}</div>
            <div className="text-xs text-neutral-500">
              {new Date(conflict.cloud_updated_at).toLocaleString()}
            </div>
          </div>
        </div>

        <div className="flex flex-col gap-2">
          <button
            onClick={() => onResolve('keep_local')}
            className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-500 rounded transition-colors"
          >
            <Upload size={16} />
            {t('sync.keepLocal', 'Keep Local (Upload)')}
          </button>
          <button
            onClick={() => onResolve('keep_cloud')}
            className="flex items-center gap-2 px-4 py-2 bg-green-600 hover:bg-green-500 rounded transition-colors"
          >
            <Download size={16} />
            {t('sync.keepCloud', 'Keep Cloud (Download)')}
          </button>
          <button
            onClick={onCancel}
            className="px-4 py-2 bg-neutral-600 hover:bg-neutral-500 rounded transition-colors"
          >
            {t('common.cancel', 'Cancel')}
          </button>
        </div>
      </div>
    </div>
  );
}

export default function SkillSync() {
  const { t } = useTranslation();
  const { isAuthenticated, user } = useCloudStore();

  const [devices, setDevices] = useState<SyncDevice[]>([]);
  const [currentDevice, setCurrentDevice] = useState<SyncDevice | null>(null);
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSyncing, setIsSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastSyncResult, setLastSyncResult] = useState<{
    uploaded: number;
    downloaded: number;
  } | null>(null);
  const [activeConflict, setActiveConflict] = useState<SyncConflict | null>(null);

  // Load devices and sync status
  const loadData = useCallback(async () => {
    if (!isAuthenticated) return;

    setIsLoading(true);
    setError(null);

    try {
      const deviceList = await getSyncDevices();
      setDevices(deviceList);

      // Find current device
      const deviceId = generateDeviceId();
      const current = deviceList.find(d => d.device_id === deviceId);

      if (current) {
        setCurrentDevice(current);
        const status = await getSyncStatus(deviceId);
        setSyncStatus(status);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load sync data');
    } finally {
      setIsLoading(false);
    }
  }, [isAuthenticated]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  // Register current device
  const handleRegisterDevice = async () => {
    setError(null);

    try {
      const deviceInfo = getDeviceInfo();
      const device = await registerSyncDevice({
        device_name: deviceInfo.name,
        device_type: deviceInfo.type,
        device_id: generateDeviceId(),
        os_name: deviceInfo.os,
        app_version: '0.5.0',
      });

      setCurrentDevice(device);
      await loadData();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to register device');
    }
  };

  // Remove a device
  const handleRemoveDevice = async (deviceId: string) => {
    if (!confirm(t('sync.confirmRemoveDevice', 'Are you sure you want to remove this device?'))) {
      return;
    }

    try {
      await removeSyncDevice(deviceId);
      await loadData();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to remove device');
    }
  };

  // Perform sync
  const handleSync = async () => {
    if (!currentDevice) return;

    setIsSyncing(true);
    setError(null);
    setLastSyncResult(null);

    try {
      // TODO: Get local skills from the skills store
      // For now, we'll send an empty array to just fetch cloud skills
      const localSkills: LocalSkillForSync[] = [];

      const result = await syncSkills(currentDevice.device_id, localSkills);

      setLastSyncResult({
        uploaded: result.uploaded.length,
        downloaded: result.downloaded.length,
      });

      // Handle conflicts
      if (result.conflicts.length > 0) {
        setActiveConflict(result.conflicts[0]);
      }

      await loadData();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Sync failed');
    } finally {
      setIsSyncing(false);
    }
  };

  // Resolve conflict
  const handleResolveConflict = async (resolution: 'keep_local' | 'keep_cloud' | 'merge', content?: string) => {
    if (!activeConflict || !currentDevice) return;

    try {
      await resolveSyncConflict(
        activeConflict.skill_id,
        currentDevice.device_id,
        resolution,
        content
      );

      setActiveConflict(null);

      // Check for more conflicts
      if (syncStatus && syncStatus.conflicts.length > 1) {
        const remaining = syncStatus.conflicts.filter(c => c.skill_id !== activeConflict.skill_id);
        if (remaining.length > 0) {
          setActiveConflict(remaining[0]);
        }
      }

      await loadData();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to resolve conflict');
    }
  };

  // Not authenticated
  if (!isAuthenticated) {
    return (
      <div className="p-6">
        <div className="flex flex-col items-center justify-center py-12 text-center">
          <CloudOff size={48} className="text-neutral-500 mb-4" />
          <h2 className="text-xl font-semibold mb-2">{t('sync.notAuthenticated', 'Not Connected')}</h2>
          <p className="text-neutral-400 max-w-md">
            {t('sync.signInToSync', 'Sign in to your ATO Cloud account to sync skills across your devices.')}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="p-6">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold flex items-center gap-2">
            <Cloud size={28} />
            {t('sync.title', 'Skill Sync')}
          </h1>
          <p className="text-neutral-400 mt-1">
            {t('sync.subtitle', 'Keep your skills synchronized across all your devices')}
          </p>
        </div>

        {currentDevice && (
          <button
            onClick={handleSync}
            disabled={isSyncing}
            className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-500 disabled:bg-neutral-600 rounded-lg transition-colors"
          >
            {isSyncing ? (
              <Loader2 size={18} className="animate-spin" />
            ) : (
              <RefreshCw size={18} />
            )}
            {isSyncing ? t('sync.syncing', 'Syncing...') : t('sync.syncNow', 'Sync Now')}
          </button>
        )}
      </div>

      {/* Error */}
      {error && (
        <div className="mb-6 p-4 bg-red-500/20 border border-red-500 rounded-lg flex items-center gap-2 text-red-400">
          <AlertTriangle size={18} />
          {error}
        </div>
      )}

      {/* Last sync result */}
      {lastSyncResult && (
        <div className="mb-6 p-4 bg-green-500/20 border border-green-500 rounded-lg flex items-center gap-2 text-green-400">
          <Check size={18} />
          {t('sync.syncComplete', 'Sync complete: {{uploaded}} uploaded, {{downloaded}} downloaded', lastSyncResult)}
        </div>
      )}

      {isLoading ? (
        <div className="flex items-center justify-center py-12">
          <Loader2 size={32} className="animate-spin text-neutral-500" />
        </div>
      ) : (
        <>
          {/* Current Device */}
          <div className="mb-8">
            <h2 className="text-lg font-semibold mb-4">{t('sync.thisDevice', 'This Device')}</h2>

            {currentDevice ? (
              <div className="bg-neutral-800 rounded-lg p-4">
                <div className="flex items-center gap-4">
                  <div className="p-3 bg-blue-500/20 rounded-lg text-blue-400">
                    {deviceIcons[currentDevice.device_type]}
                  </div>
                  <div className="flex-1">
                    <div className="font-medium">{currentDevice.device_name}</div>
                    <div className="text-sm text-neutral-400">
                      {currentDevice.os_name} • {t('sync.registered', 'Registered')} {new Date(currentDevice.created_at).toLocaleDateString()}
                    </div>
                  </div>
                  <div className="text-right">
                    {currentDevice.sync_enabled ? (
                      <span className="flex items-center gap-1 text-green-400 text-sm">
                        <Check size={14} />
                        {t('sync.enabled', 'Sync Enabled')}
                      </span>
                    ) : (
                      <span className="flex items-center gap-1 text-neutral-400 text-sm">
                        <CloudOff size={14} />
                        {t('sync.disabled', 'Sync Disabled')}
                      </span>
                    )}
                  </div>
                </div>

                {/* Sync Status */}
                {syncStatus && (
                  <div className="mt-4 pt-4 border-t border-neutral-700 grid grid-cols-3 gap-4">
                    <div className="text-center">
                      <div className="text-2xl font-bold text-blue-400">{syncStatus.pendingUploads}</div>
                      <div className="text-xs text-neutral-400 flex items-center justify-center gap-1">
                        <Upload size={12} />
                        {t('sync.pendingUploads', 'Pending Uploads')}
                      </div>
                    </div>
                    <div className="text-center">
                      <div className="text-2xl font-bold text-green-400">{syncStatus.pendingDownloads}</div>
                      <div className="text-xs text-neutral-400 flex items-center justify-center gap-1">
                        <Download size={12} />
                        {t('sync.pendingDownloads', 'Pending Downloads')}
                      </div>
                    </div>
                    <div className="text-center">
                      <div className="text-2xl font-bold text-yellow-400">{syncStatus.conflicts.length}</div>
                      <div className="text-xs text-neutral-400 flex items-center justify-center gap-1">
                        <GitMerge size={12} />
                        {t('sync.conflicts', 'Conflicts')}
                      </div>
                    </div>
                  </div>
                )}

                {syncStatus?.lastSyncAt && (
                  <div className="mt-4 text-sm text-neutral-400 flex items-center gap-1">
                    <Clock size={14} />
                    {t('sync.lastSync', 'Last synced')}: {new Date(syncStatus.lastSyncAt).toLocaleString()}
                  </div>
                )}
              </div>
            ) : (
              <div className="bg-neutral-800 rounded-lg p-6 text-center">
                <CloudOff size={32} className="mx-auto mb-3 text-neutral-500" />
                <p className="text-neutral-400 mb-4">
                  {t('sync.deviceNotRegistered', 'This device is not registered for sync.')}
                </p>
                <button
                  onClick={handleRegisterDevice}
                  className="px-4 py-2 bg-blue-600 hover:bg-blue-500 rounded-lg transition-colors"
                >
                  {t('sync.registerDevice', 'Register This Device')}
                </button>
              </div>
            )}
          </div>

          {/* Other Devices */}
          <div>
            <h2 className="text-lg font-semibold mb-4">{t('sync.otherDevices', 'Other Devices')}</h2>

            {devices.filter(d => d.device_id !== currentDevice?.device_id).length === 0 ? (
              <div className="bg-neutral-800 rounded-lg p-6 text-center text-neutral-400">
                {t('sync.noOtherDevices', 'No other devices registered. Sign in on another device to sync skills.')}
              </div>
            ) : (
              <div className="space-y-2">
                {devices
                  .filter(d => d.device_id !== currentDevice?.device_id)
                  .map(device => (
                    <div
                      key={device.id}
                      className="bg-neutral-800 rounded-lg p-4 flex items-center gap-4"
                    >
                      <div className="p-2 bg-neutral-700 rounded-lg text-neutral-400">
                        {deviceIcons[device.device_type]}
                      </div>
                      <div className="flex-1">
                        <div className="font-medium">{device.device_name}</div>
                        <div className="text-sm text-neutral-400">
                          {device.os_name} • {t('sync.lastSeen', 'Last sync')}: {device.last_sync_at ? new Date(device.last_sync_at).toLocaleDateString() : t('sync.never', 'Never')}
                        </div>
                      </div>
                      <button
                        onClick={() => handleRemoveDevice(device.id)}
                        className="p-2 text-neutral-400 hover:text-red-400 transition-colors"
                        title={t('sync.removeDevice', 'Remove device')}
                      >
                        <Trash2 size={18} />
                      </button>
                    </div>
                  ))}
              </div>
            )}
          </div>

          {/* Cloud Skills Count */}
          {syncStatus && (
            <div className="mt-8 p-4 bg-neutral-800 rounded-lg">
              <div className="flex items-center justify-between">
                <div>
                  <div className="font-medium">{t('sync.cloudSkills', 'Skills in Cloud')}</div>
                  <div className="text-sm text-neutral-400">
                    {t('sync.cloudSkillsDescription', 'Total skills stored in your cloud account')}
                  </div>
                </div>
                <div className="text-3xl font-bold text-blue-400">
                  {syncStatus.cloudSkillCount}
                </div>
              </div>
            </div>
          )}
        </>
      )}

      {/* Conflict Resolution Modal */}
      {activeConflict && (
        <ConflictResolutionModal
          conflict={activeConflict}
          onResolve={handleResolveConflict}
          onCancel={() => setActiveConflict(null)}
        />
      )}
    </div>
  );
}
