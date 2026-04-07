/**
 * Notifications Settings Component
 * Allows users to configure notification channels (Slack, Discord, Telegram, Email)
 */

import React, { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Bell,
  Plus,
  Trash2,
  TestTube,
  Check,
  X,
  Loader2,
  AlertTriangle,
  MessageSquare,
  Mail,
  Send,
  Hash,
  ExternalLink,
  Eye,
  EyeOff,
  Settings,
} from 'lucide-react';
import { useCloudStore } from '../../stores/useCloudStore';

// Provider icons (using simple representations)
const SlackIcon = () => (
  <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
    <path d="M5.042 15.165a2.528 2.528 0 0 1-2.52 2.523A2.528 2.528 0 0 1 0 15.165a2.527 2.527 0 0 1 2.522-2.52h2.52v2.52zm1.271 0a2.527 2.527 0 0 1 2.521-2.52 2.527 2.527 0 0 1 2.521 2.52v6.313A2.528 2.528 0 0 1 8.834 24a2.528 2.528 0 0 1-2.521-2.522v-6.313zM8.834 5.042a2.528 2.528 0 0 1-2.521-2.52A2.528 2.528 0 0 1 8.834 0a2.528 2.528 0 0 1 2.521 2.522v2.52H8.834zm0 1.271a2.528 2.528 0 0 1 2.521 2.521 2.528 2.528 0 0 1-2.521 2.521H2.522A2.528 2.528 0 0 1 0 8.834a2.528 2.528 0 0 1 2.522-2.521h6.312zm10.124 2.521a2.528 2.528 0 0 1 2.522-2.521A2.528 2.528 0 0 1 24 8.834a2.528 2.528 0 0 1-2.52 2.521h-2.522V8.834zm-1.271 0a2.528 2.528 0 0 1-2.521 2.521 2.528 2.528 0 0 1-2.521-2.521V2.522A2.528 2.528 0 0 1 15.166 0a2.528 2.528 0 0 1 2.521 2.522v6.312zm-2.521 10.124a2.528 2.528 0 0 1 2.521 2.522A2.528 2.528 0 0 1 15.166 24a2.528 2.528 0 0 1-2.521-2.52v-2.522h2.521zm0-1.271a2.528 2.528 0 0 1-2.521-2.521 2.528 2.528 0 0 1 2.521-2.521h6.312A2.528 2.528 0 0 1 24 15.165a2.528 2.528 0 0 1-2.52 2.521h-6.313z"/>
  </svg>
);

const DiscordIcon = () => (
  <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
    <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03zM8.02 15.33c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.956-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.956 2.418-2.157 2.418zm7.975 0c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.955-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.946 2.418-2.157 2.418z"/>
  </svg>
);

const TelegramIcon = () => (
  <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
    <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"/>
  </svg>
);

// Event types that can trigger notifications
const EVENT_TYPES = [
  { id: 'cron_failure', label: 'Cron Job Failures', description: 'When a scheduled job fails' },
  { id: 'cron_success', label: 'Cron Job Success', description: 'When a scheduled job completes' },
  { id: 'health_alert', label: 'Health Alerts', description: 'When a runtime becomes unhealthy' },
  { id: 'team_invitation', label: 'Team Invitations', description: 'When you receive a team invite' },
  { id: 'sync_conflict', label: 'Sync Conflicts', description: 'When skill sync has conflicts' },
  { id: 'sync_complete', label: 'Sync Complete', description: 'When skill sync finishes' },
  { id: 'skill_shared', label: 'Skill Shared', description: 'When someone shares a skill with your team' },
  { id: 'member_joined', label: 'Member Joined', description: 'When someone joins your team' },
];

