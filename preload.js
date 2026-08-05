const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
  minimize: () => ipcRenderer.send('window-minimize'),
  close: () => ipcRenderer.send('window-close'),
  
  toggleService: () => ipcRenderer.invoke('toggle-service'),
  onLogUpdate: (callback) => ipcRenderer.on('log-update', (event, log) => callback(log)),
  onStatusUpdate: (callback) => ipcRenderer.on('status-update', (event, status) => callback(status)),
  getInitialState: () => ipcRenderer.invoke('get-initial-state'),
  
  isElectron: true
});
