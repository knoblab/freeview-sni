// Tauri IPC Bridge Module
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

export interface InitialState {
  isServiceRunning: boolean;
  logs: string[];
  version: string;
}

export const appBridge = {
  minimize: async () => {
    try {
      const win = getCurrentWindow();
      await win.hide();
    } catch (e) {
      console.warn('minimize failed:', e);
    }
  },

  close: async () => {
    try {
      const win = getCurrentWindow();
      await win.hide();
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

  toggleService: async (): Promise<boolean> => {
    try {
      return await invoke<boolean>('toggle_service');
    } catch (e) {
      console.error('toggle_service error:', e);
      return false;
    }
  },

  getInitialState: async (): Promise<InitialState> => {
    try {
      return await invoke<InitialState>('get_initial_state');
    } catch (e) {
      console.warn('get_initial_state fallback:', e);
      return {
        isServiceRunning: false,
        logs: [],
        version: "3.1.0"
      };
    }
  },

  onLogUpdate: (callback: (log: string) => void): Promise<UnlistenFn> => {
    return listen<string>('log-update', (event) => {
      callback(event.payload);
    });
  },

  onStatusUpdate: (callback: (status: boolean) => void): Promise<UnlistenFn> => {
    return listen<boolean>('status-update', (event) => {
      callback(event.payload);
    });
  }
};