// Provider definitions
const PROVIDERS = [
  {
    id: 'slack',
    name: 'Slack',
    icon: SlackIcon,
    color: '#4A154B',
    fields: [
      { name: 'webhookUrl', label: 'Webhook URL', type: 'url', placeholder: 'https://hooks.slack.com/services/...' },
      { name: 'channel', label: 'Channel (optional)', type: 'text', placeholder: '#general' },
    ],
    helpUrl: 'https://api.slack.com/messaging/webhooks',
    helpText: 'Create an Incoming Webhook in your Slack workspace',
  },
  {
    id: 'discord',
    name: 'Discord',
    icon: DiscordIcon,
    color: '#5865F2',
    fields: [
      { name: 'webhookUrl', label: 'Webhook URL', type: 'url', placeholder: 'https://discord.com/api/webhooks/...' },
    ],
    helpUrl: 'https://support.discord.com/hc/en-us/articles/228383668',
    helpText: 'Create a Webhook in your Discord server settings',
  },
  {
    id: 'telegram',
    name: 'Telegram',
    icon: TelegramIcon,
    color: '#0088cc',
    fields: [
      { name: 'botToken', label: 'Bot Token', type: 'password', placeholder: '123456789:ABCdefGHI...' },
      { name: 'chatId', label: 'Chat ID', type: 'text', placeholder: '-1001234567890' },
    ],
    helpUrl: 'https://core.telegram.org/bots#creating-a-new-bot',
    helpText: 'Create a bot with @BotFather and get your chat ID',
  },
  {
    id: 'email',
    name: 'Email',
    icon: () => <Mail size={20} />,
    color: '#EA4335',
    fields: [
      { name: 'host', label: 'SMTP Host', type: 'text', placeholder: 'smtp.gmail.com' },
      { name: 'port', label: 'Port', type: 'number', placeholder: '587' },
      { name: 'authUser', label: 'Username', type: 'text', placeholder: 'you@example.com' },
      { name: 'authPass', label: 'Password', type: 'password', placeholder: 'App password' },
      { name: 'from', label: 'From Address', type: 'email', placeholder: 'notifications@example.com' },
      { name: 'to', label: 'To Address', type: 'email', placeholder: 'you@example.com' },
    ],
    helpUrl: 'https://support.google.com/mail/answer/185833',
    helpText: 'Use an App Password for Gmail or SMTP credentials',
  },
];

interface NotificationConfig {
  id: string;
  provider: string;
  name: string;
  config: Record<string, string>;
  events: string[];
  enabled: boolean;
}

interface AddProviderModalProps {
  provider: typeof PROVIDERS[0];
  onSave: (config: Omit<NotificationConfig, 'id'>) => void;
  onCancel: () => void;
  existingConfig?: NotificationConfig;
}

