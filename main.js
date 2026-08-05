const { app, BrowserWindow, ipcMain, Tray, Menu, powerMonitor } = require('electron');
const path = require('path');
const net = require('net');
const https = require('https');
const os = require('os');
const { exec, execSync, execFile } = require('child_process');
const fs = require('fs');

// GPU 하드웨어 가속 비활성화: Chromium GPU 캐시 권한 충돌(Access Denied)로 인한 네이티브 크래시 원천 차단
app.disableHardwareAcceleration();

// 관리자(Administrator) 권한 실행 시 Chromium 샌드박스 초기화 실패로 렌더러 프로세스가 크래시하는 문제 해결
// requireAdministrator 매니페스트와 Chromium 샌드박스는 호환되지 않음
app.commandLine.appendSwitch('no-sandbox');

let mainWindow = null;
let isServiceRunning = false;
let proxyServer = null;
let tray = null;
let cleanupDone = false;
const logs = [];

// DoH DNS 캐시: { hostname → { ip, expiry(ms) } }
const dnsCache = new Map();

const PROXY_LISTEN_IP = "127.0.0.1";
const PROXY_LISTEN_PORT = 8080;
const BUFFER_SIZE = 32768;
const DOH_ENDPOINT = "https://1.1.1.1/dns-query";
const DOH_TIMEOUT_MS = 3000;

// DoH를 우회할 로컬/내부 도메인 접미사 목록
const DOH_LOCAL_SUFFIXES = [
  '.local', '.localhost', '.internal', '.intranet',
  '.lan', '.home', '.corp', '.private', '.test',
  '.example', '.invalid'
];

const refreshExePath = app.isPackaged 
  ? path.join(process.resourcesPath, 'app.asar.unpacked', 'refresh-proxy.exe') 
  : path.join(__dirname, 'refresh-proxy.exe');

const iconPath = app.isPackaged 
  ? path.join(process.resourcesPath, 'app.asar.unpacked', 'logo.ico') 
  : path.join(__dirname, 'logo.ico');

let validIconPath = null;
try {
  if (fs.existsSync(iconPath)) {
    validIconPath = iconPath;
  }
} catch (e) {
  // ignore
}

// ─────────────────────────────────────────────────────────
// 서비스 상태 파일: 비정상 종료 감지용
// ─────────────────────────────────────────────────────────

function getStateFilePath() {
  try {
    return path.join(app.getPath('userData'), 'service_state.json');
  } catch (e) {
    return path.join(os.tmpdir(), 'freeview-service-state.json');
  }
}

function saveServiceState(running) {
  try {
    const data = JSON.stringify({ running, pid: process.pid, timestamp: Date.now() });
    fs.writeFileSync(getStateFilePath(), data);
  } catch (e) { /* ignore */ }
}

function loadServiceState() {
  try {
    const data = fs.readFileSync(getStateFilePath(), 'utf-8');
    return JSON.parse(data);
  } catch (e) {
    return { running: false };
  }
}

// ─────────────────────────────────────────────────────────
// 로깅
// ─────────────────────────────────────────────────────────

let logFilePath = null;
function getLogFilePath() {
  if (!logFilePath) {
    try {
      logFilePath = path.join(app.getPath('userData'), 'app.log');
    } catch (e) {
      logFilePath = path.join(os.tmpdir(), 'freeview-app.log');
    }
  }
  return logFilePath;
}

function writeLogToFile(msg) {
  try {
    const time = new Date().toLocaleTimeString('ko-KR', { hour12: false });
    fs.appendFileSync(getLogFilePath(), `[${time}] ${msg}\n`);
  } catch (e) {
    // ignore
  }
}

function addLog(msg) {
  const time = new Date().toLocaleTimeString('ko-KR', { hour12: false });
  const formatted = `[${time}] ${msg}`;
  logs.push(formatted);
  writeLogToFile(msg);
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('log-update', formatted);
  }
}

