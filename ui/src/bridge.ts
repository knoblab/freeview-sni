// Tauri IPC Bridge Module
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

export interface InitialState {
  isServiceRunning: boolean;
  port: number;
  logs: string[];
  version: string;
}

export interface StatusPayload {
  isRunning: boolean;
  port: number;
  error?: string | null;
}

export const appBridge = {
  minimize: async () => {
    try {
      const win = getCurrentWindow();
      await win.minimize();
    } catch (e) {
      console.warn('minimize failed:', e);
    }
  },

  close: async () => {
    try {
      const win = getCurrentWindow();
      await win.close();
    } catch (e) {
      console.warn('close failed:', e);
    }
  },

  startDragging: async () => {
    try {
      const win = getCurrentWindow();
      await win.startDragging();
    } catch (e) {
      console.warn('startDragging failed:', e);
    }
  },

  toggleService: async (): Promise<StatusPayload> => {
    try {
      return await invoke<StatusPayload>('toggle_service');
    } catch (e) {
      console.error('toggle_service error:', e);
      return {
        isRunning: false,
        port: 8080,
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
        version: '3.2.0',
      };
    }
  },

  onLogUpdate: (callback: (log: string) => void): Promise<UnlistenFn> => {
    return listen<string>('log-update', (event) => {
      callback(event.payload);
    });
  },

  onStatusUpdate: (callback: (status: StatusPayload) => void): Promise<UnlistenFn> => {
    return listen<StatusPayload>('status-update', (event) => {
      callback(event.payload);
    });
  },
};