function AddProviderModal({ provider, onSave, onCancel, existingConfig }: AddProviderModalProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(existingConfig?.name || `My ${provider.name}`);
  const [config, setConfig] = useState<Record<string, string>>(existingConfig?.config || {});
  const [events, setEvents] = useState<string[]>(existingConfig?.events || ['cron_failure', 'health_alert']);
  const [showSecrets, setShowSecrets] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; message: string } | null>(null);

  const Icon = provider.icon;

  const handleFieldChange = (fieldName: string, value: string) => {
    setConfig(prev => ({ ...prev, [fieldName]: value }));
  };

  const toggleEvent = (eventId: string) => {
    setEvents(prev =>
      prev.includes(eventId)
        ? prev.filter(e => e !== eventId)
        : [...prev, eventId]
    );
  };

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);

    // Simulate test - in real implementation, call API
    await new Promise(resolve => setTimeout(resolve, 1500));

    // For now, just validate fields are filled
    const requiredFields = provider.fields.filter(f => !f.label.includes('optional'));
    const missingFields = requiredFields.filter(f => !config[f.name]);

    if (missingFields.length > 0) {
      setTestResult({ success: false, message: `Missing: ${missingFields.map(f => f.label).join(', ')}` });
    } else {
      setTestResult({ success: true, message: 'Configuration looks good! (Test message would be sent)' });
    }

    setTesting(false);
  };

  const handleSave = () => {
    onSave({
      provider: provider.id,
      name,
      config,
      events,
      enabled: true,
    });
  };

  const isValid = name.trim() && events.length > 0 &&
    provider.fields.filter(f => !f.label.includes('optional')).every(f => config[f.name]);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-neutral-800 rounded-lg max-w-2xl w-full max-h-[90vh] overflow-y-auto">
        {/* Header */}
        <div className="flex items-center gap-3 p-4 border-b border-neutral-700">
          <div className="p-2 rounded-lg" style={{ backgroundColor: provider.color + '20' }}>
            <Icon />
          </div>
          <div>
            <h3 className="text-lg font-semibold">
              {existingConfig ? 'Edit' : 'Add'} {provider.name}
            </h3>
            <p className="text-sm text-neutral-400">{provider.helpText}</p>
          </div>
        </div>

        <div className="p-4 space-y-6">
          {/* Name */}
          <div>
            <label className="block text-sm font-medium mb-2">Name</label>
            <input
              type="text"
              value={name}
              onChange={e => setName(e.target.value)}
              className="w-full px-3 py-2 bg-neutral-700 border border-neutral-600 rounded-lg focus:border-blue-500 focus:outline-none"
              placeholder="My Slack Notifications"
            />
          </div>

          {/* Provider-specific fields */}
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <label className="block text-sm font-medium">Configuration</label>
              <button
                onClick={() => setShowSecrets(!showSecrets)}
                className="text-xs text-neutral-400 hover:text-white flex items-center gap-1"
              >
                {showSecrets ? <EyeOff size={14} /> : <Eye size={14} />}
                {showSecrets ? 'Hide' : 'Show'} secrets
              </button>
            </div>

            {provider.fields.map(field => (
              <div key={field.name}>
                <label className="block text-xs text-neutral-400 mb-1">{field.label}</label>
                <input
                  type={field.type === 'password' && !showSecrets ? 'password' : field.type === 'password' ? 'text' : field.type}
                  value={config[field.name] || ''}
                  onChange={e => handleFieldChange(field.name, e.target.value)}
                  className="w-full px-3 py-2 bg-neutral-700 border border-neutral-600 rounded-lg focus:border-blue-500 focus:outline-none text-sm"
                  placeholder={field.placeholder}
                />
              </div>
            ))}

            <a
              href={provider.helpUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 text-xs text-blue-400 hover:text-blue-300"
            >
              <ExternalLink size={12} />
              How to get these values
            </a>
          </div>

          {/* Events */}
          <div>
            <label className="block text-sm font-medium mb-2">Notify me about</label>
            <div className="grid grid-cols-2 gap-2">
              {EVENT_TYPES.map(event => (
                <label
                  key={event.id}
                  className={`flex items-start gap-2 p-2 rounded-lg cursor-pointer transition-colors ${
                    events.includes(event.id)
                      ? 'bg-blue-500/20 border border-blue-500/50'
                      : 'bg-neutral-700/50 border border-transparent hover:border-neutral-600'
                  }`}
                >
                  <input
                    type="checkbox"
                    checked={events.includes(event.id)}
                    onChange={() => toggleEvent(event.id)}
                    className="mt-1"
                  />
                  <div>
                    <div className="text-sm font-medium">{event.label}</div>
                    <div className="text-xs text-neutral-400">{event.description}</div>
                  </div>
                </label>
              ))}
            </div>
          </div>

          {/* Test Result */}
          {testResult && (
            <div className={`p-3 rounded-lg flex items-center gap-2 ${
              testResult.success ? 'bg-green-500/20 text-green-400' : 'bg-red-500/20 text-red-400'
            }`}>
              {testResult.success ? <Check size={16} /> : <AlertTriangle size={16} />}
              {testResult.message}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between p-4 border-t border-neutral-700">
          <button
            onClick={handleTest}
            disabled={testing}
            className="flex items-center gap-2 px-4 py-2 bg-neutral-700 hover:bg-neutral-600 disabled:opacity-50 rounded-lg transition-colors"
          >
            {testing ? <Loader2 size={16} className="animate-spin" /> : <TestTube size={16} />}
            Test Configuration
          </button>

          <div className="flex items-center gap-2">
            <button
              onClick={onCancel}
              className="px-4 py-2 bg-neutral-700 hover:bg-neutral-600 rounded-lg transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              disabled={!isValid}
              className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg transition-colors"
            >
              <Check size={16} />
              {existingConfig ? 'Update' : 'Add'} {provider.name}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export default function NotificationsSettings() {
  const { t } = useTranslation();
  const { isAuthenticated } = useCloudStore();

  const [configs, setConfigs] = useState<NotificationConfig[]>([]);
  const [addingProvider, setAddingProvider] = useState<typeof PROVIDERS[0] | null>(null);
  const [editingConfig, setEditingConfig] = useState<NotificationConfig | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  // Load configs on mount
  useEffect(() => {
    // TODO: Load from API
    // For now, load from localStorage
    const stored = localStorage.getItem('ato_notification_configs');
    if (stored) {
      try {
        setConfigs(JSON.parse(stored));
      } catch {
        // ignore
      }
    }
  }, []);

  // Save configs
  const saveConfigs = (newConfigs: NotificationConfig[]) => {
    setConfigs(newConfigs);
    localStorage.setItem('ato_notification_configs', JSON.stringify(newConfigs));
    // TODO: Sync to cloud API when authenticated
  };

  const handleAddConfig = (config: Omit<NotificationConfig, 'id'>) => {
    const newConfig: NotificationConfig = {
      ...config,
      id: `${config.provider}_${Date.now()}`,
    };
    saveConfigs([...configs, newConfig]);
    setAddingProvider(null);
  };

  const handleUpdateConfig = (config: Omit<NotificationConfig, 'id'>) => {
    if (!editingConfig) return;
    const updated = configs.map(c =>
      c.id === editingConfig.id ? { ...config, id: editingConfig.id } : c
    );
    saveConfigs(updated);
    setEditingConfig(null);
  };

  const handleDeleteConfig = (id: string) => {
    if (!confirm('Delete this notification channel?')) return;
    saveConfigs(configs.filter(c => c.id !== id));
  };

  const handleToggleConfig = (id: string) => {
    const updated = configs.map(c =>
      c.id === id ? { ...c, enabled: !c.enabled } : c
    );
    saveConfigs(updated);
  };

  const getProviderById = (id: string) => PROVIDERS.find(p => p.id === id);

  return (
    <div className="p-6">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold flex items-center gap-2">
            <Bell size={28} />
            {t('notifications.title', 'Notifications')}
          </h1>
          <p className="text-neutral-400 mt-1">
            {t('notifications.subtitle', 'Get notified about important events via your preferred channels')}
          </p>
        </div>
      </div>

      {/* Provider Cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-8">
        {PROVIDERS.map(provider => {
          const Icon = provider.icon;
          const configCount = configs.filter(c => c.provider === provider.id).length;

          return (
            <button
              key={provider.id}
              onClick={() => setAddingProvider(provider)}
              className="p-4 bg-neutral-800 rounded-lg border border-neutral-700 hover:border-neutral-600 transition-colors text-left"
            >
              <div className="flex items-center gap-3 mb-2">
                <div
                  className="p-2 rounded-lg"
                  style={{ backgroundColor: provider.color + '20', color: provider.color }}
                >
                  <Icon />
                </div>
                <div>
                  <div className="font-medium">{provider.name}</div>
                  {configCount > 0 && (
                    <div className="text-xs text-neutral-400">{configCount} configured</div>
                  )}
                </div>
              </div>
              <div className="flex items-center gap-1 text-xs text-blue-400">
                <Plus size={12} />
                Add {provider.name}
              </div>
            </button>
          );
        })}
      </div>

      {/* Configured Channels */}
      <div>
        <h2 className="text-lg font-semibold mb-4">
          {t('notifications.configuredChannels', 'Configured Channels')}
        </h2>

        {configs.length === 0 ? (
          <div className="bg-neutral-800 rounded-lg p-8 text-center">
            <Bell size={48} className="mx-auto mb-4 text-neutral-500" />
            <p className="text-neutral-400 mb-2">
              {t('notifications.noChannels', 'No notification channels configured')}
            </p>
            <p className="text-sm text-neutral-500">
              {t('notifications.noChannelsHint', 'Add a channel above to start receiving notifications')}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {configs.map(config => {
              const provider = getProviderById(config.provider);
              if (!provider) return null;
              const Icon = provider.icon;

              return (
                <div
                  key={config.id}
                  className={`bg-neutral-800 rounded-lg p-4 border transition-colors ${
                    config.enabled ? 'border-neutral-700' : 'border-neutral-800 opacity-60'
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-3">
                      <div
                        className="p-2 rounded-lg"
                        style={{ backgroundColor: provider.color + '20', color: provider.color }}
                      >
                        <Icon />
                      </div>
                      <div>
                        <div className="font-medium">{config.name}</div>
                        <div className="text-xs text-neutral-400">
                          {config.events.length} event{config.events.length !== 1 ? 's' : ''} •{' '}
                          {config.enabled ? 'Enabled' : 'Disabled'}
                        </div>
                      </div>
                    </div>

                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => handleToggleConfig(config.id)}
                        className={`p-2 rounded-lg transition-colors ${
                          config.enabled
                            ? 'bg-green-500/20 text-green-400 hover:bg-green-500/30'
                            : 'bg-neutral-700 text-neutral-400 hover:bg-neutral-600'
                        }`}
                        title={config.enabled ? 'Disable' : 'Enable'}
                      >
                        {config.enabled ? <Bell size={16} /> : <Bell size={16} />}
                      </button>
                      <button
                        onClick={() => {
                          setEditingConfig(config);
                          setAddingProvider(provider);
                        }}
                        className="p-2 bg-neutral-700 hover:bg-neutral-600 rounded-lg transition-colors"
                        title="Edit"
                      >
                        <Settings size={16} />
                      </button>
                      <button
                        onClick={() => handleDeleteConfig(config.id)}
                        className="p-2 bg-neutral-700 hover:bg-red-500/20 hover:text-red-400 rounded-lg transition-colors"
                        title="Delete"
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  </div>

                  {/* Events tags */}
                  <div className="flex flex-wrap gap-1 mt-3">
                    {config.events.map(eventId => {
                      const event = EVENT_TYPES.find(e => e.id === eventId);
                      return event ? (
                        <span
                          key={eventId}
                          className="px-2 py-0.5 bg-neutral-700 rounded text-xs text-neutral-300"
                        >
                          {event.label}
                        </span>
                      ) : null;
                    })}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Add/Edit Modal */}
      {addingProvider && (
        <AddProviderModal
          provider={addingProvider}
          existingConfig={editingConfig || undefined}
          onSave={editingConfig ? handleUpdateConfig : handleAddConfig}
          onCancel={() => {
            setAddingProvider(null);
            setEditingConfig(null);
          }}
        />
      )}
    </div>
  );
}