function updateStatus(status) {
  isServiceRunning = status;
  saveServiceState(status);
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('status-update', status);
  }
  if (tray) {
    tray.setToolTip(status ? 'FreeView — 보호 활성화' : 'FreeView — 대기 중');
  }
  updateTrayMenu();
}

// ─────────────────────────────────────────────────────────
// PAC (Proxy Auto-Configuration) 파일 관리
// PAC의 "PROXY …; DIRECT" 폴백 덕분에 프록시가 죽어도 인터넷이 유지됨
// ─────────────────────────────────────────────────────────

function getPacFilePath() {
  try {
    return path.join(app.getPath('userData'), 'freeview_proxy.pac');
  } catch (e) {
    return path.join(os.tmpdir(), 'freeview_proxy.pac');
  }
}

function getPacFileUrl() {
  const filePath = getPacFilePath();
  // Windows 경로를 file:// URL로 변환: C:\Users\... → file:///C:/Users/...
  return 'file:///' + filePath.replace(/\\/g, '/');
}

function createPacFile() {
  const pacContent = [
    'function FindProxyForURL(url, host) {',
    `  return "PROXY ${PROXY_LISTEN_IP}:${PROXY_LISTEN_PORT}; DIRECT";`,
    '}'
  ].join('\n');
  try {
    fs.writeFileSync(getPacFilePath(), pacContent, 'utf-8');
    writeLogToFile(`PAC file created: ${getPacFilePath()}`);
  } catch (e) {
    writeLogToFile(`PAC file creation failed: ${e.message}`);
  }
}

function deletePacFile() {
  try {
    const pacPath = getPacFilePath();
    if (fs.existsSync(pacPath)) {
      fs.unlinkSync(pacPath);
      writeLogToFile('PAC file deleted.');
    }
  } catch (e) { /* ignore */ }
}

function deletePacFileSync() {
  try {
    const pacPath = getPacFilePath();
    if (fs.existsSync(pacPath)) {
      fs.unlinkSync(pacPath);
    }
  } catch (e) { /* ignore */ }
}

// ─────────────────────────────────────────────────────────
// DNS over HTTPS (DoH) — OS의 DNS 설정을 건드리지 않고
// 프록시 내부에서 자체적으로 도메인을 해석
// ─────────────────────────────────────────────────────────

/**
 * 로컬/내부망 도메인인지 판별. 해당 도메인은 DoH를 우회하여 OS DNS로 해석.
 * 사내망 호환성 보장을 위한 핵심 로직.
 */
function isLocalDomain(hostname) {
  // IP 주소 리터럴은 해석 불필요
  if (net.isIP(hostname)) return true;

  // 단일 라벨 호스트명 (점이 없는 내부 서버명, 예: 'intranet', 'server1')
  if (!hostname.includes('.')) return true;

  const lower = hostname.toLowerCase();

  for (const suffix of DOH_LOCAL_SUFFIXES) {
    if (lower === suffix.substring(1) || lower.endsWith(suffix)) return true;
  }

  return false;
}

/**
 * Cloudflare DoH (https://1.1.1.1/dns-query)를 사용하여 도메인의 A 레코드를 해석.
 * - 로컬 도메인은 DoH를 우회하여 hostname을 그대로 반환 (Node.js가 OS DNS 사용)
 * - DoH 실패 시에도 hostname을 그대로 반환하여 OS DNS로 자동 폴백
 * - TTL 기반 DNS 캐시 적용
 */
