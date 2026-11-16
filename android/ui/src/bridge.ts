// Tauri IPC Bridge Module (Desktop + Android VpnService)
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

export const IS_ANDROID = /Android/i.test(navigator.userAgent);

export interface InitialState {
  isServiceRunning: boolean;
  port: number;
  logs: string[];
  version: string;
  desyncMode?: string;
  delayMs?: number;
}

export interface StatusPayload {
  isRunning: boolean;
  port: number;
  error?: string | null;
}

export interface UpdateCheckResult {
  status: 'up_to_date' | 'update_available' | 'error';
  latestVersion?: string | null;
}

export const appBridge = {
  startDragging: async () => {
    if (IS_ANDROID) return;
    try {
      const win = getCurrentWindow();
      await win.startDragging();
    } catch (e) {
      console.warn('startDragging failed:', e);
    }
  },

  toggleService: async (_desyncMode = 'TlsHeader5', _delayMs = 2): Promise<StatusPayload> => {
    try {
      return await invoke<StatusPayload>('toggle_service');
    } catch (e) {
      console.error('toggle_service error:', e);
      return {
        isRunning: false,
        port: 0,
        error: String(e),
      };
    }
  },

  getInitialState: async (): Promise<InitialState> => {
    try {
      return await invoke<InitialState>('get_initial_state');
    } catch (e) {
      console.warn('get_initial_state fallback:', e);
      return {
        isServiceRunning: false,
        port: 8080,
        logs: [],
        version: '3.3.1',
      };
    }
  },

  getLogs: async (): Promise<string[]> => {
    try {
      return await invoke<string[]>('get_logs');
    } catch (e) {
      console.warn('get_logs failed:', e);
      return [];
    }
  },

  openLogFile: async (): Promise<void> => {
    try {
      await invoke('open_log_file');
    } catch (e) {
      console.warn('open_log_file failed:', e);
    }
  },

  openGithub: async (): Promise<void> => {
    if (IS_ANDROID) {
      window.open('https://github.com/knoblab/freeview-sni', '_blank');
      return;
    }
    try {
      await invoke('open_github');
    } catch (e) {
      console.warn('open_github failed:', e);
    }
  },

  checkUpdate: async (): Promise<UpdateCheckResult> => {
    try {
      return await invoke<UpdateCheckResult>('check_update');
    } catch (e) {
      console.warn('check_update failed:', e);
      return { status: 'error', latestVersion: null };
    }
  },

  onStatusUpdate: (callback: (status: StatusPayload) => void): Promise<UnlistenFn> => {
    return listen<StatusPayload>('status-update', (event) => {
      callback(event.payload);
    });
  },
};