function resolveViaDoH(hostname) {
  return new Promise((resolve) => {
    // IP 주소 또는 로컬 도메인은 DoH 우회
    if (isLocalDomain(hostname)) {
      resolve(hostname);
      return;
    }

    // 캐시 조회
    const cached = dnsCache.get(hostname);
    if (cached && cached.expiry > Date.now()) {
      resolve(cached.ip);
      return;
    }

    const url = `${DOH_ENDPOINT}?name=${encodeURIComponent(hostname)}&type=A`;
    const options = {
      headers: { 'Accept': 'application/dns-json' },
      timeout: DOH_TIMEOUT_MS
    };

    const req = https.get(url, options, (res) => {
      let data = '';
      res.on('data', (chunk) => { data += chunk; });
      res.on('end', () => {
        try {
          const json = JSON.parse(data);
          if (json.Status === 0 && json.Answer && json.Answer.length > 0) {
            // type 1 = A 레코드 (IPv4)
            const aRecord = json.Answer.find((a) => a.type === 1);
            if (aRecord && aRecord.data) {
              // TTL: 최소 60초, 최대 600초 (10분)
              const ttl = Math.max(60, Math.min(aRecord.TTL || 300, 600));
              dnsCache.set(hostname, {
                ip: aRecord.data,
                expiry: Date.now() + ttl * 1000
              });
              resolve(aRecord.data);
              return;
            }
          }
        } catch (e) {
          // JSON 파싱 실패 → OS DNS 폴백
        }
        resolve(hostname);
      });
    });

    req.on('error', () => {
      // DoH 요청 실패 → OS DNS 폴백
      resolve(hostname);
    });

    req.on('timeout', () => {
      req.destroy();
      resolve(hostname);
    });
  });
}

// ─────────────────────────────────────────────────────────
// C#으로 빌드된 네이티브 exe를 호출해 시스템 프록시 설정을 5ms 만에 즉시 반영
// ─────────────────────────────────────────────────────────

function refreshProxySettings() {
  return new Promise((resolve) => {
    execFile(refreshExePath, [], { windowsHide: true }, (err) => {
      if (err) {
        // exe가 없거나 오류 시 기존의 느린 powershell 방식으로 폴백
        const refreshCode = '[DllImport("wininet.dll", SetLastError = true)] public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);';
        const refreshCmd = `powershell -NoProfile -NonInteractive -Command "$code = '${refreshCode}'; $type = Add-Type -MemberDefinition $code -Name 'WinInet' -Namespace 'Win32' -PassThru; $type::InternetSetOption(0, 39, 0, 0); $type::InternetSetOption(0, 37, 0, 0)"`;
        exec(refreshCmd, { windowsHide: true }, () => resolve());
      } else {
        resolve();
      }
    });
  });
}

// ─────────────────────────────────────────────────────────
// 8080 포트를 점유 중인 기존 프로세스를 찾아 강제 종료 (EADDRINUSE 오류 완벽 예방)
// ─────────────────────────────────────────────────────────

function freePort8080() {
  return new Promise((resolve) => {
    exec('netstat -ano | findstr :8080', { windowsHide: true }, (err, stdout) => {
      if (err || !stdout) {
        resolve();
        return;
      }
      const lines = stdout.split('\n');
      const pidsToKill = new Set();
      for (const line of lines) {
        if (line.includes('LISTENING') || line.includes('8080')) {
          const parts = line.trim().split(/\s+/);
          const pid = parts[parts.length - 1];
          if (pid && !isNaN(pid) && parseInt(pid) > 0) {
            pidsToKill.add(parseInt(pid));
          }
        }
      }
      
      const killPromises = Array.from(pidsToKill).map((pid) => {
        if (pid === process.pid) return Promise.resolve(); // 자기 자신 제외
        return new Promise((resolveKill) => {
          exec(`taskkill /F /PID ${pid}`, { windowsHide: true }, () => resolveKill());
        });
      });
      
      Promise.all(killPromises).then(() => {
        resolve();
      });
    });
  });
}

// ─────────────────────────────────────────────────────────
// 시스템 프록시 설정 — PAC 파일 기반
// AutoConfigURL에 PAC file:// URL을 등록하여 "PROXY …; DIRECT" 폴백 보장
// ─────────────────────────────────────────────────────────

const REGISTRY_PATH = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings';

function setProxy() {
  return new Promise((resolve) => {
    addLog("SNI 터널링 활성화 (PAC)");

    // 1. PAC 파일 생성
    createPacFile();
    const pacUrl = getPacFileUrl();

    // 2. 수동 프록시 비활성화 (PAC과 충돌 방지)
    exec(`reg add "${REGISTRY_PATH}" /v ProxyEnable /t REG_DWORD /d 0 /f`, { windowsHide: true }, () => {
      // 3. AutoConfigURL에 PAC 파일 경로 등록
      exec(`reg add "${REGISTRY_PATH}" /v AutoConfigURL /t REG_SZ /d "${pacUrl}" /f`, { windowsHide: true }, () => {
        // 4. WinInet 캐시 강제 갱신하여 모든 브라우저에 즉시 반영
        refreshProxySettings().then(resolve);
      });
    });
  });
}

function restoreProxy() {
  return new Promise((resolve) => {
    addLog("터널링 해제");

    // 1. AutoConfigURL 레지스트리 키 삭제
    exec(`reg delete "${REGISTRY_PATH}" /v AutoConfigURL /f`, { windowsHide: true }, () => {
      // 2. 수동 프록시도 확실히 비활성화
      exec(`reg add "${REGISTRY_PATH}" /v ProxyEnable /t REG_DWORD /d 0 /f`, { windowsHide: true }, () => {
        // 3. PAC 파일 삭제
        deletePacFile();
        // 4. WinInet 캐시 갱신
        refreshProxySettings().then(resolve);
      });
    });
  });
}

// ─────────────────────────────────────────────────────────
// TCP 루프백 프록시 서버 — DoH DNS 연동
// ─────────────────────────────────────────────────────────

/**
 * CONNECT 요청을 처리하는 비동기 핸들러.
 * DoH를 통해 대상 호스트의 IP를 해석한 뒤 TCP 터널을 수립.
 */
async function handleConnectRequest(clientSocket, requestBuffer) {
  const requestStr = requestBuffer.toString('utf-8');
  const lines = requestStr.split('\n');
  if (lines.length === 0) {
    clientSocket.destroy();
    return;
  }
  
  const firstLine = lines[0].trim();
  if (!firstLine.includes('CONNECT')) {
    clientSocket.destroy();
    return;
  }
  
  const parts = firstLine.split(' ');
  if (parts.length < 2) {
    clientSocket.destroy();
    return;
  }
  
  const hostPort = parts[1];
  if (!hostPort.includes(':')) {
    clientSocket.destroy();
    return;
  }
  
  const [host, portStr] = hostPort.split(':');
  const port = parseInt(portStr);

  // DoH를 통한 DNS 해석 (실패 시 OS DNS로 자동 폴백)
  const resolvedHost = await resolveViaDoH(host);

  clientSocket.pause(); // 타겟 연결 전까지 읽기 일시 중단

  const targetSocket = net.connect(port, resolvedHost, () => {
    clientSocket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
    clientSocket.resume(); // 클라이언트 소켓 읽기 재개

    let isFirstUpstream = true;

    clientSocket.on('data', (data) => {
      if (isFirstUpstream) {
        isFirstUpstream = false;

        // TLS Record Header(5바이트)를 분할 전송하여 OS의 TCP 결합(Coalescing) 방지 및 SNI 차단 우회
        const splitIndex = Math.min(data.length, 5);
        const firstChunk = data.subarray(0, splitIndex);
        const secondChunk = data.subarray(splitIndex);

        targetSocket.write(firstChunk);
        if (secondChunk.length > 0) {
          clientSocket.pause();
          setTimeout(() => {
            if (!targetSocket.destroyed && !clientSocket.destroyed) {
              targetSocket.write(secondChunk, () => {
                if (!clientSocket.destroyed) {
                  clientSocket.resume();
                }
              });
            } else {
              if (!clientSocket.destroyed) {
                clientSocket.resume();
              }
            }
          }, 5);
        }
      } else {
        targetSocket.write(data);
      }
    });

    targetSocket.on('data', (data) => {
      clientSocket.write(data);
    });
  });

  targetSocket.on('error', () => {
    clientSocket.destroy();
  });

  clientSocket.on('error', () => {
    targetSocket.destroy();
  });

  clientSocket.on('close', () => {
    targetSocket.destroy();
  });

  targetSocket.on('close', () => {
    clientSocket.destroy();
  });
}

function startProxyServer() {
  return new Promise((resolve, reject) => {
    proxyServer = net.createServer((clientSocket) => {
      clientSocket.setNoDelay(true);

      clientSocket.once('data', (requestBuffer) => {
        handleConnectRequest(clientSocket, requestBuffer).catch(() => {
          if (!clientSocket.destroyed) clientSocket.destroy();
        });
      });
    });

    proxyServer.on('error', (err) => {
      reject(err);
    });

    proxyServer.listen(PROXY_LISTEN_PORT, PROXY_LISTEN_IP, () => {
      resolve();
    });
  });
}

function stopProxyServer() {
  if (proxyServer) {
    proxyServer.close();
    proxyServer = null;
  }
}

// ─────────────────────────────────────────────────────────
// 서비스 생명주기
// ─────────────────────────────────────────────────────────

async function startService() {
  try {
    addLog("System initializing...");

    // 빠른 부팅: 일단 포트 바인딩 시도, 충돌(EADDRINUSE) 시에만 netstat(느림)를 통한 프로세스 정리 후 재시도
    try {
      await startProxyServer();
    } catch (err) {
      if (err.code === 'EADDRINUSE') {
        addLog("포트 8080 점유 감지. 충돌 프로세스 정리 중...");
        await freePort8080();
        await startProxyServer();
      } else {
        throw err;
      }
    }

    await setProxy();

    cleanupDone = false; // 새 세션 시작, cleanup 플래그 리셋
    addLog("✓ Service is fully operational.");
    updateStatus(true);
  } catch (err) {
    addLog(`서비스 활성화 오류: ${err.message}`);
    updateStatus(false);
  }
}

async function stopService() {
  try {
    addLog("Stopping services...");
    stopProxyServer();
    await restoreProxy();
    dnsCache.clear();
    addLog("✓ System standby.");
    updateStatus(false);
  } catch (err) {
    addLog(`서비스 해제 오류: ${err.message}`);
  }
}

/**
 * 동기식 긴급 정리. process.on('exit'), uncaughtException 등
 * 비동기 코드를 실행할 수 없는 컨텍스트에서 호출.
 */
function stopServiceSync() {
  try {
    stopProxyServer();

    // PAC 설정 롤백: AutoConfigURL 삭제
    try {
      execSync(`reg delete "${REGISTRY_PATH}" /v AutoConfigURL /f`, { windowsHide: true });
    } catch (e) { /* AutoConfigURL 키가 없으면 무시 */ }

    // 수동 프록시도 확실히 비활성화
    execSync(`reg add "${REGISTRY_PATH}" /v ProxyEnable /t REG_DWORD /d 0 /f`, { windowsHide: true });

    // WinInet 캐시 갱신
    try {
      execSync(`"${refreshExePath}"`, { windowsHide: true });
    } catch (e) {
      const refreshCode = '[DllImport("wininet.dll", SetLastError = true)] public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);';
      execSync(`powershell -NoProfile -NonInteractive -Command "$code = '${refreshCode}'; $type = Add-Type -MemberDefinition $code -Name 'WinInet' -Namespace 'Win32' -PassThru; $type::InternetSetOption(0, 39, 0, 0); $type::InternetSetOption(0, 37, 0, 0)"`, { windowsHide: true });
    }

    // PAC 파일 정리
    deletePacFileSync();

    // DNS 캐시 정리
    dnsCache.clear();
  } catch (e) {
    // 종료 시 오류 무시 — 최대한 정리 시도
  }
}

/**
 * 모든 종료 경로에서 호출되는 통합 정리 함수.
 * 중복 실행 방지(idempotent) — 어떤 이벤트가 먼저 발생해도 안전.
 */
function performCleanup() {
  if (cleanupDone) return;
  cleanupDone = true;
  writeLogToFile("performCleanup: Executing cleanup...");
  stopServiceSync();
  saveServiceState(false);
  writeLogToFile("performCleanup: Cleanup complete.");
}

// ─────────────────────────────────────────────────────────
// 시작 시 자가 복구 (Self-Healing)
// 이전 세션의 비정상 종료로 남은 PAC/프록시 잔여물을 정리
// ─────────────────────────────────────────────────────────

async function startupCleanup() {
  try {
    writeLogToFile("startupCleanup: Starting self-heal cleanup...");
    const prevState = loadServiceState();

    if (prevState.running) {
      writeLogToFile("startupCleanup: 이전 세션 비정상 종료 감지! 긴급 복구 시작...");
      addLog("⚠ 비정상 종료 감지 → 네트워크 설정 복구 중...");
    } else {
      addLog("System initializing...");
    }

    // 1. PAC 프록시 설정 즉시 해제 (인터넷 연결 최우선 복구)
    try {
      exec(`reg delete "${REGISTRY_PATH}" /v AutoConfigURL /f`, { windowsHide: true }, () => {});
    } catch (e) { /* AutoConfigURL이 없으면 무시 */ }
    exec(`reg add "${REGISTRY_PATH}" /v ProxyEnable /t REG_DWORD /d 0 /f`, { windowsHide: true }, () => {
      refreshProxySettings();
    });

    // 2. PAC 파일 잔여물 삭제
    deletePacFileSync();

    // 3. DNS 캐시 초기화
    dnsCache.clear();

    // 4. 상태 초기화
    saveServiceState(false);
    cleanupDone = false; // 정상 시작을 위해 cleanup 플래그 리셋
    addLog(prevState.running ? "✓ 네트워크 설정 복구 완료." : "✓ System standby (Clean start).");
  } catch (e) {
    addLog(`자가 치유 실패: ${e.message}`);
  }
}

// ─────────────────────────────────────────────────────────
// 시스템 트레이 및 윈도우
// ─────────────────────────────────────────────────────────

function createTray() {
  try {
    writeLogToFile(`createTray: Entered. validIconPath = ${validIconPath}`);
    if (validIconPath) {
      tray = new Tray(validIconPath);
      tray.setToolTip('FreeView — 대기 중');
      updateTrayMenu();
      
      tray.on('double-click', () => {
        if (mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.show();
          mainWindow.focus();
        }
      });
      writeLogToFile("createTray: Tray creation complete.");
    } else {
      writeLogToFile("Warning: logo.ico not found, skipping tray icon creation.");
    }
  } catch (e) {
    writeLogToFile(`Tray creation failed: ${e.message}`);
  }
}

function updateTrayMenu() {
  if (!tray) return;
  const statusLabel = isServiceRunning ? '● 보호 활성화' : '○ 대기 중';
  const contextMenu = Menu.buildFromTemplate([
    { label: 'FreeView 2.0.0', enabled: false },
    { label: statusLabel, enabled: false },
    { type: 'separator' },
    { label: '앱 열기', click: () => {
        if (mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.show();
          mainWindow.focus();
        }
      }
    },
    { label: isServiceRunning ? '▣ 서비스 중지' : '▶ 서비스 시작', click: async () => {
        if (isServiceRunning) {
          await stopService();
        } else {
          await startService();
        }
      }
    },
    { type: 'separator' },
    { label: '종료', click: () => {
        performCleanup();
        app.quit();
      }
    }
  ]);
  tray.setContextMenu(contextMenu);
}

async function clearChromiumCaches() {
  writeLogToFile("clearChromiumCaches: Starting cache cleanup...");
  try {
    const userDataPath = app.getPath('userData');
    writeLogToFile(`clearChromiumCaches: UserData path is ${userDataPath}`);
    const cacheDirs = [
      path.join(userDataPath, 'Cache'),
      path.join(userDataPath, 'GPUCache'),
      path.join(userDataPath, 'Code Cache'),
      path.join(userDataPath, 'DawnGraphiteCache'),
      path.join(userDataPath, 'DawnWebGPUCache')
    ];

    const deletePromises = cacheDirs.map(async (dir) => {
      try {
        const exists = await fs.promises.access(dir).then(() => true).catch(() => false);
        if (exists) {
          writeLogToFile(`clearChromiumCaches: Deleting ${dir}...`);
          await fs.promises.rm(dir, { recursive: true, force: true });
          writeLogToFile(`clearChromiumCaches: Deleted ${dir}`);
        }
      } catch (e) {
        writeLogToFile(`clearChromiumCaches: Failed to delete ${dir} - ${e.message}`);
      }
    });

    await Promise.all(deletePromises);
  } catch (err) {
    writeLogToFile(`clearChromiumCaches: Critical failure during cache path resolution - ${err.message}`);
  }
}

function createWindow() {
  writeLogToFile("createWindow: Creating browser window...");
  writeLogToFile("createWindow: Checking tray icon...");
  if (!tray) {
    createTray();
  }
  writeLogToFile("createWindow: Tray check complete.");

  writeLogToFile(`createWindow: Instantiating BrowserWindow. icon path: ${validIconPath || 'undefined'}`);
  mainWindow = new BrowserWindow({
    width: 380,
    height: 600,
    frame: false,
    resizable: false,
    show: false,
    skipTaskbar: true,
    icon: validIconPath || undefined,
    backgroundColor: '#1a1a2e',
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: false,
      preload: path.join(__dirname, 'preload.js')
    }
  });
  writeLogToFile("createWindow: BrowserWindow instantiated.");

  // 콘텐츠가 완전히 렌더링된 후에만 창을 표시 (빈 테두리 깜빡임 방지)
  let windowShown = false;
  const showWindow = () => {
    if (!windowShown && mainWindow && !mainWindow.isDestroyed()) {
      windowShown = true;
      mainWindow.show();
      writeLogToFile("createWindow: Window shown.");
    }
  };
  mainWindow.once('ready-to-show', showWindow);
  // 만약 ready-to-show가 5초 내에 발생하지 않으면 강제로 표시 (안전장치)
  setTimeout(showWindow, 5000);

  if (app.isPackaged) {
    const htmlPath = path.join(__dirname, 'dist', 'index.html');
    writeLogToFile(`createWindow: Loading file: ${htmlPath}`);
    mainWindow.loadFile(htmlPath).catch((err) => {
      writeLogToFile(`createWindow: loadFile FAILED: ${err.message}`);
    });
    writeLogToFile("createWindow: loadFile call dispatched.");
  } else {
    writeLogToFile("createWindow: Loading dev URL http://localhost:3000");
    mainWindow.loadURL('http://localhost:3000');
    writeLogToFile("createWindow: loadURL call complete.");
  }

  mainWindow.webContents.on('console-message', (event, level, message, line, sourceId) => {
    console.log(`[Renderer] ${message} (${sourceId}:${line})`);
  });

  mainWindow.webContents.once('did-finish-load', () => {
    writeLogToFile("createWindow: did-finish-load event received.");
    startupCleanup();
  });

  // 렌더러 프로세스 충돌 진단
  mainWindow.webContents.on('did-fail-load', (event, errorCode, errorDescription) => {
    writeLogToFile(`RENDERER FAIL-LOAD: code=${errorCode} desc=${errorDescription}`);
  });

  mainWindow.webContents.on('render-process-gone', (event, details) => {
    writeLogToFile(`RENDERER GONE: reason=${details.reason} exitCode=${details.exitCode}`);
  });

  mainWindow.on('unresponsive', () => {
    writeLogToFile("WINDOW UNRESPONSIVE detected.");
  });
}

// ─────────────────────────────────────────────────────────
// IPC 핸들러
// ─────────────────────────────────────────────────────────

// X 버튼 클릭 시 완전 종료하는 대신 시스템 트레이(알림 영역)로 창 숨김 처리
ipcMain.on('window-close', () => {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.hide();
  }
});

// 최소화 시에도 트레이로 숨기기
ipcMain.on('window-minimize', () => {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.hide();
  }
});

ipcMain.handle('toggle-service', async () => {
  if (isServiceRunning) {
    await stopService();
  } else {
    await startService();
  }
  return isServiceRunning;
});

ipcMain.handle('get-initial-state', () => {
  return {
    isServiceRunning,
    logs
  };
});

// ─────────────────────────────────────────────────────────
// 앱 초기화 및 Lifecycle 방어
// ─────────────────────────────────────────────────────────

app.whenReady().then(async () => {
  writeLogToFile("app.whenReady: Application initialized. Cleaning caches...");
  await clearChromiumCaches();
  writeLogToFile("app.whenReady: Cache cleanup complete. Spawning window...");
  
  // Windows 시작 프로그램 자동 등록 (비정상 종료 후 부팅 시 네트워크 복구 보장)
  if (app.isPackaged) {
    try {
      app.setLoginItemSettings({ openAtLogin: true, args: ['--startup'] });
      writeLogToFile("app.whenReady: Auto-start registered.");
    } catch (e) {
      writeLogToFile(`app.whenReady: Auto-start registration failed: ${e.message}`);
    }
  }
  
  createWindow();

  // ── powerMonitor Lifecycle 방어 ──
  // 시스템 종료(셧다운) 시 프록시 설정 긴급 롤백
  powerMonitor.on('shutdown', () => {
    writeLogToFile("powerMonitor: System shutdown detected.");
    performCleanup();
  });

  // 절전(Sleep/Hibernate) 진입 시 — PAC DIRECT 폴백이 처리하므로 로그만 기록
  powerMonitor.on('suspend', () => {
    writeLogToFile("powerMonitor: System suspend detected.");
  });

  // 절전 복귀 시
  powerMonitor.on('resume', () => {
    writeLogToFile("powerMonitor: System resumed from suspend.");
  });
});

// 모든 창이 닫혀도 인스톨러 종료 방지 (시스템 트레이 상태 유지)
app.on('window-all-closed', (e) => {
  e.preventDefault();
});

// 앱 종료 직전 — 프록시 설정 롤백 (정상 종료 경로)
app.on('before-quit', () => {
  writeLogToFile("app.on('before-quit'): Performing cleanup...");
  performCleanup();
});

// 앱 종료 확정 — 이중 안전장치
app.on('will-quit', () => {
  writeLogToFile("app.on('will-quit'): Performing cleanup...");
  performCleanup();
});

// ─────────────────────────────────────────────────────────
// 프로세스 이벤트 핸들러 — 최후의 방어선
// ─────────────────────────────────────────────────────────

process.on('exit', () => {
  performCleanup();
});

process.on('SIGINT', () => {
  performCleanup();
  process.exit(0);
});

process.on('SIGTERM', () => {
  performCleanup();
  process.exit(0);
});

process.on('unhandledRejection', (reason, promise) => {
  writeLogToFile(`UNHANDLED REJECTION: ${reason}`);
  // 치명적이지 않은 오류이므로 앱은 유지하되, 안전을 위해 프록시 상태 점검
  try {
    if (isServiceRunning) {
      performCleanup();
      updateStatus(false);
    }
  } catch (e) { /* ignore */ }
});

process.on('uncaughtException', (err) => {
  console.error(err);
  writeLogToFile(`CRITICAL ERROR: ${err.stack || err.message}`);
  // 네트워크 설정 복구 후 앱 유지 (즉시 종료하지 않음)
  try {
    performCleanup();
    updateStatus(false);
  } catch (e) { /* ignore */ }
});
